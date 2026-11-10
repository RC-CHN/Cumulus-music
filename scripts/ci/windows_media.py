"""Build the pinned, complete UCRT64 FFmpeg recipe with LADSPA support.

No codecs are removed. Only the shared runtime variant is distributed. Cache
identity covers every installed dependency plus the recipe and build scripts;
cached packages are authenticated and the installed filter is checked every run.
"""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import uuid

import windows_media_seed as seed
from windows_media_recipe import (
    BUILD, HERE, PACKAGE, ROOT, build_package, configuration, download, output,
    patch_recipe, prepare, sha256, write_json,
)


def native_pcm_report(stdout, destination, stderr):
    """Keep the full response even when exit=0 has no valid verification report."""
    destination.write_text(stdout, encoding='utf-8', newline='')
    try:
        report = json.loads(stdout)
        assert isinstance(report, dict) and report.get('ok') is True
        assert type(report.get('remote_requests')) is int and report['remote_requests'] == 0
    except (ValueError, AssertionError):
        print('Native PCM verifier produced no valid success report; exit=0 is insufficient.', flush=True)
        print(f'Captured stdout: {stdout[:4000]!r}', flush=True)
        if stderr.exists():
            print(stderr.read_text(encoding='utf-8', errors='replace')[-16000:], flush=True)
        raise
    return report


def verify_cache(cache, plan, lock=None):
    if lock is None:
        lock = json.loads((HERE / 'windows_media.lock.json').read_text(encoding='utf-8'))
    receipt = json.loads((cache / 'build.json').read_text(encoding='utf-8'))
    assert receipt['key'] == plan['key'], 'runtime cache identity mismatch'
    assert receipt['packages'] == plan['packages'], 'runtime cache package inventory mismatch'
    assert receipt['recipeCommit'] == lock['recipeCommit'] and receipt['source'] == lock['source'], (
        'runtime cache recipe or source identity mismatch')
    for name, expected in receipt['files'].items():
        assert Path(name).name == name, 'invalid runtime cache member'
        assert sha256(cache / name) == expected, f'runtime cache checksum mismatch: {name}'
    packages = list(cache.glob('*.pkg.tar.zst'))
    assert len(packages) == 1 and packages[0].name in receipt['files'], 'missing/ambiguous runtime package'
    for name, expected in lock['recipeFiles'].items():
        source_name = 'PKGBUILD.upstream' if name == 'PKGBUILD' else name
        assert source_name in receipt['files'], f'cache lacks corresponding source: {source_name}'
        assert sha256(cache / source_name) == expected, f'locked recipe checksum mismatch: {source_name}'
    source = lock['source']
    assert source['name'] in receipt['files'], 'cache lacks the FFmpeg source archive'
    assert sha256(cache / source['name']) == source['sha256'], 'locked source checksum mismatch'
    expected_recipe = patch_recipe((cache / 'PKGBUILD.upstream').read_text(encoding='utf-8'))
    assert (cache / 'PKGBUILD').read_text(encoding='utf-8') == expected_recipe, 'modified recipe differs from reviewed patch'
    patch = 'ffmpeg-ladspa-windows.patch'
    assert patch in receipt['files'] and sha256(cache / patch) == sha256(HERE / patch), 'Windows loader patch differs from reviewed source'
    return packages[0]


def proof_binaries(profile):
    assert profile in ('debug', 'release'), 'invalid Windows media Cargo profile'
    target = Path(os.environ.get('CARGO_TARGET_DIR', ROOT / '.build/target')) / profile
    return target / 'cumulus_audio_filter.dll', target / 'examples/player_probe.exe'


def prepare_with_seed(msys):
    prepare(msys)
    plan = json.loads((BUILD / 'plan.json').read_text(encoding='utf-8'))
    if key := seed.legacy_key(plan):
        with open(os.environ['GITHUB_OUTPUT'], 'a', encoding='utf-8') as stream:
            stream.write(f'legacy-cache-key={key}\n')


