"""Opt-in measurement of Windows Cargo target selection, never a release gate."""

import argparse
from collections import defaultdict
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time

from windows_native_shard import SHARDS, inventory_key, owner
from windows_webview_inputs import ci_identity, frontend_receipts


FEATURE = 'cumulus-desktop/custom-protocol'
PACKAGES = {'cumulus-core': 'core', 'cumulus-audio-filter': 'core/audio-filter',
            'cumulus-desktop': 'desktop'}
MODES = ('reference', 'selective')
FLAGS = {'test': '--test', 'example': '--example', 'bin': '--bin'}
LIBRARY = ('--lib',)
NAME = re.compile(r'[A-Za-z0-9_-]+')
SCHEMA = 1


def need(condition, message):
    if not condition:
        raise ValueError(message)


def cargo_metadata(repo):
    return json.loads(subprocess.check_output([
        'cargo', 'metadata', '--format-version=1', '--no-deps', '--locked', '--offline',
        '--features', FEATURE,
    ], cwd=repo))


def plan_targets(metadata, repo):
    """Keep metadata test:false examples: --all-targets runs their harnesses."""
    ids, targets, selectors = {}, {}, defaultdict(list)
    for package in metadata['packages']:
        cwd = Path(package['manifest_path']).resolve().parent.relative_to(repo).as_posix()
        need(PACKAGES.get(package['name']) == cwd and package['id'] not in ids,
             'pilot package set or location changed')
        ids[package['id']] = cwd
        for target in package['targets']:
            kinds, name = target['kind'], target['name']
            if kinds == ['custom-build']:
                continue
            required = target.get('required-features', [])
            need(not required or (cwd == 'desktop' and required == ['custom-protocol']),
                 f'review unsupported required features for {cwd}/{name}: {required}')
            need(NAME.fullmatch(name) is not None, f'unsupported Cargo target name: {name}')
            if kinds in (['lib'], ['cdylib', 'rlib']):
                key = (cwd, 'deps/' + name.replace('-', '_'))
                selector = LIBRARY
            else:
                need(len(kinds) == 1 and kinds[0] in FLAGS, f'unsupported target kind: {kinds}')
                key = (cwd, ('examples/' if kinds == ['example'] else 'deps/') + name)
                selector = (FLAGS[kinds[0]], name)
            need(key not in targets, f'ambiguous normalized Cargo target: {key}')
            targets[key] = selector
            selectors[selector].append(key)
    need(set(ids) == set(metadata['workspace_members']) and set(ids.values()) == set(PACKAGES.values())
         and len(ids) == len(PACKAGES), 'pilot requires the complete known workspace')

    # The existing heavy-suite policy owns singleton targets. With --workspace,
    # --lib and repeated selector names apply to every matching package.
    assigned = {key: owner(key) for key in targets}
    for key in selectors[LIBRARY]:
        assigned[key] = owner(('core', 'deps/cumulus_core'))
    for selector, keys in sorted(selectors.items()):
        candidates = {assigned[key] for key in keys}
        if len(candidates) > 1:
            shard = min(candidates, key=lambda part: (list(assigned.values()).count(part), part))
            for key in keys:
                assigned[key] = shard
    for shard in SHARDS:
        need({key[0] for key in targets if assigned[key] == shard} == set(PACKAGES.values()),
             f'pilot shard {shard} must still select tests from every workspace package')
    return ids, targets, assigned


def cargo_command(targets, assigned, mode, shard):
    args = ['cargo', 'test', '--workspace', '--no-fail-fast', '--locked',
            '--features', FEATURE, '--message-format=json']
    if mode == 'reference':
        return [*args, '--all-targets']
    selectors = sorted({selector for key, selector in targets.items() if assigned[key] == shard})
    return [*args, *(arg for selector in selectors for arg in selector)]


def json_value(path):
    return json.loads(Path(path).read_text(encoding='utf-8'))


def json_write(path, value):
    with Path(path).open('x', encoding='utf-8') as output:
        json.dump(value, output, sort_keys=True)
        output.write('\n')


def repo_identity(repo):
    identity = ci_identity(os.environ)
    head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=repo, text=True).strip()
    need(head == identity['commit'], 'checkout does not match pilot attempt')
    return identity


def artifact_facts(row, package_ids, known):
    cwd = package_ids.get(row.get('package_id'))
    if not cwd or not row.get('executable') or row['profile'].get('test') is not True:
        return None
    target = row['target']
    name = target['name'].replace('-', '_') if target['kind'] in (['lib'], ['cdylib', 'rlib']) else target['name']
    key = (cwd, ('examples/' if target['kind'] == ['example'] else 'deps/') + name)
    need(key in known, f'unexpected compiled test harness: {key}')
    return key, {'target': list(key), 'features': sorted(row['features']), 'profile': row['profile']}


def elapsed(start):
    return round((time.monotonic() - start) * 1000)


