"""Ordinary radio-detail program row menu in the sealed Linux WebView."""
import argparse
import json
import os
from pathlib import Path
from window_manager import launch

LABELS = ('more', 'right_click', 'share', 'next', 'download', 'comments', 'play')
COUNT = '/api/resource/commentInfo/list'


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--script', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    if os.name != 'posix' or not os.environ.get('DISPLAY'):
        parser.error('isolated X11 display required')
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root / 'run'), CUMULUS_PODCAST_PROGRAM_MENU_PROOF='1',
               CUMULUS_PROOF_FRONTEND_DIST=str(args.frontend_dist.resolve(strict=True)),
               GDK_BACKEND='x11', LC_ALL='en_US.UTF-8')
    if args.script:
        env['CUMULUS_UX_SCRIPT'] = str(args.script.resolve(strict=True))
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime')]:
        directory = root / 'os-profile' / name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    output = root / 'run/first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        result = launch(args.binary.resolve(strict=True), env, log, output, args.screenshots)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert result == 0, (root / 'host.log').read_text(encoding='utf-8')
    complete = read(output / 'complete.json')
    assert complete['ok'] and complete['stageCount'] == len(LABELS)
    assert complete['realAccountWrites'] == complete['realMusicRequests'] == 0
    locale = read(output / 'locale.json')
    assert locale['numeric'] == 'C' and all(locale[key] != 'C' for key in ('messages', 'characters')), locale
    paths = sorted(output.glob('ux_*.json'))
    assert [path.stem for path in paths] == [f'ux_{i:02}_{name}' for i, name in enumerate(LABELS, 1)]
    stages = dict(zip(LABELS, map(read, paths), strict=True))
    assert all(stage['actual'] for stage in stages.values())
    assert dict(stages['more']['menu']) == {
        'play': '播放', 'next': '下一首播放', 'comments': '查看评论(51)',
        'download': '下载', 'share': '分享...', 'copy': '复制链接',
    }, stages['more']
    assert stages['right_click']['pointerHitVoice'] == '503'
    assert dict(stages['right_click']['menu'])['comments'] == '查看评论(53)'
    link = 'https://music.163.com/dj?id=503'
    assert stages['share']['link'] == link and stages['share']['copied'] == [link]
    step = stages['next']
    assert step['copied'] == [link, link] and len(step['queueCalls']) == 1
    action = step['queueCalls'][0]['action']
    target = action['program']
    assert action == {'kind':'queue_voice', 'id':'503', 'program':target,
                      'expected_queue_revision':step['previous']['revision']}
    assert target == {'radioId':'8','ascending':True,'revision':target['revision'],'voiceId':'503'}
    assert target['revision'] and step['native']['revision'] != step['previous']['revision']
    assert step['native']['state'] == 'paused' and step['native']['rows'] == ['501','503']
    assert all(step['native'][key] == step['previous'][key] for key in ('playId','entryId','position'))
    assert stages['download']['dialog']
    played = stages['play']['native']
    assert played['voice'] == '504' and played['song'] == '1504' and played['rows'] == ['501','503','504']
    assert played['origin'] == {'kind':'podcast','id':'8'} and played['state'] == 'playing', played
    calls = complete['calls']
    count_args = [row for path,row in calls if path == COUNT]
    menu_ids = ('501','501','503','503','503','503','503','504')
    receipts = stages['play']['countReceipts']
    assert [receipt['target']['voiceId'] for receipt in receipts] == list(menu_ids), receipts
    assert all(receipt['owner'] == receipt['result']['epoch'] and
               receipt['target'] == {**target, 'voiceId': receipt['target']['voiceId']}
               for receipt in receipts), receipts
    assert [receipt['result']['value'] for receipt in receipts] == [51,51,53,53,53,53,53,54]
    # Baseline playback and the final radio selection also invoke the existing
    # player-bar observer for the current program (501 and then 504).
    wire_ids = (*menu_ids[:4], '501', *menu_ids[4:], '504')
    assert count_args == [{'resourceType':1,'resourceIds':json.dumps([id])} for id in wire_ids], count_args
    assert any(path == '/api/v1/resource/comments/A_DJ_1_503' for path,_ in calls), calls
    assert not any(path == '/api/song/enhance/privilege' for path,_ in calls), calls
    screenshots = len(list(output.glob('ux_*.png')))
    assert screenshots == (len(LABELS) if args.screenshots else 0)
    report = {'ok':True, 'stages':list(LABELS),'screenshots':screenshots,'actualWebView':True,
              'nativeNullPlayback':True, 'countType':1,'publicProgramLink':link,
              'realMusicRequests':0,'realAccountWrites':0,'physicalAudioVerified':False}
    (root / 'report.json').write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    main()
