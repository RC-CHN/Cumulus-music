"""Open the playlist collector tab in a real WebView with a synthetic API."""
import argparse
import json
import os
from pathlib import Path

from motion_smoke import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1',
               CUMULUS_PLAYLIST_SUBSCRIBERS_UI_PROOF='1', CUMULUS_PLAYLIST_CREATOR_PROOF='1')
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR',
                'APPDATA', 'LOCALAPPDATA'):
        folder = root / 'os-profile' / key
        folder.mkdir(parents=True, mode=0o700)
        env[key] = str(folder)
    env['CUMULUS_UX_SCRIPT'] = str(Path(__file__).resolve().parents[2] /
                                  'desktop/examples/playlist_subscribers_ui_probe.js')
    output = root / 'first'
    with (root / 'first.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, output, False)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'probe exited {code}'

    def read(stage):
        return json.loads((output / f'{stage}.json').read_text(encoding='utf-8'))

    assert read('ux_01_tracks')['subscriberOffsets'] == []
    assert read('ux_02_collectors')['subscriberOffsets'] == [0]
    assert read('ux_03_retried')['subscriberOffsets'] == [0, 24, 24]
    assert read('ux_04_profile')['playlistCreatorCalls'] == [
        {'all': True, 'userId': '3'},
        {'uid': '3', 'limit': 1000, 'offset': 0},
    ]
    assert read('ux_05_profile_back')['subscriberOffsets'] == [0, 24, 24]
    result = read('complete')
    assert result['ok'] and result['subscriberOffsets'] == [0, 24, 24, 0]
    print(json.dumps({'ok': True, 'actualWebView': True, 'subscriberOffsets': result['subscriberOffsets']}))


if __name__ == '__main__':
    main()
