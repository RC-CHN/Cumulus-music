"""Plan Windows Cargo test executables and verify coverage without building any."""

import argparse
from dataclasses import dataclass
import json
from pathlib import Path
import re
import subprocess
import tomllib

from windows_native_shard import SHARDS, owner, provenance


FEATURE = 'cumulus-desktop/custom-protocol'
WORKSPACE_PACKAGES = {
    'core': 'cumulus-core',
    'core/audio-filter': 'cumulus-audio-filter',
    'desktop': 'cumulus-desktop',
}
NAMED_SELECTORS = {'bin': '--bin', 'example': '--example', 'test': '--test'}
KEY_NAME = re.compile(r'[A-Za-z0-9_-]+')
PINNED_VERSION = re.compile(r'\d+\.\d+\.\d+')


@dataclass(frozen=True)
class ShardPlan:
    targets: tuple[tuple[str, str], ...]
    selectors: tuple[str, ...]


def unsupported(message):
    return ValueError(f'{message}; review against the full Cargo --all-targets inventory')


def is_library(kinds):
    return tuple(kinds) == ('lib',) or sorted(kinds) == ['cdylib', 'rlib']


def package_targets(package, package_path):
    package_name = package['name']
    features = package['features']
    enabled = {'custom-protocol'} if package_name == 'cumulus-desktop' else set()
    if features.get('default') or not enabled <= features.keys():
        raise unsupported(f'unhandled feature defaults in {package_name}')
    libraries = 0
    for target in package['targets']:
        name = target['name']
        kinds = tuple(target['kind'])
        if kinds == ('custom-build',):
            # Build scripts are dependencies of Cargo targets, not run by the
            # target runner. Do not assign a separate execution receipt.
            continue
        if not isinstance(name, str) or not KEY_NAME.fullmatch(name):
            raise unsupported(f'unhandled Cargo executable name {name!r}')
        if type(target['test']) is not bool:
            raise unsupported(f'invalid Cargo test flag for {package_name}/{name}')
        required = target.get('required-features', [])
        if (not isinstance(required, list) or not all(isinstance(value, str) for value in required)
                or not set(required) <= enabled):
            # Do not guess what Cargo will silently omit under --all-targets.
            raise unsupported(f'unhandled required features for {package_name}/{name}: {required!r}')
        if is_library(kinds):
            libraries += 1
            selector = ('--lib',)
        elif len(kinds) == 1 and kinds[0] in NAMED_SELECTORS:
            kind = kinds[0]
            if kind == 'test' and not target['test']:
                raise unsupported(f'disabled integration test {package_name}/{name}')
            # --all-targets selects examples even with test=false. Explicit
            # --example name must preserve that test-mode executable.
            selector = (NAMED_SELECTORS[kind], name)
        else:
            # Bench inclusion depends on flags not described by current
            # metadata. Fail closed on new kinds before changing CI coverage.
            raise unsupported(f'unhandled Cargo target kind {package_name}/{name}: {kinds!r}')
        directory = 'examples' if kinds == ('example',) else 'deps'
        executable = name.replace('-', '_') if selector == ('--lib',) else name
        yield (package_path, f'{directory}/{executable}'), selector
    if libraries != 1:
        raise unsupported(f'expected one Cargo library for {package_name}; found {libraries}')


