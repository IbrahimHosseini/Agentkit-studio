#!/usr/bin/env python3
"""Explicitly authorized single-run native smoke, with durable redacted evidence."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time
import uuid
import urllib.parse

TERMINAL = {'completed', 'failed', 'cancelled', 'interrupted'}
SENSITIVE = re.compile(r'^(authorization|proxy.authorization|password|secret|token|api.?key|access.?token|refresh.?token|id.?token|auth.?token|_auth|.*_API_KEY|.*_AUTH_TOKEN)$', re.I)


def redact(value, secrets):
    if isinstance(value, dict):
        return {k: '[REDACTED]' if SENSITIVE.match(k) else redact(v, secrets) for k, v in value.items()}
    if isinstance(value, list):
        return [redact(v, secrets) for v in value]
    if isinstance(value, str):
        for secret in sorted(set(secrets), key=len, reverse=True):
            if secret:
                value = value.replace(secret, '[REDACTED]')
        value = re.sub(r'(?i)(https?://)[^/\s@]+@', r'\1[REDACTED]@', value)
        value = re.sub(r'(?i)([?&](?:access_token|api_key|token|key|signature|credential)=)[^&\s\"<>]+', r'\1[REDACTED]', value)
        value = re.sub(r'(?i)(Bearer\s+)[\w.\-]+', r'\1[REDACTED]', value)
        return value
    return value


def summarize(events, nonce, sample):
    messages = [e['payload']['text'] for e in events if e['kind'] == 'message' and isinstance(e['payload'].get('text'), str)]
    deltas = ''.join(e['payload']['text'] for e in events if e['kind'] == 'text' and isinstance(e['payload'].get('text'), str))
    response = messages[-1] if messages else deltas
    errors = [e['payload']['error'] for e in events if e['payload'].get('error')]
    errors += [e['payload'] for e in events if e['kind'] == 'error']
    return dict(response=response, messages=messages, streamed_text=deltas, errors=errors,
                event_count=len(events), event_kinds=sorted({e['kind'] for e in events}),
                fixture_code_verified=nonce in response, rtl_verified=sample in response)


def claude_config(original_home, source_env):
    config = Path(source_env.get('CLAUDE_CONFIG_DIR', str(original_home / '.claude'))) / 'settings.json'
    data = json.loads(config.read_text()) if config.is_file() else {}
    configured = data.get('env', {})
    keys = ('ANTHROPIC_AUTH_TOKEN', 'ANTHROPIC_API_KEY', 'ANTHROPIC_BASE_URL', 'ANTHROPIC_MODEL',
            'ANTHROPIC_DEFAULT_OPUS_MODEL', 'ANTHROPIC_DEFAULT_SONNET_MODEL', 'ANTHROPIC_DEFAULT_HAIKU_MODEL')
    env = {k: source_env.get(k, configured.get(k)) for k in keys}
    env = {k: v for k, v in env.items() if isinstance(v, str) and v}
    model = env.get('ANTHROPIC_MODEL') or data.get('model')
    if model is not None and not isinstance(model, str):
        raise ValueError('Configured Claude model must be text')
    return config, env, model



def native_claude_token(read_keychain=None, now_ms=None):
    """Read existing sign-in privately; never trigger login or refresh."""
    if read_keychain is None:
        def read_keychain():
            result = subprocess.run(['/usr/bin/security', 'find-generic-password', '-s',
                                     'Claude Code-credentials', '-w'], capture_output=True, text=True, timeout=15)
            if result.returncode:
                raise RuntimeError('Existing native Claude Keychain sign-in is unavailable; login first')
            return result.stdout
    credentials = json.loads(read_keychain()).get('claudeAiOauth', {})
    token = credentials.get('accessToken')
    expiry = credentials.get('expiresAt')
    now_ms = time.time()*1000 if now_ms is None else now_ms
    if not isinstance(token, str) or not token:
        raise RuntimeError('Native Claude sign-in has no access token; login first (no inference submitted)')
    if not isinstance(expiry, (float, int)) or expiry <= now_ms + 120000:
        raise RuntimeError('Native Claude sign-in is expired or expires too soon; login first (no inference submitted)')
    return token, expiry


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--provider', choices=['codex', 'claude'], required=True)
    p.add_argument('--binary', type=Path, required=True)
    p.add_argument('--runtime', type=Path, required=True)
    p.add_argument('--report', type=Path, required=True)
    p.add_argument('--authorize-live', action='store_true')
    p.add_argument('--claude-auth-mode', choices=['configured', 'native'], default='configured')
    p.add_argument('--timeout-seconds', type=int, default=120)
    args = p.parse_args()
    if not args.authorize_live:
        p.error('Explicit per-test authorization is required; no account was accessed')
    if not 1 <= args.timeout_seconds <= 120: p.error('Timeout must be 1–120 seconds')
    binary, runtime = args.binary.resolve(), args.runtime.resolve()
    if not binary.is_file() or not runtime.is_file():
        p.error('Build CLI and locate native runtime first')
    report_path = args.report.resolve()
    evidence = report_path.with_suffix('.evidence')
    if report_path.exists() or evidence.exists():
        p.error('Use a new report path; existing evidence will not be overwritten')
    os.umask(0o077)
    evidence.mkdir(parents=True, mode=0o700)
    secrets = []
    report = {'provider': args.provider, 'limit': 'one native run, no Studio retry', 'acceptance_passed': False,
              'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(), 'runtime': str(runtime),
              'evidence_directory': str(evidence), 'inference_submissions': 0}
    events = []
    nonce, sample = uuid.uuid4().hex, 'متن آزمایشی با نیم‌فاصله و English'
    report['expected'] = {'fixture_code': nonce, 'rtl_sample': sample}
    source_hashes = {}
    scratch_path = None
    def save():
        report_path.write_text(json.dumps(redact(report, secrets), ensure_ascii=False, indent=2) + '\n')
    def record(batch):
        with (evidence / 'events.jsonl').open('a') as f:
            for event in batch:
                events.append(event)
                f.write(json.dumps(redact(event, secrets), ensure_ascii=False) + '\n')
            f.flush()
            os.fsync(f.fileno())
    save()
    try:
        with tempfile.TemporaryDirectory(prefix='studio-live-', dir='/tmp') as scratch:
            home = Path(scratch)
            scratch_path = home
            work = home / 'project'; work.mkdir()
            fixture = work / 'fixture.txt'
            fixture.write_text('CODE=' + nonce + '\nTEXT=' + sample + '\n')
            original = hashlib.sha256(fixture.read_bytes()).hexdigest()
            env = {k: os.environ[k] for k in ('PATH', 'TMPDIR', 'LANG') if k in os.environ}
            env.update(HOME=scratch, USERPROFILE=scratch, CODEX_HOME=str(home/'.codex'),
                       CLAUDE_CONFIG_DIR=str(home/'.claude'), AGENTKIT_DATA_DIR=str(home/'state'),
                       AGENTKIT_ENGINE_BIN=str(binary), CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC='1')
            native_stderr = home/'provider.stderr'
            proxy = home/'native-runtime-proxy'
            proxy.write_text('#!' + sys.executable + '\nimport os,sys\n' +
                             'fd=os.open(' + repr(str(native_stderr)) + ',os.O_WRONLY|os.O_CREAT|os.O_APPEND,0o600)\n' +
                             'os.dup2(fd,2);os.close(fd)\n' +
                             'os.execv(' + repr(str(runtime)) + ',[' + repr(str(runtime)) + ']+sys.argv[1:])\n')
            proxy.chmod(0o700)
            env['AGENTKIT_' + args.provider.upper() + '_BIN'] = str(proxy)
            (home/'.codex').mkdir(); (home/'.claude').mkdir()
            model = None
            if args.provider == 'codex':
                auth_path = Path(os.environ.get('CODEX_HOME', str(Path.home()/'.codex'))) / 'auth.json'
                raw = auth_path.read_bytes(); source_hashes[auth_path] = hashlib.sha256(raw).hexdigest()
                auth = json.loads(raw)
                secrets.extend(v for v in [auth.get('OPENAI_API_KEY'), *(auth.get('tokens') or {}).values()] if isinstance(v, str))
                (home/'.codex/auth.json').write_bytes(raw)
                (home/'.codex/config.toml').write_text('cli_auth_credentials_store = "file"\napproval_policy = "never"\nsandbox_mode = "read-only"\n')
            else:
                config, routing, model = claude_config(Path.home(), os.environ)
                if config.is_file(): source_hashes[config] = hashlib.sha256(config.read_bytes()).hexdigest()
                if args.claude_auth_mode == 'native':
                    model = json.loads(config.read_text()).get('model') if config.is_file() else None
                    token, expiry = native_claude_token()
                    secrets.append(token)
                    env['CLAUDE_CODE_OAUTH_TOKEN'] = token
                    report['auth_source'] = 'existing native Keychain access token, isolated subprocess environment'
                    report['auth_expires_at'] = expiry
                    report['claude_auth_mode'] = 'native'
                else:
                    env.update(routing)
                    secrets.extend(routing[k] for k in ('ANTHROPIC_AUTH_TOKEN', 'ANTHROPIC_API_KEY') if routing.get(k))
                    if not secrets: raise RuntimeError('No existing Claude environment credential; no inference submitted')
                    report['claude_auth_mode'] = 'configured'
                settings = {'permissions': {'allow': ['Read'], 'deny': ['Bash', 'Edit', 'Write', 'Agent']}}
                if model: settings['model'] = model
                (home/'.claude/settings.json').write_text(json.dumps(settings))
            report['selected_model'] = model
            report['route_host'] = urllib.parse.urlsplit(env.get('ANTHROPIC_BASE_URL', '')).hostname if args.provider == 'claude' and args.claude_auth_mode == 'configured' else ('api.anthropic.com' if args.provider == 'claude' else 'native OpenAI account')
            version = subprocess.run([str(runtime), '--version'], env=env, capture_output=True, text=True, timeout=10)
            report['runtime_version'] = version.stdout.strip()
            def rpc(method, params=None):
                proc = subprocess.run([str(binary), '--json', 'rpc', method, '--params', json.dumps(params or {})],
                                      env=env, capture_output=True, text=True, timeout=30)
                if proc.returncode: raise RuntimeError(redact(proc.stderr, secrets))
                return json.loads(proc.stdout)
            run_id = None; cursor = 0; state = {}; daemon = None
            stderr_path = home/'daemon.stderr'
            def drain():
                nonlocal cursor
                for _ in range(1000):
                    page = rpc('subscribe', {'run_id': run_id, 'after_seq': cursor})
                    batch = page['events']
                    if not batch: return
                    record(batch); cursor = batch[-1]['seq']
                raise RuntimeError('Evidence pagination limit exceeded')
            with stderr_path.open('wb') as stderr:
                try:
                    daemon = subprocess.Popen([str(binary), 'daemon'], env=env, stdout=subprocess.DEVNULL, stderr=stderr)
                    deadline = time.monotonic()+10
                    while not (home/'state/engine.token').exists():
                        if daemon.poll() is not None or time.monotonic()>deadline: raise RuntimeError('Isolated daemon did not start')
                        time.sleep(.05)
                    secrets.append((home/'state/engine.token').read_text().strip())
                    if args.provider == 'claude' and args.claude_auth_mode == 'native':
                        settings = rpc('studio/settings')
                        settings['claude_auth_mode'] = 'native'
                        rpc('studio/settings', {'settings': settings})
                    if args.provider == 'codex':
                        try: report['metadata_preflight'] = rpc('studio/models')
                        except Exception as error: report['metadata_preflight_error'] = str(error)
                        save()
                    prompt = 'Read only fixture.txt in the workspace. Return its CODE and TEXT exactly. Use read-only file inspection only. Do not modify files, access network, delegate, or inspect any other files. Keep the answer under 60 words.'
                    report['prompt'] = prompt; save()
                    # Record before submission: a transport timeout must never cause resubmission.
                    report['inference_submissions'] = 1; save()
                    run = rpc('start', {'provider': args.provider, 'workspace': str(work), 'prompt': prompt,
                                       'read_only': True, 'model': model, 'instructions': 'One isolated read-only integration test. No delegation or modifications.', 'attachments': []})
                    run_id = run['id']; report['run_id'] = run_id; save()
                    deadline = time.monotonic()+args.timeout_seconds
                    while time.monotonic()<deadline:
                        state = rpc('get', {'run_id': run_id}); drain()
                        report['status'] = state['status']; save()
                        if state['status'] in TERMINAL: break
                        time.sleep(.3)
                    else:
                        report['timed_out'] = True; rpc('stop', {'run_id': run_id})
                except Exception as error:
                    report['harness_error'] = str(error)
                finally:
                    if run_id:
                        try:
                            if rpc('get', {'run_id': run_id})['status'] not in TERMINAL: rpc('stop', {'run_id': run_id})
                            for _ in range(50):
                                state = rpc('get', {'run_id': run_id}); drain()
                                if state['status'] in TERMINAL: break
                                time.sleep(.1)
                            drain(); report['status'] = state['status']
                        except Exception as error: report['collection_error'] = str(error)
                    if daemon:
                        try: rpc('shutdown')
                        except Exception as error: report['shutdown_error'] = str(error)
                        if daemon.poll() is None:
                            try: daemon.wait(timeout=3)
                            except subprocess.TimeoutExpired:
                                daemon.terminate()
                                try: daemon.wait(timeout=10)
                                except subprocess.TimeoutExpired: daemon.kill(); daemon.wait()
                    stderr.flush()
                    (evidence/'daemon.stderr.log').write_text(redact(stderr_path.read_text(errors='replace'), secrets))
                    (evidence/'provider.stderr.log').write_text(redact(native_stderr.read_text(errors='replace') if native_stderr.exists() else '', secrets))
                    report.update(summarize(events, nonce, sample))
                    report['configuration_evidence'] = [e['payload'] for e in events if e['kind'] == 'execution_config']
                    report['truncated_events'] = [e.get('seq') for e in events if e['payload'].get('truncated')]
                    (evidence/'response.txt').write_text(redact(report['response'], secrets))
                    report['fixture_unchanged'] = hashlib.sha256(fixture.read_bytes()).hexdigest() == original
                    report['original_settings_unchanged'] = all(hashlib.sha256(path.read_bytes()).hexdigest() == digest for path, digest in source_hashes.items())
                    report['acceptance_passed'] = (report.get('status') == 'completed' and report['fixture_code_verified'] and report['rtl_verified'] and report['fixture_unchanged'] and not report.get('harness_error') and not report.get('collection_error') and not report['truncated_events'])
                    save()
        report['temporary_credentials_and_state_removed'] = True
    except Exception as error:
        report['harness_error'] = str(error)
    finally:
        report['temporary_credentials_and_state_removed'] = scratch_path is not None and not scratch_path.exists()
        save()
    print(json.dumps({k: report.get(k) for k in ('provider', 'status', 'acceptance_passed', 'inference_submissions')}, indent=2))
    return 0 if report['acceptance_passed'] else 1

if __name__ == '__main__':
    raise SystemExit(main())
