"""Exact downloaded song/episode queue handoff in real Linux WebView."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

from local_folder_reveal_smoke import fake_file_manager
from queue_collection_smoke import isolated_profile
from window_manager import launch

STAGES = ('ux_01_exact_song_menu', 'ux_02_exact_voice_menu',
          'ux_03_voice_download_source', 'ux_04_original_rejected')


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def verify(binary, root, dist, screenshots):
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    script = Path(__file__).resolve().parents[2] / 'desktop/examples/queue_download_file_reveal_ui_probe.js'
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1', CUMULUS_QUEUE_UI_PROOF='1',
               CUMULUS_DOWNLOAD_PAGE_UI_PROOF='1', CUMULUS_QUEUE_DOWNLOAD_REVEAL_PROOF='1',
               CUMULUS_UX_SCRIPT=str(script), CUMULUS_PROOF_FRONTEND_DIST=str(dist),
               LC_ALL='en_US.UTF-8')
    isolated_profile(env, root)
    fixture_bin = root / 'fake-bin'
    fixture_bin.mkdir()
    fake_open = fixture_bin / 'xdg-open'
    fake_open.write_text(
        f'#!{sys.executable}\nfrom pathlib import Path\nPath({str(root / "unexpected-fallback")!r}).touch()\n',
        encoding='utf-8',
    )
    fake_open.chmod(0o700)
    env['PATH'] = str(fixture_bin) + os.pathsep + env.get('PATH', '')
    output = root / 'first'
    with fake_file_manager(root, env, exact_successes=2):
        with (root / 'host.log').open('x', encoding='utf-8') as log:
            code = launch(binary, env, log, output, screenshots)
        failed = output / 'failed.json'
        assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
        assert code == 0
    complete = read(output / 'complete.json')
    assert complete['ok'] and complete['samePlay']
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    assert complete['negative'] == ['invalid_input', 'snapshot_changed', 'stale']
    rows = [read(output / f'{stage}.json') for stage in STAGES]
    baseline = rows[0]['nativePlayer']
    for row in rows:
        native = row['nativePlayer']
        assert native['state'] == 'paused' and not native['error']
        assert all(native[key] == baseline[key] for key in (
            'playId', 'entryId', 'queueRevision', 'position', 'downloadTaskId'))
        assert row['nativeQueue'] == rows[0]['nativeQueue']
    calls = [json.loads(line) for line in (root / 'show-items.jsonl').read_text(encoding='utf-8').splitlines()]
    media = root / 'media'
    chosen = [media / 'queued original 合成.wav', media / 'queued original 声音.wav']
    assert calls == [[[path.resolve(strict=True).as_uri()], ''] for path in chosen]
    with (media / 'alternate same song.wav').open('rb') as alternate:
        assert alternate.read(4) == b'RIFF', 'alternate exists and was never selected or modified'
    assert not (root / 'unexpected-fallback').exists()
    assert {p.stem for p in output.glob('ux_*.png')} == (set(STAGES) if screenshots else set())
    locale = read(output / 'locale.json')
    assert locale['numeric'] == 'C' and locale['messages'].casefold().replace('-', '') == 'en_us.utf8'
    report = dict(ok=True, actualWebView=True, exactFileManager1Calls=2, screenshots=len(STAGES) if screenshots else 0,
                  samePlay=True, noFallback=True, voiceCategoryVerified=True, realMusicRequests=0,
                  realAccountWrites=0, negative=complete['negative'])
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path, required=True)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--private-child', action='store_true', help=argparse.SUPPRESS)
    args = parser.parse_args()
    assert sys.platform == 'linux'
    binary, root, dist = args.binary.resolve(strict=True), args.root.resolve(), args.frontend_dist.resolve(strict=True)
    if args.private_child:
        verify(binary, root, dist, args.screenshots)
    else:
        env = {key: value for key, value in os.environ.items()
               if key not in ('DBUS_SESSION_BUS_ADDRESS', 'AT_SPI_BUS_ADDRESS', 'AT_SPI_DISPLAY')}
        subprocess.run(['dbus-run-session', '--', sys.executable, __file__, '--private-child',
                        '--binary', str(binary), '--root', str(root), '--frontend-dist', str(dist),
                        *(['--screenshots'] if args.screenshots else [])],
                       env=env, check=True, timeout=180)


if __name__ == '__main__':
    main()
