"""Playlist batch layouts, selection and recovery in the production WebView."""
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
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1',
               CUMULUS_LIBRARY_UI_PROOF='1', CUMULUS_PLAYLIST_MENU_UI_PROOF='1',
               CUMULUS_PLAYLIST_QUEUE_UI_PROOF='1', CUMULUS_PLAYLIST_BATCH_UI_PROOF='1')
    for key, name in [('XDG_DATA_HOME','data'), ('XDG_CONFIG_HOME','config'),
                      ('XDG_CACHE_HOME','cache'), ('XDG_RUNTIME_DIR','runtime'),
                      ('APPDATA','roaming'), ('LOCALAPPDATA','local')]:
        directory = root / 'os-profile' / name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/playlist_batch_ui_probe.js'
    script = root / 'batch.js'
    script.write_text(source.read_text(encoding='utf-8'), encoding='utf-8')
    env['CUMULUS_UX_SCRIPT'] = str(script)
    with (root / 'first.log').open('x', encoding='utf-8') as log:
        status = launch(args.binary.resolve(), env, log, root / 'first', args.screenshots)
    failed = root / 'first/failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert status == 0, f'probe exit {status}'
    result = json.loads((root / 'first/complete.json').read_text(encoding='utf-8'))
    assert result['ok'] and result['playlistBatchUi']
    assert result['realMusicRequests'] == result['realAccountWrites'] == 0
    records = sorted((root / 'first').glob('ux_*.json'))
    assert len(records) == result['stageCount'] == 52
    if args.screenshots:
        assert len(list((root / 'first').glob('ux_*.png'))) == 52
    captures = {p.stem.split('_',2)[2]:json.loads(p.read_text(encoding='utf-8')) for p in records}
    ids = lambda name: [item['track']['id'] for item in captures[name]['nativeQueue']['tracks']]
    assert ids('selected_playback') == ['1','3','240']
    assert ids('selected_queue') == ['1','2','4','3','240']
    initial = captures['selected_playback']['nativePlayer']
    for name in ['selected_queue','queue_failed','queue_retry_wait','queue_recovered']:
        state = captures[name]['nativePlayer']
        assert state['state'] == 'paused' and state['playId'] == initial['playId']
        assert state['position'] == initial['position']
    assert result['syntheticLibrary']['trackWrites'] == [
        {'pid':7,'op':'del','ids':['2','4']},
        {'pid':7,'op':'del','ids':['1']},
        {'pid':2,'op':'add','ids':['240']},
    ]
    result.update(actualWebView=True, physicalDisplayVerified=False, windowsVisualVerified=False)
    (root / 'audit.json').write_text(json.dumps(result,indent=2)+'\n', encoding='utf-8')
    print(json.dumps({k:result[k] for k in ['ok','stageCount','actualWebView','realAccountWrites']}))


if __name__ == '__main__':
    main()
