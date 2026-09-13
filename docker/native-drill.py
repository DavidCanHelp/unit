#!/usr/bin/env python3
"""Native capability drill, using the existing FIFO/REPL Docker process.
Only the isolated unit-native Compose project is created/paused/stopped.
"""
import csv
import json
import os
from pathlib import Path
import re
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]
LOGS = ROOT / 'docker/native-logs'
OUT = ROOT / 'experiments/native'
NETEM = os.environ.get('DRILL_NETEM') == '1'
LABEL = os.environ.get('DRILL_LABEL', '')
assert re.fullmatch(r'[a-zA-Z0-9_-]*', LABEL), 'invalid result label'
SUFFIX = ('-' + LABEL if LABEL else '') + ('-netem' if NETEM else '')
DC = ['docker', 'compose', '-p', 'unit-native', '-f', str(ROOT / 'docker/compose.native.yml')]
checks = []


def command(*args, timeout=40):
    return subprocess.run([*DC, *args], text=True, capture_output=True, check=True, timeout=timeout).stdout


def inject(node, code):
    command('exec', '-T', node, 'inject', code)


def log(node):
    p = LOGS / f'{node}.log'
    return p.read_text(errors='replace') if p.exists() else ''


def wait(predicate, seconds=20):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        result = predicate()
        if result:
            return result
        time.sleep(.1)
    raise AssertionError('condition did not become true within deadline')


def check(name, condition):
    assert condition, name
    checks.append(name)
    print('PASS', name, flush=True)


def node_id(node):
    matches = re.findall(r'Mesh node ([0-9a-f]{16})', log(node))
    return matches[-1] if matches else None


def slot(node, ident):
    inject(node, 'RECRUITS-SEXP')
    matches = re.findall(r'\(recruit-slot :id ' + str(ident) + r' :seq 0 [^\n]+', log(node))
    return matches[-1] if matches else ''


def settled(node, ident, state='ok'):
    s = slot(node, ident)
    return s if f':state {state}' in s else None


def instruction(customers=1000000, version=1):
    return (f'(native :kernel queue-sim :version {version} :arrival 100 :service 80 '
            f':customers {customers} :seed 42 :task queue-sim/v1/100/80/{customers}/42)')


