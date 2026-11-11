"""Run actual Lyrics group collection against a sealed native account/playlist fixture."""

import argparse
from contextlib import closing
import json
import os
from pathlib import Path
import sqlite3

from motion_smoke import launch


STAGES = (
    'lyric_collect_menu', 'lyric_collect_picker', 'lyric_collect_cancelled',
    'lyric_collect_second_picker', 'lyric_collect_uncertain', 'lyric_collect_confirmed',
)
QUERY = dict(kind='lyrics', args=dict(keyword='合成连续单曲', scene='normal', limit=10, offset=0),
             cached=False)
PREPARE = dict(anchor=dict(id='104', ordinal=3),
               rows=[dict(id='103', ordinal=2), dict(id='101', ordinal=0)])
PREVIEW = dict(kind='preview_lyrics_anchor', trackId='104')
ADD = dict(kind='playlist_add', args=dict(op='add', pid='81', trackIds='["103","101"]'))


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def audit(root, screenshots):
    output = root / 'first'
    names = [f'ux_{i:02}_{stage}' for i, stage in enumerate(STAGES, 1)]
    assert [p.stem for p in sorted(output.glob('ux_*.json'))] == names
    assert [p.stem for p in sorted(output.glob('ux_*.png'))] == (names if screenshots else [])
    menu, picker, cancelled, second, unknown, confirmed = rows = [
        read(output / f'{name}.json') for name in names
    ]
    complete = read(output / 'complete.json')
    assert complete['ok'] and complete['stageCount'] == len(rows) and complete['statusReads'] == 1
    assert complete['realAccountWrites'] == complete['realMusicRequests'] == 0
    assert [row['label'] for row in rows] == list(STAGES)
    if screenshots:
        for index in (1, 3, 4):
            painted = read(output / f'paint-{names[index]}.json')
            assert len(painted['samples']) == 1 and painted['samples'][0]['ok'], painted
    assert menu['clicked'] == '104'
    assert menu['menuKeys'][:3] == ['play', 'next', 'collect'] and len(menu['menuKeys']) in (3, 4)
    if len(menu['menuKeys']) == 4:
        assert menu['menuKeys'][-1] == 'download'
    for row in (picker, second):
        assert (row['title'], row['summary'], row['destinations']) == (
            '收藏到歌单', '所选歌词结果共 2 首，将收藏 2 首。', ['81'],
        )
    assert cancelled['focusRestored'] and cancelled['firstCancelled']
    assert (unknown['phase'], unknown['statusAction']) == ('uncertain', '核对结果')
    assert confirmed['focusRestored'] and confirmed['feedback'] == '已收藏到歌单。'
    assert all(row['selectedOrdinals'] == [0, 2, 4] and row['queueUnchanged'] for row in rows)
    assert complete['queueUnchanged']
    for row, plans, previews, destinations, status in zip(
        rows, (0, 1, 1, 2, 2, 2), (1, 1, 1, 2, 2, 2), (0, 0, 0, 0, 1, 1),
        (0, 0, 0, 0, 0, 1), strict=True,
    ):
        assert row['preparations'] == [PREPARE] * plans
        assert row['downloadIpc'] == [PREVIEW] * previews
        assert row['choices'] == ['81'] * destinations
        assert row['statusReads'] == status

    trace = complete['searchGroupDownload']
    assert all(call['kind'] in ('lyrics', 'playlist_list', 'playlist_detail', 'playlist_add')
               for call in trace), trace
    assert [call for call in trace if call['kind'] == 'lyrics'] == [QUERY]
    assert [call for call in trace if call['kind'] == 'playlist_add'] == [ADD]
    assert [call for call in trace if call['kind'] == 'playlist_list'] == [
        dict(kind='playlist_list', args=dict(uid='2', offset=0, limit=10, sortType='default'),
             cached=False),
    ] * 2
    for row in (menu, picker, cancelled, second):
        observed = row['searchGroupDownload']
        assert not any(call['kind'] in ('playlist_add', 'playlist_detail') for call in observed)
        assert observed == trace[:len(observed)]
    pending = unknown['searchGroupDownload']
    assert pending == trace[:len(pending)] and ADD in pending
    exact_detail = lambda tracks: dict(kind='playlist_detail', id='81', tracks=tracks, cached=False)
    assert [call for call in pending if call['kind'] in ('playlist_detail', 'playlist_add')] == [
        exact_detail([]), ADD, exact_detail([]),
    ]
    assert trace[len(pending):] == [exact_detail([dict(id=103), dict(id=101)])]
    assert confirmed['searchGroupDownload'] == trace
    with closing(sqlite3.connect((root / 'state/core.sqlite3').as_uri() + '?mode=ro', uri=True)) as db:
        assert not db.execute("SELECT 1 FROM state WHERE owner='2' AND kind='download_tasks_v1'").fetchall()

    report = dict(ok=True, actualWebView=True, stageCount=len(rows),
                  screenshots=len(rows) if screenshots else 0, guardedPlaylistAdds=1,
                  destination='81', orderedSongs=['103', '101'], clickedSong='104',
                  statusOnlyReadback=True, cancelBeforeWrite=True, nativeDownloadTasks=0,
                  queueUnchanged=True, realMusicRequests=0, realAccountWrites=0)
    (root / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n',
                                      encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()

    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False, mode=0o700)
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/search_lyric_group_collect_ui_probe.js'
    env = {key: value for key, value in os.environ.items() if not key.upper().startswith('CUMULUS_')}
    env.update(
        CUMULUS_PROOF_DIR=str(root),
        CUMULUS_PROOF_FRONTEND_DIST=str(args.frontend_dist.resolve(strict=True)),
        CUMULUS_UX_SCRIPT=str(source),
        CUMULUS_MOTION_PROOF='1',
        CUMULUS_SEARCH_UI_PROOF='1',
        CUMULUS_SEARCH_GROUP_DOWNLOAD_PROOF='1',
        CUMULUS_SEARCH_GROUP_DOWNLOAD_LYRICS_PROOF='1',
        CUMULUS_SEARCH_GROUP_DOWNLOAD_DESTINATION_PROOF='1',
        CUMULUS_SEARCH_LYRIC_GROUP_COLLECT_PROOF='1',
    )
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        directory = root / 'os-profile' / name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve(strict=True)) + os.pathsep + env.get('PATH', '')
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(strict=True), env, log, root / 'first', args.screenshots,
                      acknowledge_receipts=True)
    failed = root / 'first/failed.json'
    assert not failed.exists(), read(failed) if failed.exists() else None
    assert code == 0, f'WebView exited {code}'
    audit(root, args.screenshots)


if __name__ == '__main__':
    main()