def plan_targets(metadata, workspace):
    """Derive exact runner keys, selectors and ownership from Cargo metadata."""
    workspace = Path(workspace).resolve()
    if Path(metadata['workspace_root']).resolve() != workspace:
        raise ValueError('Cargo metadata is from another workspace')
    ids = metadata['workspace_members']
    packages = metadata['packages']
    if (not isinstance(ids, list) or not isinstance(packages, list)
            or len(ids) != len(set(ids)) or len(packages) != len(ids)
            or {package['id'] for package in packages} != set(ids)):
        raise unsupported('expected Cargo --no-deps workspace metadata')
    groups = {}
    targets = {}
    package_ids = {}
    package_paths = set()
    for package in packages:
        manifest = Path(package['manifest_path']).resolve()
        if not manifest.is_relative_to(workspace) or manifest.name != 'Cargo.toml':
            raise unsupported(f'unexpected Cargo workspace manifest {manifest}')
        path = manifest.parent.relative_to(workspace).as_posix()
        if path in package_paths or WORKSPACE_PACKAGES.get(path) != package['name']:
            raise unsupported(f'unrecognized Cargo workspace package {path}: {package["name"]}')
        package_paths.add(path)
        package_ids[package['id']] = path
        for key, selector in package_targets(package, path):
            if key in targets:
                raise unsupported(f'duplicate Windows Cargo executable key {key}')
            targets[key] = selector
            groups.setdefault(selector, []).append(key)
    if package_paths != set(WORKSPACE_PACKAGES):
        raise unsupported(f'incomplete Cargo workspace members: {sorted(package_paths)}')

    # Preserve the initial pilot's grouping and balancing. The old runner's
    # expensive core unit suite owns the indivisible --lib selector.
    assigned = {key: owner(key) for key in targets}
    for key in groups[('--lib',)]:
        assigned[key] = owner(('core', 'deps/cumulus_core'))
    for _, group in sorted(groups.items()):
        candidates = {assigned[key] for key in group}
        if len(candidates) > 1:
            shard = min(candidates, key=lambda part: (list(assigned.values()).count(part), part))
            for key in group:
                assigned[key] = shard
    for shard in SHARDS:
        selected_packages = {key[0] for key, part in assigned.items() if part == shard}
        if selected_packages != package_paths:
            raise unsupported(f'Windows Cargo shard {shard} must select targets from every package')
    return package_ids, targets, assigned


def derive_plan(metadata, workspace):
    """Materialize the exact ownership and literal Cargo argv for each shard."""
    _, targets, assigned = plan_targets(metadata, workspace)
    plan = {}
    for shard in SHARDS:
        keys = tuple(sorted(key for key, part in assigned.items() if part == shard))
        selectors = sorted({targets[key] for key in keys})
        plan[shard] = ShardPlan(keys, tuple(arg for selector in selectors for arg in selector))
    return plan


def pinned_cargo(workspace):
    workspace = Path(workspace).resolve()
    toolchain = tomllib.loads((workspace / 'rust-toolchain.toml').read_text(encoding='utf-8'))
    channel = toolchain['toolchain']['channel']
    if not isinstance(channel, str) or not PINNED_VERSION.fullmatch(channel):
        raise ValueError(f'expected a pinned stable Cargo version, got {channel!r}')
    return 'cargo', f'+{channel}'


def cargo_metadata(workspace, cargo=None):
    cargo = cargo or pinned_cargo(workspace)
    metadata = subprocess.check_output([
        *cargo, 'metadata', '--offline', '--locked', '--no-deps', '--format-version', '1',
        '--features', FEATURE,
    ], cwd=workspace, encoding='utf-8')
    return json.loads(metadata)


def load_plan(workspace):
    workspace = Path(workspace).resolve()
    cargo = pinned_cargo(workspace)
    return cargo, derive_plan(cargo_metadata(workspace, cargo), workspace)


def cargo_argv(cargo, shard):
    if not shard.selectors:
        raise ValueError('refusing a Cargo invocation without target selectors')
    return (*cargo, 'test', '--no-fail-fast', '--workspace', '--features', FEATURE, '--locked',
            *shard.selectors)


def all_targets(plan):
    return frozenset(key for item in plan.values() for key in item.targets)


