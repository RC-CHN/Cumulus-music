"""Build and hand off each Windows WebView shard's same-attempt test probes."""
import argparse
from collections import Counter
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys

from windows_cold_flac import test_executable
from windows_webview_inputs import UX, ci_identity, file_receipt, frontend_receipts, plain_stat


ARTIFACT = Path('.build/windows-webview-probes')
EXAMPLES = Path('.build/target/debug/examples')
FRONTEND = Path('frontend/dist')
MANIFEST = 'manifest.json'
COLD_FLAC_TEST = 'cumulus_core_cold_flac_tests.exe'
PROBES = {
    'playback': (
        'equalizer_ui_probe', 'login_interception_ui_probe', 'loudness_ui_probe',
        'main_flow_ui_probe', 'native_audio_probe', 'playback_power_ui_probe',
        'queue_room_ui_probe', 'scroll_ui_probe', 'search_playback_read',
    ),
    # These shards only use the shared UX executable. Their empty
    # inventories still get an exact-attempt manifest like the other shards.
    'library': (),
    'search': (),
    'playlists': (),
    'library-local': (
        'desktop_lyrics_ui_probe', 'local_ui_probe', 'mini_ui_probe', 'output_ui_probe',
        'startup_ui_probe',
    ),
    'package-media': (
        'audiobook_skip_ui_probe', 'minibar_skip_ui_probe', 'native_local_equalizer_probe',
    ),
    'package-runtime': ('local_ui_probe', 'native_audio_probe'),
    'search-account': (
        'account_login_ui_probe', 'auth_ui_probe', 'device_login_ui_probe', 'email_login_ui_probe',
        'legacy_renewal_ui_probe', 'login_notice_ui_probe', 'phone_entry_ui_probe',
        'question_login_ui_probe', 'renewal_ui_probe', 'reuse_login_ui_probe',
        'saved_accounts_ui_probe', 'shortcut_probe', 'sms_login_ui_probe',
    ),
    'catalog': ('catalog_ui_probe', 'comment_edit_ui_probe', 'community_ui_probe', 'discovery_home_ui_probe'),
    'catalog-discovery': (
        'catalog_ui_probe', 'comment_report_ui_probe', 'manual_update_ui_probe',
        'network_diagnostics_ui_probe', 'podcast_recent_ui_probe', 'profile_report_ui_probe',
        'user_blacklist_ui_probe',
    ),
}


def binary_names(shard):
    examples = tuple(f'{example}.exe' for example in PROBES[shard])
    return (*examples, COLD_FLAC_TEST) if shard == 'package-runtime' else examples


def receipt(directory, identity, shard, frontend):
    return dict(schema=1, **identity, shard=shard, frontend=frontend,
                binaries={name: file_receipt(directory / name) for name in binary_names(shard)})


def stage(repo, identity, frontend):
    root, examples = repo / ARTIFACT, repo / EXAMPLES
    manifests = {shard: receipt(examples, identity, shard, frontend) for shard in PROBES}
    remaining = Counter(name for shard in PROBES for name in binary_names(shard))
    root.mkdir(parents=True)
    for shard, manifest in manifests.items():
        directory = root / shard
        directory.mkdir()
        for name in binary_names(shard):
            remaining[name] -= 1
            source = examples / name
            if remaining[name]:
                # A second shard needs these same bytes; link with Cargo only once.
                shutil.copyfile(source, directory / name)
            else:
                source.rename(directory / name)
        (directory / MANIFEST).write_text(
            json.dumps(manifest, sort_keys=True) + '\n', encoding='utf-8',
        )


def restore(repo, identity, shard):
    directory = repo / ARTIFACT / shard
    assert stat.S_ISDIR(plain_stat(directory).st_mode), 'expected a probe artifact directory'
    assert {p.name for p in directory.iterdir()} == {MANIFEST, *binary_names(shard)}, (
        'unexpected WebView probe contents'
    )
    manifest = directory / MANIFEST
    info = plain_stat(manifest)
    assert stat.S_ISREG(info.st_mode) and info.st_size <= 2 * 1024 * 1024, 'invalid probe manifest'
    stored = json.loads(manifest.read_text(encoding='utf-8'))
    expected = receipt(directory, identity, shard, frontend_receipts(repo / FRONTEND))
    assert stored == expected, 'WebView probe provenance or checksum mismatch'
    examples = repo / EXAMPLES
    examples.mkdir(parents=True, exist_ok=True)
    assert all(not (examples / name).exists() and not (examples / name).is_symlink()
               for name in binary_names(shard)), 'unexpected executable already present'
    for name in binary_names(shard):
        (directory / name).rename(examples / name)


def build(repo, identity):
    frontend = frontend_receipts(repo / FRONTEND)
    examples = sorted({UX.stem, *(example for names in PROBES.values() for example in names)})
    command = ['cargo', 'build', '-p', 'cumulus-desktop', '--features', 'custom-protocol', '--locked']
    for example in examples:
        command.extend(['--example', example])
    subprocess.run(command, cwd=repo, check=True)
    # The cold FLAC verifier uses the standalone core libtest feature set.
    # Keep its compilation independent of desktop features, and bind the
    # resulting executable to the same attempt as the other shard inputs.
    print('Building the Windows cold FLAC core test harness', flush=True)
    shutil.copyfile(test_executable(sys.stderr, repo=repo), repo / EXAMPLES / COLD_FLAC_TEST)
    assert frontend == frontend_receipts(repo / FRONTEND), 'frontend assets changed during build'
    stage(repo, identity, frontend)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    modes = parser.add_subparsers(dest='operation', required=True)
    modes.add_parser('build')
    receive = modes.add_parser('restore')
    receive.add_argument('shard', choices=PROBES)
    args = parser.parse_args()
    repo, identity = Path.cwd(), ci_identity(os.environ)
    if args.operation == 'build':
        build(repo, identity)
    else:
        restore(repo, identity, args.shard)
    print(json.dumps(dict(operation=args.operation, **identity)))


if __name__ == '__main__':
    main()
