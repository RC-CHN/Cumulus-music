"""Sealed desktop check for completed episode subtitle navigation."""

import argparse
import json
import os
from pathlib import Path

from motion_smoke import launch
from queue_collection_smoke import isolated_profile


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
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/download_voice_podcast_ui_probe.js'
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1', CUMULUS_QUEUE_UI_PROOF='1',
               CUMULUS_DOWNLOAD_PAGE_UI_PROOF='1', CUMULUS_DOWNLOAD_VOICE_PODCAST_PROOF='1',
               CUMULUS_UX_SCRIPT=str(source))
    isolated_profile(env, root)
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve(strict=True)) + os.pathsep + env.get('PATH', '')
    output = root / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        status = launch(args.binary.resolve(strict=True), env, log, output, args.screenshots)
    failure = output / 'failed.json'
    assert not failure.exists(), failure.read_text(encoding='utf-8') if failure.exists() else ''
    assert status == 0
    read = lambda name: json.loads((output / f'{name}.json').read_text(encoding='utf-8'))
    complete = read('complete')
    assert complete['ok'] and complete['stageCount'] == 3
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    assert len(list(output.glob('ux_*.json'))) == 3
    rows, opened, returned = (read(f'ux_{i:02}_{name}') for i, name in
                              enumerate(('voice_rows', 'podcast_opened', 'download_returned'), 1))
    assert rows['linked'] == {'name': '合成下载节目所属播客', 'id': '9007199254740993'}
    assert rows['plain'] == {'name': '没有编号的播客'} and rows['mismatchedHasPodcast'] is False
    assert opened['heading'] == rows['linked']['name']
    assert returned['focus'] == 'podcast' and returned['task'] == returned['originalTask']
    assert rows['downloadVoicePodcastReads'] == []
    reads = opened['downloadVoicePodcastReads']
    assert reads == returned['downloadVoicePodcastReads'] == complete['downloadVoicePodcastReads']
    radio = rows['linked']['id']
    assert reads == [
        {'path': '/api/djradio/v3/get', 'args': {'id': radio}},
        {'path': '/api/v6/dj/program/byradio', 'args': {'radioId': radio, 'limit': 200, 'offset': 0, 'asc': True}},
        {'path': '/api/content/my/liked/voice', 'args': {'limit': 100, 'likeTime': 0}},
        {'path': '/api/dj/user/setting/resumeplay/get', 'args': {'radioId': radio}},
    ]
    player = rows['nativePlayer']
    keys = ('playId', 'entryId', 'queueRevision', 'position', 'downloadTaskId')
    assert player['track']['id'] == '1' and player['downloadTaskId'] and player['playId']
    for sample in (rows, opened, returned):
        assert sample['nativeQueue'] == rows['nativeQueue']
        assert sample['nativePlayer']['state'] == 'paused' and not sample['nativePlayer']['error']
        assert all(sample['nativePlayer'][key] == player[key] for key in keys)
    screenshots = len(list(output.glob('ux_*.png')))
    assert screenshots == (3 if args.screenshots else 0)
    report = dict(ok=True, actualWebView=True, screenshots=screenshots, nativeStates=3,
                  pausedDownloadPreserved=True, focusRestored=True, reads=reads,
                  realAccountWrites=0, realMusicRequests=0, physicalAudioOutputVerified=False)
    (root / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    main()