def read_execution(path, shard, current_run):
    entries = {}
    for line in Path(path).read_text(encoding='utf-8').splitlines():
        row = json.loads(line)
        if not isinstance(row, list) or len(row) != 4:
            raise ValueError(f'invalid Windows Cargo receipt in shard {shard}')
        target, recorded_shard, status, recorded_run = row
        if (not isinstance(target, list) or len(target) != 2
                or not all(isinstance(piece, str) and piece for piece in target)):
            raise ValueError(f'invalid Windows Cargo target in shard {shard}: {target!r}')
        key = tuple(target)
        if (key in entries or recorded_shard != shard or recorded_run != current_run
                or (status is not None and (type(status) is not int or status != 0))):
            raise ValueError(f'invalid or unsuccessful Windows Cargo receipt for {key} on {shard}')
        entries[key] = status
    return entries


def require_keys(expected, actual, shard):
    missing = sorted(expected - actual)
    extra = sorted(actual - expected)
    if missing or extra:
        raise ValueError(f'Windows Cargo inventory differs on {shard}: missing={missing}, extra={extra}')


def verify_selected(plan, receipts, current_run, reference_shard=None):
    """Verify each target ran once; optionally keep one full old-command inventory."""
    if len(receipts) != len(SHARDS) or reference_shard not in (None, *SHARDS):
        raise ValueError('expected exactly one receipt for each known Windows Cargo shard')
    entire = all_targets(plan)
    executed = set()
    counts = {}
    for shard, path in zip(SHARDS, receipts, strict=True):
        owned = set(plan[shard].targets)
        entries = read_execution(path, shard, current_run)
        require_keys(entire if shard == reference_shard else owned, entries.keys(), shard)
        actual = {key for key, status in entries.items() if status == 0}
        require_keys(owned, actual, shard)
        if actual & executed:
            raise ValueError(f'Windows Cargo target executed on more than one shard: {sorted(actual & executed)}')
        executed.update(actual)
        counts[shard] = len(actual)
    require_keys(entire, executed, 'aggregate')
    return counts


def compare_legacy(plan, receipt, shard, recorded_run):
    """Compare a historical full inventory even if its owner policy changed."""
    if shard not in SHARDS:
        raise ValueError(f'unknown legacy Windows Cargo shard {shard!r}')
    entries = read_execution(receipt, shard, recorded_run)
    expected = all_targets(plan)
    require_keys(expected, entries.keys(), shard)
    if not any(status == 0 for status in entries.values()):
        raise ValueError(f'legacy Windows Cargo shard {shard} did not execute any targets')
    return len(expected)


def require_checkout(workspace, current_run):
    commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=workspace, encoding='utf-8').strip()
    if commit != current_run[0]:
        raise ValueError('Windows Cargo verifier checkout does not match the receipt commit')


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument('--plan', action='store_true')
    mode.add_argument('--verify-selected', nargs=len(SHARDS), metavar='RECEIPT')
    mode.add_argument('--compare-legacy', type=Path, metavar='RECEIPT')
    parser.add_argument('--workspace', type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument('--reference-shard', choices=SHARDS)
    parser.add_argument('--legacy-shard', choices=SHARDS)
    args = parser.parse_args(argv)
    if args.reference_shard and not args.verify_selected:
        parser.error('--reference-shard requires --verify-selected')
    if bool(args.legacy_shard) != bool(args.compare_legacy):
        parser.error('--legacy-shard must be used with --compare-legacy')
    identity = provenance() if not args.plan else None
    if args.verify_selected:
        require_checkout(args.workspace, identity)
    cargo, plan = load_plan(args.workspace)
    if args.plan:
        print(json.dumps({shard: {'targets': item.targets, 'argv': cargo_argv(cargo, item)}
                          for shard, item in plan.items()}, indent=2))
    elif args.verify_selected:
        counts = verify_selected(plan, args.verify_selected, identity, args.reference_shard)
        print(f'Verified {sum(counts.values())} Windows Cargo targets: {counts}')
    else:
        count = compare_legacy(plan, args.compare_legacy, args.legacy_shard, identity)
        print(f'Exact legacy Windows Cargo inventory match: {count} targets')


if __name__ == '__main__':
    main()