def run_target(executable, args):
    """Invoked by Cargo in its normal package directory and runtime environment."""
    key = inventory_key(executable)
    plan = json_value(os.environ['CUMULUS_NATIVE_PILOT_PLAN'])
    mode, shard = plan['mode'], plan['shard']
    planned = {tuple(target): part for target, part in plan['targets']}
    need(key in planned, f'Cargo invoked an unknown pilot target: {key}')
    execute = planned[key] == shard
    need(execute or mode == 'reference', f'selective command admitted a different shard: {key}')
    start = time.monotonic()
    status = subprocess.run([executable, *args], check=False, stdout=sys.stderr).returncode if execute else None
    record = {'target': list(key), 'args': args, 'status': status, 'elapsedMs': elapsed(start) if execute else None}
    with Path(os.environ['CUMULUS_NATIVE_PILOT_LOG']).open('a', encoding='utf-8') as output:
        output.write(json.dumps(record) + '\n')
    return status or 0


def run_cargo(command, env, ids, known):
    started = time.monotonic()
    artifacts, compiled = {}, None
    print('+', ' '.join(command), flush=True)
    with subprocess.Popen(command, env=env, stdout=subprocess.PIPE, text=True, encoding='utf-8') as cargo:
        for line in cargo.stdout:
            if not line.startswith('{'):
                print(line, end='', flush=True)
                continue
            row = json.loads(line)
            reason = row.get('reason')
            if reason == 'compiler-message':
                print(row['message'].get('rendered') or row['message']['message'], file=sys.stderr)
            elif reason == 'compiler-artifact':
                fact = artifact_facts(row, ids, known)
                if fact:
                    key, value = fact
                    need(key not in artifacts, f'duplicate compiled test harness: {key}')
                    artifacts[key] = value
            elif reason == 'build-finished':
                need(compiled is None, 'Cargo reported a second build boundary')
                compiled = elapsed(started)
        status = cargo.wait()
    need(status == 0 and compiled is not None, f'Cargo pilot failed: exit {status}')
    return list(artifacts.values()), compiled, elapsed(started)


def phase_time(value):
    return type(value) is int and value >= 0


def verify_shard(receipt, identity, mode, shard, assigned):
    need(receipt['schema'] == SCHEMA and receipt['mode'] == mode and receipt['shard'] == shard
         and all(receipt[key] == value for key, value in identity.items()), f'wrong pilot receipt for {shard}')
    selected = {key for key, part in assigned.items() if part == shard}
    expected = set(assigned) if mode == 'reference' else selected
    inventory = receipt['inventory']
    observed = {tuple(row['target']): row for row in inventory}
    artifacts = {tuple(row['target']): row for row in receipt['artifacts']}
    need(len(observed) == len(inventory) and observed.keys() == expected, f'wrong Cargo runner inventory for {shard}')
    need(len(artifacts) == len(receipt['artifacts']) and artifacts.keys() == expected,
         f'wrong compiled harness inventory for {shard}')
    for key, row in observed.items():
        need(row['args'] == [], f'pilot Cargo passed extra test arguments to {key}')
        need(artifacts[key]['profile']['test'] is True, f'non-test executable in pilot: {key}')
        if key in selected:
            need(type(row['status']) is int and row['status'] == 0 and phase_time(row['elapsedMs']),
                 f'pilot did not execute {key} successfully')
        else:
            need(row['status'] is None and row['elapsedMs'] is None, f'wrong shard executed {key}')
    need(phase_time(receipt['compileMs']) and phase_time(receipt['totalMs'])
         and receipt['totalMs'] >= receipt['compileMs'], 'invalid pilot phase timings')
    return observed, artifacts, selected


def execute_shard(repo, mode, shard, output):
    need(os.name == 'nt', 'run the hosted pilot on Windows')
    identity = repo_identity(repo)
    ids, targets, assigned = plan_targets(cargo_metadata(repo), repo)
    frontend = hashlib.sha256(json.dumps(frontend_receipts(repo/'frontend/dist'), sort_keys=True).encode()).hexdigest()
    toolchain = {program: subprocess.check_output([program, '-vV'], cwd=repo, text=True).strip()
                 for program in ('rustc', 'cargo')}
    installed = json_value(repo/'.build/windows-media/installed.json')
    need(installed['filterLoaded'] is True and installed['nativePcmVerified'] is True,
         'pilot requires the verified native media runtime')
    media = {key: installed[key] for key in ('key', 'package')}
    with tempfile.TemporaryDirectory(prefix='cumulus-windows-native-pilot-', dir=os.environ['RUNNER_TEMP']) as tmp:
        directory = Path(tmp)
        plan, log = directory/'plan.json', directory/'executed.jsonl'
        json_write(plan, dict(mode=mode, shard=shard, targets=sorted(assigned.items())))
        env = {**os.environ, 'CUMULUS_NATIVE_PILOT_PLAN': str(plan), 'CUMULUS_NATIVE_PILOT_LOG': str(log),
               'CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUNNER': 'python -m windows_native_selectors_pilot target'}
        artifacts, compiled, total = run_cargo(cargo_command(targets, assigned, mode, shard), env, ids, targets)
        inventory = [json.loads(line) for line in log.read_text(encoding='utf-8').splitlines()]
    result = dict(schema=SCHEMA, **identity, mode=mode, shard=shard, frontend=frontend,
                  toolchain=toolchain, media=media,
                  dependencyCacheKey=os.environ['CUMULUS_NATIVE_PILOT_CACHE_KEY'],
                  compileMs=compiled, totalMs=total, inventory=inventory, artifacts=artifacts)
    verify_shard(result, identity, mode, shard, assigned)
    json_write(output, result)


