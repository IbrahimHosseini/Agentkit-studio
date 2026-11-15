#!/usr/bin/env python3
"""Sample explicit app roots and their descendants using the host's POSIX ps.

This is a measurement helper, not a benchmark claim. WebView services owned by
OS brokers may require additional explicit --root PIDs. RSS sums double-count
shared pages; CPU is ps's platform-specific estimate. Windows is unsupported.
"""
import argparse
import json
import os
from pathlib import Path
import platform
import subprocess
import time


def snapshot(roots):
    raw = subprocess.check_output(['ps', '-axo', 'pid=,ppid=,rss=,pcpu=,comm='], text=True)
    processes = {}
    for line in raw.splitlines():
        fields = line.split(None, 4)
        if len(fields) != 5:
            continue
        pid, parent, rss, cpu, command = fields
        processes[int(pid)] = {'pid': int(pid), 'ppid': int(parent), 'rss_kib': int(rss), 'cpu_percent_ps': float(cpu), 'command': command}
    selected = set(roots).intersection(processes)
    while True:
        children = {pid for pid, process in processes.items() if process['ppid'] in selected}
        if children <= selected:
            break
        selected.update(children)
    return [processes[pid] for pid in sorted(selected)]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=int, action='append', required=True, help='Explicit app/engine/WebView root PID; repeat as needed')
    parser.add_argument('--label', required=True, help='For example idle-ui, active-mock, long-history, after-stop')
    parser.add_argument('--samples', type=int, default=5)
    parser.add_argument('--interval', type=float, default=1)
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    if os.name != 'posix':
        parser.error('This helper supports POSIX ps only; Windows measurements are unverified')
    if not 1 <= args.samples <= 120 or not 0.05 <= args.interval <= 60 or any(pid < 1 for pid in args.root):
        parser.error('Use 1–120 samples, interval 0.05–60 seconds, and positive root PIDs')
    result = {'label': args.label, 'platform': platform.platform(), 'roots': args.root, 'measurement': 'POSIX ps; RSS sums include shared-page double counting; CPU estimates are platform-specific', 'coverage': 'Explicit roots and PPID descendants only. OS-brokered WebView services can be missing.', 'samples': []}
    for index in range(args.samples):
        processes = snapshot(args.root)
        result['samples'].append({'time_unix': time.time(), 'rss_sum_kib': sum(process['rss_kib'] for process in processes), 'cpu_sum_percent_ps': sum(process['cpu_percent_ps'] for process in processes), 'processes': processes})
        if index + 1 < args.samples:
            time.sleep(args.interval)
    text = json.dumps(result, ensure_ascii=False, indent=2) + '\n'
    if args.output:
        args.output.write_text(text, encoding='utf-8')
    else:
        print(text, end='')


if __name__ == '__main__':
    main()
