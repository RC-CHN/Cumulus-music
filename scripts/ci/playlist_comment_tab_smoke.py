"""Open in-place playlist comments in a real WebView with a synthetic API."""
import argparse
import json
import os
from pathlib import Path

from motion_smoke import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path, help='Local synthetic frontend comparison')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1',
               CUMULUS_PLAYLIST_COMMENT_TAB_PROOF='1', CUMULUS_PLAYLIST_SUBSCRIBERS_UI_PROOF='1')
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve())
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR',
                'APPDATA', 'LOCALAPPDATA'):
        folder = root / 'os-profile' / key
        folder.mkdir(parents=True, mode=0o700)
        env[key] = str(folder)
    env['CUMULUS_UX_SCRIPT'] = str(Path(__file__).resolve().parents[2] /
                                  'desktop/examples/playlist_comment_tab_ui_probe.js')
    output = root / 'first'
    with (root / 'first.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, output, False)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'probe exited {code}'

    def read(stage):
        return json.loads((output / f'{stage}.json').read_text(encoding='utf-8'))

    assert read('ux_01_tracks')['playlistCommentThreads'] == []
    assert read('ux_01_tracks')['compactSearch'] is True
    assert read('ux_02_comments')['playlistCommentThreads'] == ['A_PL_0_2']
    assert read('ux_03_tracks_again')['playlistCommentThreads'] == ['A_PL_0_2']
    assert read('ux_01_tracks')['playlistCommentDetails'] == read('ux_03_tracks_again')['playlistCommentDetails'] == [2]
    assert read('ux_03_tracks_again')['subscriberOffsets'] == [0]
    result = read('complete')
    assert result['ok'] and result['playlistCommentThreads'] == ['A_PL_0_2', 'A_PL_0_3']
    assert result['playlistCommentDetails'] == [2, 3]
    print(json.dumps({'ok': True, 'actualWebView': True, 'commentThreads': result['playlistCommentThreads']}))


if __name__ == '__main__':
    main()
