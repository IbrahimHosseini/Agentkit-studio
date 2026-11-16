#!/usr/bin/env python3
"""Run Studio checks with isolated application homes and no inherited provider secrets.

Uses the installed Rust toolchain. CARGO_HOME/RUSTUP_HOME/CARGO_TARGET_DIR may
point to existing offline caches. Does not install dependencies or run models.
"""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]

def main():
    original_home = Path.home()
    with tempfile.TemporaryDirectory(prefix='studio-verify-', dir='/tmp' if os.name == 'posix' else None) as scratch:
        env = {key: os.environ[key] for key in ('PATH', 'TMPDIR', 'LANG', 'SYSTEMROOT', 'SDKROOT', 'DEVELOPER_DIR') if key in os.environ}
        env.update(HOME=scratch, USERPROFILE=scratch, CODEX_HOME=scratch+'/.codex', CLAUDE_CONFIG_DIR=scratch+'/.claude', AGENTKIT_DATA_DIR=scratch+'/state', PYTHONDONTWRITEBYTECODE='1', npm_config_cache=scratch+'/npm-cache', npm_config_update_notifier='false')
        env['CARGO_HOME'] = os.environ.get('CARGO_HOME', str(original_home/'.cargo'))
        env['RUSTUP_HOME'] = os.environ.get('RUSTUP_HOME', str(original_home/'.rustup'))
        env['CARGO_TARGET_DIR'] = os.environ.get('CARGO_TARGET_DIR', str(ROOT/'target'))
        env['PATH'] = str(Path(env['CARGO_HOME'])/'bin') + os.pathsep + env.get('PATH','')
        binary = Path(env['CARGO_TARGET_DIR'])/'debug'/('agentkit.exe' if os.name=='nt' else 'agentkit')
        env['AGENTKIT_TEST_BIN'] = str(binary)
        commands = [
            ['cargo','test','--offline','--locked','--workspace'],
            ['npm','--prefix','apps/desktop','run','check'],
            ['node','--test',*[str(path.relative_to(ROOT)) for folder in ('src','devtools') for path in sorted((ROOT/'apps/desktop'/folder).rglob('*.test.mjs'))]],
            ['npm','--prefix','apps/desktop','run','build'],
            ['cargo','build','--offline','--locked','-p','agentkit-cli'],
            [sys.executable,'-I','-B','-m','unittest','discover','-s','tests/phase1','-v'],
            [sys.executable,'-I','-B','-m','unittest','discover','-s','tests/phase2','-v'],
        ]
        for command in commands:
            print('\nRUN '+ ' '.join(command), flush=True)
            if not shutil.which(command[0],path=env['PATH']):
                print('Required executable unavailable: '+command[0],file=sys.stderr)
                return 2
            result = subprocess.run(command,cwd=ROOT,env=env)
            if result.returncode:
                return result.returncode
    return 0

if __name__=='__main__':
    raise SystemExit(main())
