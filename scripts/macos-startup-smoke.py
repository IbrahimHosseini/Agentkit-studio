#!/usr/bin/env python3
"""Opt-in native macOS startup timing checks. Fake providers, no accounts."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bundle', type=Path, required=True)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--native', action='store_true', help='Explicitly open test windows')
    args = parser.parse_args()
    if sys.platform != 'darwin' or not args.native:
        parser.error('Use --native on macOS; no model request is made')
    report = args.report.resolve()
    report.parent.mkdir(parents=True, exist_ok=True)
    os.environ['AGENTKIT_TEST_BIN'] = str(args.binary.resolve())
    spec = importlib.util.spec_from_file_location('studio_fixture', ROOT/'tests/phase2/test_studio.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    results = []
    with tempfile.TemporaryDirectory(prefix='studio-startup-', dir='/tmp') as directory:
        bundle = Path(directory)/'Startup QA.app'
        shutil.copytree(args.bundle, bundle, symlinks=True)
        info_path = bundle/'Contents/Info.plist'
        info = plistlib.loads(info_path.read_bytes())
        info['CFBundleIdentifier'] = 'dev.agentkit.studio.startupqa'
        info['CFBundleName'] = 'Startup QA'
        info_path.write_bytes(plistlib.dumps(info))
        executable = bundle/'Contents/MacOS'/info['CFBundleExecutable']
        fixture = None
        try:
            # Fresh means application homes/state, not a flushed OS/WebKit cache.
            for index, mode in enumerate(['fresh', 'warm', 'warm', 'fresh', 'warm', 'engine-unavailable']):
                if fixture is None or mode == 'fresh':
                    if fixture:
                        fixture.doCleanups()
                    fixture = module.StudioTests()
                    fixture.setUp()
                env = fixture.env.copy()
                env['AGENTKIT_STARTUP_DIAGNOSTICS'] = '1'
                if mode == 'engine-unavailable':
                    env['AGENTKIT_DATA_DIR'] = str(Path(directory)/'offline-state')
                    env['AGENTKIT_ENGINE_BIN'] = str(Path(directory)/'missing-engine')
                log = report.with_name(f'{report.stem}-{index+1}.log')
                started = time.monotonic()
                with log.open('wb') as stderr:
                    process = subprocess.Popen([str(executable)], env=env, stdout=subprocess.DEVNULL, stderr=stderr)
                    try:
                        while time.monotonic()-started < 12:
                            text = log.read_text(errors='replace')
                            if 'window-visible' in text or 'startup-failed' in text or process.poll() is not None:
                                break
                            time.sleep(.05)
                        text = log.read_text(errors='replace')
                        stages = {}
                        for line in text.splitlines():
                            parts = line.split()
                            if len(parts) == 3 and parts[0] == 'studio-startup':
                                stages[parts[2]] = int(parts[1].removesuffix('ms'))
                        passed = ('frontend-mounted' in stages and 'window-visible' in stages
                                  and stages['frontend-mounted'] <= stages['window-visible'] < 10000
                                  and 'startup-failed' not in stages)
                        results.append({'mode': mode, 'passed': passed, 'stages_ms': stages,
                                        'log': str(log), 'process_exit': process.poll()})
                        report.write_text(json.dumps({'platform': 'macOS', 'provider_mode': 'offline fixtures',
                            'real_provider_requests': 0, 'runs': results,
                            'all_passed': len(results) == 6 and all(r['passed'] for r in results)}, indent=2)+'\n')
                        print(json.dumps(results[-1]), flush=True)
                        if not passed:
                            return 1
                    finally:
                        if process.poll() is None:
                            process.terminate()
                            try:
                                process.wait(timeout=10)
                            except subprocess.TimeoutExpired:
                                process.kill()
                                process.wait(timeout=5)
        finally:
            if fixture:
                fixture.doCleanups()
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
