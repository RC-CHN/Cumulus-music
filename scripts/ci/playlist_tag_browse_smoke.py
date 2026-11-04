"""Verify desktop playlist tag navigation with an isolated synthetic service."""
import argparse
import json
import os
from pathlib import Path

from motion_smoke import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1',
               CUMULUS_PLAYLIST_TAG_BROWSE_PROOF='1')
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR',
                'APPDATA', 'LOCALAPPDATA'):
        folder = root / 'os-profile' / key
        folder.mkdir(parents=True, mode=0o700)
        env[key] = str(folder)
    env['CUMULUS_UX_SCRIPT'] = str(Path(__file__).resolve().parents[2] /
                                  'desktop/examples/playlist_tag_browse_ui_probe.js')
    output = root / 'first'
    with (root / 'first.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, output, args.screenshots)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'probe exited {code}'

    def read(stage):
        return json.loads((output / f'{stage}.json').read_text(encoding='utf-8'))

    first = {'cat': '轻音乐', 'limit': 24, 'newStyle': True}
    ids = [3, 4, *range(124, 338)]
    continuations = [
        {'ids': '[' + ','.join(map(str, ids[offset:offset + 24])) + ']', 'newStyle': True}
        for offset in range(0, len(ids), 24)
    ]
    ordinary = [first, continuations[0], *continuations]
    assert read('ux_01_source')['playlistTagCalls'] == []
    assert read('ux_02_first')['playlistTagCalls'] == [first]
    assert read('ux_03_next')['playlistTagCalls'] == [first, continuations[0], continuations[0]]
    assert read('ux_15_ordinary_long')['playlistTagCalls'] == ordinary
    official = {'cat': '官方', 'limit': 24, 'newStyle': True}
    selected = {'cat': '摇滚', 'limit': 24, 'newStyle': True}
    promoted = {'cat': '民谣', 'limit': 24, 'newStyle': True}
    assert read('ux_04_square')['playlistTagCalls'] == [*ordinary, promoted, official]
    assert read('ux_13_top_tags')['playlistCatalogueCalls'] == []
    assert read('ux_05_catalogue')['playlistCatalogueCalls'] == [{}, {}]
    complete = read('complete')
    assert complete['ok'] and complete['catalogueVerified'] and complete['recommendationVerified']
    assert complete['essentialVerified'] and complete['topTagsVerified'] and complete['waterfallVerified']
    assert complete['playlistTagCalls'] == [*ordinary, promoted, official, selected]
    assert complete['playlistCatalogueCalls'] == [{}, {}]
    assert read('ux_02_first')['playlistTopTagCalls'] == [{}]
    assert read('ux_13_top_tags')['playlistTopTagCalls'] == [{}, {}]
    assert complete['playlistTopTagCalls'] == [{}, {}]
    first_rcmd = {'cursor': '', 'newStyle': True}
    next_rcmd = {'cursor': '{ "next" : "第一页/第二页" }', 'newStyle': True}
    rcmd = [first_rcmd, next_rcmd, next_rcmd, {'cursor': ' continuation 2 ', 'newStyle': True},
            {'cursor': '{"page":4}', 'newStyle': True}]
    assert read('ux_06_recommended')['playlistSquare'] == {'calls': [first_rcmd], 'settings': 3}
    assert read('ux_07_recommended_back')['playlistSquare'] == {
        'calls': rcmd, 'settings': 7,
    }
    assert complete['playlistSquare'] == {'calls': rcmd, 'settings': 9}
    first_essential = {'cat': '轻音乐', 'limit': 36, 'lasttime': '-1'}
    next_essential = {'cat': '轻音乐', 'limit': 36, 'lasttime': 1_700_000_000_123}
    all_essential = {'cat': '全部', 'limit': 36, 'lasttime': '-1'}
    assert read('ux_09_essential_initial')['playlistEssential'] == {'pages': [first_essential], 'tags': 1}
    assert read('ux_11_essential_return')['playlistEssential'] == {
        'pages': [first_essential, next_essential, next_essential], 'tags': 2,
    }
    assert complete['playlistEssential'] == {
        'pages': [first_essential, next_essential, next_essential, all_essential], 'tags': 2,
    }
    assert len(list(output.glob('ux_*.png'))) == (16 if args.screenshots else 0)
    print(json.dumps({'ok': True, 'actualWebView': True, 'categoryReads': 4,
                      'continuationReads': 10, 'catalogueReads': 2, 'recommendationReads': 5,
                      'privacyReads': 9, 'essentialReads': 4, 'essentialTagReads': 2, 'topTagReads': 2,
                      'realAccountWrites': 0}))


if __name__ == '__main__':
    main()