def select_package(msys, cache, plan, lock, primary_match='', legacy_match=''):
    if primary_match == plan['key'] and not legacy_match:
        return verify_cache(cache, plan, lock), None
    if not primary_match and legacy_match:
        try:
            return seed.verify_seed(cache, plan, legacy_match, lock, verify_cache), legacy_match
        except (AssertionError, KeyError, OSError, TypeError, ValueError):
            print('Rejected unauthenticated legacy media cache; building the current recipe', flush=True)
    if not primary_match and not legacy_match and not os.environ.get('GITHUB_ACTIONS'):
        # Local CLI reuse is current-key only; migration requires an exact CI
        # cache match supplied by the pinned restore action.
        if (cache / 'build.json').is_file():
            return verify_cache(cache, plan, lock), None
    if os.path.lexists(cache):
        # Move only this runner's restore entry. Do not traverse or execute
        # an unverified archive, including a Windows reparse point.
        cache.rename(cache.with_name(f'rejected-cache-{uuid.uuid4().hex}'))
    build_package(msys, cache, plan, lock)
    return verify_cache(cache, plan, lock), None


def install(msys, profile='debug', primary_match='', legacy_match=''):
    plan = json.loads((BUILD / 'plan.json').read_text(encoding='utf-8'))
    lock = json.loads((HERE / 'windows_media.lock.json').read_text(encoding='utf-8'))
    cache = BUILD / 'cache'
    package, seeded_from = select_package(msys, cache, plan, lock, primary_match, legacy_match)
    package_path = output([msys / 'usr/bin/cygpath.exe', '-u', package]).strip()
    subprocess.run([str(msys / 'usr/bin/pacman.exe'), '-U', '--noconfirm', package_path], check=True)
    version = output([msys / 'usr/bin/pacman.exe', '-Q', PACKAGE]).strip().split(' ', 1)[1]
    assert version == lock['customPackage'], 'custom FFmpeg package was not installed'
    binary = msys / 'ucrt64/bin'
    current = configuration(output([binary / 'ffmpeg.exe', '-version']))
    missing = set(plan['configuration']) - set(current)
    assert not missing, f'FFmpeg configuration regressed: {sorted(missing)}'
    assert '--enable-ladspa' in current
    if seeded_from:
        assert current == seed.patched_configuration(plan['configuration']), 'seeded media configuration is not exact'
    filters = output([binary / 'ffmpeg.exe', '-hide_banner', '-filters'])
    assert re.search(r'\bladspa\s+', filters), 'installed FFmpeg cannot load LADSPA'
    # The real current plugin must load, not merely appear in a capability list.
    plugin, probe = proof_binaries(profile)
    assert plugin.is_file() and probe.is_file(), 'build the current Cumulus DSP and player probe before runtime verification'
    shutil.copy2(plugin, BUILD / 'cumulus_audio_filter.dll')
    subprocess.run([str(binary / 'ffmpeg.exe'), '-nostdin', '-hide_banner', '-v', 'error',
                    '-f', 'lavfi', '-i', 'sine=frequency=1000:duration=0.1',
                    '-af', 'ladspa=file=./cumulus_audio_filter.dll:plugin=cumulus_geq:controls=1',
                    '-f', 'null', '-'], cwd=BUILD, check=True, timeout=30)
    # Check the production host's absolute, escaped Unicode path in-process:
    # loading the relative ASCII plugin above cannot establish this capability.
    env = {**os.environ, 'PATH': str(binary) + os.pathsep + os.environ.get('PATH', '')}
    with (BUILD / 'player-probe.log').open('x', encoding='utf-8') as log:
        try:
            result = subprocess.run([str(probe), str(plugin.resolve()), str(BUILD / 'equalizer-proof')],
                                    stdout=subprocess.PIPE, stderr=log, env=env, check=True,
                                    text=True, encoding='utf-8', timeout=60)
        except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
            captured = error.stdout or ''
            if isinstance(captured, bytes):
                captured = captured.decode('utf-8', errors='replace')
            (BUILD / 'player-probe.stdout.log').write_text(captured, encoding='utf-8', newline='')
            log.flush()
            print((BUILD / 'player-probe.log').read_text(encoding='utf-8', errors='replace')[-16000:], flush=True)
            raise
    pcm = native_pcm_report(result.stdout, BUILD / 'player-probe.stdout.log', BUILD / 'player-probe.log')
    write_json(BUILD / 'player-probe.json', pcm)
    libraries = {p.name: sha256(p) for p in sorted(binary.glob('*.dll'))
                 if p.name.startswith(('avcodec-', 'avdevice-', 'avfilter-', 'avformat-', 'avutil-', 'swresample-', 'swscale-'))}
    assert len(libraries) == 7, f'FFmpeg library set changed: {sorted(libraries)}'
    receipt = {'key': plan['key'], 'package': version, 'configuration': current,
               'libraries': libraries, 'filterLoaded': True, 'nativePcmVerified': True,
               'pluginSha256': sha256(plugin)}
    if seeded_from:
        receipt['seededFrom'] = seeded_from
    write_json(BUILD / 'installed.json', receipt)
    print(json.dumps(receipt), flush=True)


