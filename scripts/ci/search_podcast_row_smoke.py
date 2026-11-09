"""Trusted X11 input for classified Search Podcasts and its native queue."""
import argparse
from contextlib import closing
import json
import os
from pathlib import Path
import sys
import time

from search_podcast_playback_smoke import BYRADIO, CHECK, DETAIL, URL, read
from search_queue_drop_smoke import TrustedDrag
from window_manager import launch

STAGES = ('single_armed', 'single_detail', 'double_armed', 'double_played', 'index_paused', 'failed_kept')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    args = parser.parse_args()
    if sys.platform != 'linux' or not os.environ.get('DISPLAY'):
        parser.error('this trusted pointer recipe requires an isolated Xvfb')
    assert len(Path('/proc/net/route').read_text().splitlines()) == 1, 'run in a sealed no-route network namespace'
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False, mode=0o700)
    repo = Path(__file__).resolve().parents[2]
    env = {key: value for key, value in os.environ.items()
           if not key.upper().startswith(('CUMULUS_', 'WEBVIEW2_')) and key.upper() != 'MPV_DLL_DIR_CUMULUS'}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1', CUMULUS_SEARCH_UI_PROOF='1',
               CUMULUS_SEARCH_PODCAST_PLAYBACK_PROOF='1', CUMULUS_SEARCH_PODCAST_ROW_PROOF='1',
               CUMULUS_UX_SCRIPT=str(repo / 'desktop/examples/search_podcast_row_ui_probe.js'),
               CUMULUS_PROOF_FRONTEND_DIST=str(args.frontend_dist.resolve(strict=True)),
               LC_ALL='en_US.UTF-8', GDK_BACKEND='x11')
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR'):
        folder = root / key
        folder.mkdir(mode=0o700)
        env[key] = str(folder)

    with closing(TrustedDrag()) as pointer, (root / 'host.log').open('x', encoding='utf-8') as log:
        def input_at_capture(path, payload, _pid):
            if path.stem not in ('ux_01_single_armed', 'ux_03_double_armed', 'ux_04_double_played'):
                return
            pointer.move(payload['pointer'])
            for _ in range(2 if path.stem == 'ux_03_double_armed' else 1):
                pointer.button(True)
                time.sleep(.04)
                pointer.button(False)
                time.sleep(.06)
        out = root / 'first'
        code = launch(args.binary.resolve(strict=True), env, log, out, True, before_ack=input_at_capture)
    failed = out / 'failed.json'
    assert not failed.exists(), read(failed) if failed.exists() else None
    complete = read(out / 'complete.json')
    assert code == 0 and complete['ok'] and complete['stageCount'] == len(STAGES)
    rows = {p.stem.split('_', 2)[2]: read(p) for p in sorted(out.glob('ux_*.json'))}
    assert tuple(rows) == STAGES
    assert rows['single_detail']['gestures'] == [['click', 1]]
    assert complete['trustedIndexClicks'] == 1
    assert complete['gestures'] == [['click', 1], ['click', 1], ['click', 2], ['dblclick', 2]]
    assert rows['single_detail']['nativePlayer']['state'] == 'idle'
    assert rows['single_detail']['queue']['tracks'] == []
    old = rows['double_armed']['nativePlayer']
    first = rows['double_played']['nativePlayer']
    assert old['state'] == 'paused' and old['track']['voiceId'] == '400'
    assert first['state'] == 'playing' and first['track']['voiceId'] == '1'
    assert first['origin'] == {'kind': 'podcast', 'id': '7'} and first['mode'] == 'single_loop'
    assert first['playId'] != old['playId'] and 18.8 <= first['position'] < 27
    new_queue = rows['double_played']['queue']
    assert [item['track']['voiceId'] for item in new_queue['tracks']] == (
        ['400'] + [str(id_) for id_ in range(251, 0, -1)])
    assert new_queue['tracks'][0]['entryId'] == rows['double_armed']['queue']['current']
    assert new_queue['current'] == first['entryId']
    paused = rows['index_paused']['nativePlayer']
    kept = rows['failed_kept']['nativePlayer']
    assert paused['state'] == kept['state'] == 'paused'
    for field in ('playId', 'entryId', 'queueRevision'):
        assert first[field] == paused[field] == kept[field]
    assert paused['position'] == kept['position']
    assert rows['index_paused']['searchPodcastCalls'] == rows['double_played']['searchPodcastCalls']
    wire = complete['searchPodcastCalls']
    for path in (DETAIL, CHECK, URL):
        calls = [call for call in wire if call['path'] == path]
        assert len(calls) == 2 and all(not call['cached'] for call in calls), (path, calls)
    assert [call['args']['id'] for call in wire if call['path'] == DETAIL] == ['400', '1']
    assert [call['args']['voiceId'] for call in wire if call['path'] == CHECK] == ['400', '1']
    assert [call['args']['ids'] for call in wire if call['path'] == URL] == ['["1400"]', '["1001"]']
    assert [call['args'] for call in wire if call['path'] == BYRADIO and call['args']['limit'] == 250] == [
        {'radioId': '7', 'offset': offset, 'limit': 250, 'asc': False} for offset in (0, 250)
    ] + [{'radioId': '8', 'offset': 0, 'limit': 250}]
    assert all(call['path'] != '/api/dj/around/program/v1' for call in wire)
    locale = read(out / 'locale.json')
    assert locale == {'numeric': 'C', 'messages': 'en_US.UTF-8', 'characters': 'en_US.UTF-8'}
    assert len(list(out.glob('ux_*.png'))) == len(STAGES)
    report = dict(ok=True, actualWebView=True, trustedX11BodyGestures=True, trustedX11IndexPause=True, nullOutput=True,
                  states=STAGES, screenshots=len(STAGES), locale=locale, classifiedReceipt=True,
                  oldEntryRetained=True, singleDetailWithoutPlayback=True, sameNativePause=True,
                  failedReadPreservedClock=True, realAccountWrites=0, networkNamespaceWithoutRoutes=True,
                  wire=wire)
    (root / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    main()
