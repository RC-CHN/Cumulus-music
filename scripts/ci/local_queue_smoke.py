"""Verify the local-library add-all action with real WebView, files and null audio."""
import argparse
import json
import os
from pathlib import Path
from motion_smoke import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--small', action='store_true')
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    run = root / 'run'
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    # This is the verified fixture generator, independent of the packaged
    # libmpv/ffprobe runtime selected by the Windows packaging step.
    if (ffmpeg := os.environ.get('CUMULUS_LOCAL_FIXTURE_FFMPEG')):
        env['CUMULUS_LOCAL_FIXTURE_FFMPEG'] = ffmpeg
    env.update(CUMULUS_PROOF_DIR=str(run), CUMULUS_LOCAL_QUEUE_PROOF='1')
    if args.small:
        env['CUMULUS_PROOF_SMALL'] = '1'
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR',
                'APPDATA', 'LOCALAPPDATA'):
        path = root / 'os-profile' / key
        path.mkdir(parents=True, mode=0o700)
        env[key] = str(path)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, run, args.screenshots)
    failed = run / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'local queue UI exit {code}'
    started = json.loads((run / 'local_queue_started.json').read_text(encoding='utf-8'))
    assert started == {'stage': 'local_queue_started', 'queue': 1, 'cue': True}
    complete = json.loads((run / 'complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['retainedPlayback'] and complete['realMusicRequests'] == 0
    assert complete['folderTree'] is True and complete['groupMenu'] is True and complete['trackMenu'] is True
    assert complete['localSort'] is True
    assert complete['groupPinyinOrder'] is True
    if args.small:
        assert complete['width'] == 760
        assert complete['dropQueue'] is None
    else:
        assert complete['dropQueue'] == ['雨声', '夜行 · 下篇', '夜行 · 上篇']
        assert (run / 'ux_03_local_queue_drop.json').is_file()
    assert complete['audioOutput'] == 'null' and complete['queue'] == ['夜行 · 下篇', '雨声', '夜行 · 上篇']
    assert len(list(run.glob('ux_*.png'))) == ((2 if args.small else 3) if args.screenshots else 0)
    print(json.dumps({'ok': True, 'actualWebView': True, 'queueCount': len(complete['queue'])}))


if __name__ == '__main__':
    main()