def promote_seed(profile, legacy_match):
    # The workflow calls this only after the current native PCM and the
    # independent implicit libmpv lookup rejection have both succeeded.
    installed = json.loads((BUILD / 'installed.json').read_text(encoding='utf-8'))
    if not (legacy := installed.get('seededFrom')):
        return
    plan = json.loads((BUILD / 'plan.json').read_text(encoding='utf-8'))
    lock = json.loads((HERE / 'windows_media.lock.json').read_text(encoding='utf-8'))
    assert legacy == legacy_match and installed['key'] == plan['key'], 'stale media seed proof'
    assert installed['filterLoaded'] is True and installed['nativePcmVerified'] is True
    assert installed['configuration'] == seed.patched_configuration(plan['configuration'])
    plugin, _ = proof_binaries(profile)
    assert installed['pluginSha256'] == sha256(plugin), 'media seed did not verify the current DSP'
    cache = BUILD / 'cache'
    seed.verify_seed(cache, plan, legacy, lock, verify_cache)
    source = cache / 'build.json'
    receipt = json.loads(source.read_text(encoding='utf-8'))
    with tempfile.NamedTemporaryFile(dir=BUILD, prefix='promote-cache-', delete=False) as file:
        pending = Path(file.name)
    try:
        write_json(pending, {**receipt, 'key': plan['key'], 'seededFrom': legacy})
        pending.replace(source)
    finally:
        pending.unlink(missing_ok=True)
    verify_cache(cache, plan, lock)
    print(f'Promoted verified exact media seed {legacy} to {plan["key"]}', flush=True)


def check_built_cache(_msys):
    """Authenticate build artifacts; this is not an installed-runtime proof."""
    plan = json.loads((BUILD / 'plan.json').read_text(encoding='utf-8'))
    package = verify_cache(BUILD / 'cache', plan)
    print(f'Build artifact cache verified: {package.name}', flush=True)


def copy_provenance(destination, msys):
    """Require the verified runtime; retain its corresponding source in bundles."""
    receipt = json.loads((BUILD / 'installed.json').read_text(encoding='utf-8'))
    assert receipt['filterLoaded'] and receipt['nativePcmVerified'] and '--enable-ladspa' in receipt['configuration']
    for name, expected in receipt['libraries'].items():
        assert sha256(msys / 'ucrt64/bin' / name) == expected, f'FFmpeg changed after verification: {name}'
    target = destination / 'third-party/cumulus-ffmpeg-source'
    target.mkdir(parents=True)
    cache = BUILD / 'cache'
    plan = json.loads((BUILD / 'plan.json').read_text(encoding='utf-8'))
    verify_cache(cache, plan)
    for path in cache.iterdir():
        if path.is_file() and not path.name.endswith('.pkg.tar.zst'):
            shutil.copy2(path, target)
    shutil.copy2(BUILD / 'installed.json', target / 'installed.json')
    shutil.copy2(HERE / 'windows_media.lock.json', target / 'cumulus-lock.json')
    return receipt


if __name__ == '__main__':
    assert sys.platform == 'win32', 'the runtime is built and checked on Windows'
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['prepare', 'install', 'promote-seed', 'verify-cache'])
    parser.add_argument('--msys', type=Path, required=True)
    parser.add_argument('--profile', choices=['debug', 'release'], default='debug')
    args = parser.parse_args()
    legacy_match = os.environ.get('CUMULUS_MEDIA_LEGACY_MATCHED_KEY', '')
    if args.action == 'install':
        install(args.msys.resolve(), args.profile, os.environ.get('CUMULUS_MEDIA_PRIMARY_MATCHED_KEY', ''), legacy_match)
    elif args.action == 'promote-seed':
        promote_seed(args.profile, legacy_match)
    else:
        {'prepare': prepare_with_seed, 'verify-cache': check_built_cache}[args.action](args.msys.resolve())