def aggregate(receipts, identity, mode, assigned):
    executed, all_artifacts, timing = {}, {}, {}
    need(len(receipts) == len(SHARDS), 'expected one report per pilot shard')
    for shard, receipt in zip(SHARDS, receipts, strict=True):
        observed, artifacts, selected = verify_shard(receipt, identity, mode, shard, assigned)
        need(not executed.keys() & selected, 'pilot executed a target twice')
        executed.update((key, observed[key]) for key in selected)
        for key, facts in artifacts.items():
            need(key not in all_artifacts or all_artifacts[key] == facts, f'inconsistent compile profile for {key}')
            all_artifacts[key] = facts
        timing[shard] = dict(targets=len(selected), compileMs=receipt['compileMs'],
                             testMs=receipt['totalMs']-receipt['compileMs'], totalMs=receipt['totalMs'],
                             dependencyCacheKey=receipt['dependencyCacheKey'])
    need(executed.keys() == all_artifacts.keys() == assigned.keys(), 'pilot did not execute the full current workspace')
    first = receipts[0]
    for receipt in receipts[1:]:
        need(all(receipt[field] == first[field] for field in ('frontend', 'toolchain', 'media')),
             'pilot shards used different toolchains, native media or frontend bytes')
    return dict(schema=SCHEMA, **identity, mode=mode, frontend=first['frontend'],
                toolchain=first['toolchain'], media=first['media'],
                assignment=sorted(assigned.items()), inventory=[executed[key] for key in sorted(executed)],
                artifacts=[all_artifacts[key] for key in sorted(all_artifacts)], timing=timing)


def compare(reference, selective):
    need(reference['schema'] == selective['schema'] == SCHEMA, 'unsupported pilot report schema')
    need(reference['mode'] == 'reference' and selective['mode'] == 'selective', 'compare reference then selective')
    need(reference['ciRun'] != selective['ciRun'], 'use separate reference and selective pilot runs')
    for field in ('commit', 'frontend', 'toolchain', 'media', 'assignment', 'artifacts'):
        need(reference[field] == selective[field], f'pilot reports differ in {field}')
    for report in (reference, selective):
        # Even identical reports must cover their verified assignment completely.
        assigned = {tuple(key) for key, _ in report['assignment']}
        actual = [tuple(row['target']) for row in report['inventory']]
        compiled = [tuple(row['target']) for row in report['artifacts']]
        need(len(actual) == len(compiled) == len(assigned) and set(actual) == set(compiled) == assigned
             and all(type(row['status']) is int and row['status'] == 0 and row['args'] == []
                     for row in report['inventory']), 'incomplete comparative target coverage')
    def phases(report):
        return {phase: max(report['timing'][shard][phase] for shard in SHARDS)
                for phase in ('compileMs', 'testMs', 'totalMs')}
    return dict(targets=len(reference['inventory']), reference=phases(reference), selective=phases(selective),
                sameDependencyCacheKeys=all(reference['timing'][shard]['dependencyCacheKey']
                    == selective['timing'][shard]['dependencyCacheKey'] for shard in SHARDS))


def main():
    if sys.argv[1:2] == ['target']:
        sys.exit(run_target(sys.argv[2], sys.argv[3:]))
    parser = argparse.ArgumentParser(description=__doc__)
    actions = parser.add_subparsers(dest='operation', required=True)
    run = actions.add_parser('run')
    run.add_argument('mode', choices=MODES)
    run.add_argument('shard', choices=SHARDS)
    run.add_argument('output', type=Path)
    check = actions.add_parser('verify')
    check.add_argument('mode', choices=MODES)
    check.add_argument('output', type=Path)
    check.add_argument('receipts', type=Path, nargs=3)
    comparison = actions.add_parser('compare')
    comparison.add_argument('reference', type=Path)
    comparison.add_argument('selective', type=Path)
    args = parser.parse_args()
    if args.operation == 'compare':
        print(json.dumps(compare(json_value(args.reference), json_value(args.selective)), indent=2))
    else:
        repo = Path.cwd().resolve()
        if args.operation == 'run':
            execute_shard(repo, args.mode, args.shard, args.output)
        else:
            identity = repo_identity(repo)
            _, _, assigned = plan_targets(cargo_metadata(repo), repo)
            result = aggregate([json_value(path) for path in args.receipts], identity, args.mode, assigned)
            json_write(args.output, result)
            print(json.dumps({'targets': len(result['inventory']), 'timing': result['timing']}, indent=2))


if __name__ == '__main__':
    main()