def recruit(node, target, expr):
    ids = re.findall(r'recruit #(\d+) ->', log(node))
    previous = max(map(int, ids), default=0)
    inject(node, f'RECRUIT" {target} {expr}"')
    wait(lambda: f'recruit #{previous + 1} ->' in log(node))
    return previous + 1


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    LOGS.mkdir(parents=True, exist_ok=True)
    try:
        # Fresh logs and ordered DNS/identity readiness; old logs and missing
        # seed DNS are not evidence of a discovered mesh.
        for n in ('root', 'mid', 'leaf1'):
            (LOGS / f'{n}.log').write_text('')
        ids = {}
        for n in ('root', 'mid', 'leaf1'):
            command('up', '-d', n, timeout=60)
            ids[n] = wait(lambda n=n: node_id(n))
        qdiscs = {n: command('exec', '-T', n, 'tc', 'qdisc', 'show', 'dev', 'eth0').strip()
                  for n in ('root', 'mid', 'leaf1')}
        if NETEM:
            for n, qdisc in qdiscs.items():
                check(f'netem delay and loss active on {n}', 'netem' in qdisc and 'delay' in qdisc and 'loss 1%' in qdisc)
        for n in ('mid', 'leaf1'):
            inject(n, 'NATIVE-ON')
        # Benchmark driver refuses to start until exact native capabilities are known.
        bench = command('exec', '-T', '-e', 'UNIT_BENCH_NATIVE_WINDOW=' + os.environ.get('UNIT_BENCH_NATIVE_WINDOW', '1'), 'root', 'unit', '--bench-native', '--peers', 'mid:4200,leaf1:4200', timeout=120)
        rows = [r for r in csv.reader(bench.splitlines()) if len(r) == 7 and r[-1] == 'true']
        check('36 native benchmark runs checked, with serial/thread/three-container modes', len(rows) == 36)
        with (OUT / f'docker{SUFFIX}.csv').open('w') as f:
            writer = csv.writer(f)
            writer.writerow(['mode', 'customers', 'tasks', 'repeat', 'elapsed_ms', 'remote', 'correct'])
            writer.writerows(rows)
        inject('root', 'NATIVE-ON')
        # Give the ordinary root's gossip a chance to learn native capabilities.
        time.sleep(2)

        bad = recruit('root', ids['mid'], instruction(version=2))
        check('unsupported kernel version is an explicit failure', ':kind "native"' in wait(lambda: settled('root', bad, 'err')))

        # Burst two independent recruiters at one worker, each in a single Forth
        # input line, so command-launch latency cannot hide queue saturation.
        burst = ' '.join(f'RECRUIT" {ids["mid"]} {instruction(20000000)}"' for _ in range(9))
        senders = [subprocess.Popen([*DC, 'exec', '-T', n, 'inject', burst], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                   for n in ('root', 'leaf1')]
        for p in senders:
            p.communicate(timeout=20)
            assert p.returncode == 0
        for ident in range(2, 11):
            wait(lambda ident=ident: settled('root', ident))
        for ident in range(1, 10):
            wait(lambda ident=ident: settled('leaf1', ident))
        def idle_status():
            inject('mid', 'NATIVE-STATUS')
            reports = re.findall(r'\(native-status [^\n]+', log('mid'))
            return reports[-1] if reports and ':pending 0 ' in reports[-1] else None
        status = wait(idle_status)
        check('two recruiters finish all 18 admitted-or-retried tasks', True)
        check('worker queue never exceeds three outstanding tasks', ':high-water 3 ' in status and ':pending 0 ' in status)
        check('full worker explicitly returned busy', int(re.search(r':busy (\d+)', status)[1]) > 0)
        # Equal input identities return exactly equal values across all slots.
        results = [re.search(r':value (\([^)]*\))', slot('root', i))[1] for i in range(2, 11)]
        check('replayed deterministic tasks have identical numerical results', len(set(results)) == 1)

        command('pause', 'mid')
        wedged = recruit('root', ids['mid'], instruction())
        before = time.monotonic()
        first = wait(lambda: settled('root', wedged), 15)
        elapsed = time.monotonic() - before
        check('paused worker is re-recruited by the existing supervisor', int(re.search(r':reassigned (\d+)', first)[1]) >= 1)
        command('unpause', 'mid')
        time.sleep(2)
        check('late worker reply leaves settled result unchanged', slot('root', wedged) == first)
        command('exec', '-T', 'mid', 'pgrep', '-x', 'unit')
        check('the resumed unit is still an organism, not cancelled work', True)

        command('pause', 'mid')
        killed = recruit('root', ids['mid'], instruction())
        command('kill', '-s', 'KILL', 'mid')
        final = wait(lambda: settled('root', killed), 15)
        check('killed worker task is recovered with the same result', re.search(r':value (\([^)]*\))', first)[1] == re.search(r':value (\([^)]*\))', final)[1])
        # The working units retain dictionary and ordinary Forth behavior.
        inject('root', ': STILL-UNIT 111 222 + ; STILL-UNIT .')
        wait(lambda: '333' in log('root'))
        check('Forth remains responsive and definable after worker failure', True)
        (OUT / f'drill{SUFFIX}.json').write_text(json.dumps({'passed': checks, 'wedge_recovery_seconds': elapsed,
            'worker_status': status, 'native_window': int(os.environ.get('UNIT_BENCH_NATIVE_WINDOW', '1')), 'netem': os.environ.get('DRILL_NETEM', '0'), 'qdiscs': qdiscs,
            'scope': 'three Docker containers on one Docker host; existing REPL, gossip, recruitment and supervision'}, indent=2) + '\n')
    finally:
        subprocess.run([*DC, 'unpause', 'mid'], capture_output=True)
        subprocess.run([*DC, 'down', '-t', '2'], capture_output=True)


if __name__ == '__main__':
    main()
