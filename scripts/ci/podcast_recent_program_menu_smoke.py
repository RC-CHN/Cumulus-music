"""Prove the authenticated recent-updates row menu in a sealed GTK WebView."""
import argparse
import json
import os
from pathlib import Path

from window_manager import launch

COUNT = '/api/resource/commentInfo/list'
RECENT = '/api/social/my/recent/update/voice/list/v1'
DETAIL = '/api/dj/program/detail'
LABELS = ('retired_reply', 'current_menu', 'share', 'next', 'download', 'comments',
          'play', 'current_play')


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def verify(output, screenshots):
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    complete = read(output / 'complete.json')
    assert complete['ok'] and complete['stageCount'] == len(LABELS)
    assert complete['realAccountWrites'] == complete['realMusicRequests'] == 0
    locale = read(output / 'locale.json')
    assert locale['numeric'] == 'C'
    assert all(locale[key].casefold().replace('-', '') == 'en_us.utf8'
               for key in ('messages', 'characters')), locale
    paths = sorted(output.glob('ux_*.json'))
    assert [path.stem for path in paths] == [f'ux_{i:02}_{name}' for i, name in enumerate(LABELS, 1)]
    stages = dict(zip(LABELS, map(read, paths), strict=True))
    assert all(stage['actual'] for stage in stages.values())
    retired = stages['retired_reply']
    old = retired['old']
    current = retired['current']
    assert old['voiceId'] == '501' and current != old['revision']
    assert retired['retired'] == 'snapshot_changed' and retired['menu'] is None and retired['delivered']
    assert [(r['target'], r['error']) for r in retired['menuReplies']] == [(old, 'snapshot_changed')]
    current_menu = stages['current_menu']
    assert current_menu['pointerHitVoice'] == '503'
    target = {'revision': current, 'voiceId': '503'}
    valid = current_menu['menuReplies'][-1]
    assert valid['target'] == target and valid['owner'] == valid['value']['epoch']
    assert valid['value']['value'] == {'commentCount': 53, 'downloadComplete': False}
    assert dict(current_menu['menu']) == {
        'play': '播放', 'next': '下一首播放', 'comments': '查看评论(53)',
        'download': '下载', 'share': '分享...', 'copy': '复制链接',
    }
    link = 'https://music.163.com/dj?id=503'
    assert stages['share']['link'] == link and stages['share']['copied'] == [link]
    step = stages['next']
    assert step['copied'] == [link, link] and len(step['queueCalls']) == 1
    assert step['queueCalls'][0]['action'] == {
        'kind': 'queue_recent_program', 'target': target,
        'expected_queue_revision': step['previous']['revision'],
    }
    assert step['native']['revision'] != step['previous']['revision']
    assert step['native']['state'] == 'paused' and step['native']['rows'] == ['501', '503']
    assert all(step['native'][key] == step['previous'][key] for key in ('playId', 'entryId', 'position'))
    assert stages['download']['dialog'] and stages['download']['native'] == step['native']
    assert stages['comments']['native'] == step['native']
    play = stages['play']['native']
    assert play['voice'] == '504' and play['song'] == '1504' and play['rows'] == ['501', '504', '503']
    assert play['state'] == 'playing' and play['origin'] is None
    latest = stages['current_play']
    assert latest['native']['state'] == 'playing' and latest['native']['rows'] == play['rows']
    assert all(latest['native'][key] == latest['before'][key] for key in ('playId', 'entryId'))
    assert latest['native']['position'] >= latest['before']['position']
    assert [r['selection'] for r in latest['selections']] == [
        {'kind': 'podcast_recent', 'revision': current, 'id': voice, 'double_click': False}
        for voice in ('501', '504', '504')
    ]
    trace = complete['calls']
    assert [body for path, body in trace if path == RECENT] == [{}, {'updateTime': 1700000000123}]
    counts = [body for path, body in trace if path == COUNT]
    assert [json.loads(body['resourceIds'])[0] for body in counts] == [
        '501', '503', '503', '501', '503', '503', '503', '504', '504', '504',
    ], counts
    assert all(body['resourceType'] == 1 for body in counts)
    assert [body for path, body in trace if path == DETAIL] == [
        {'id': '501'}, {'id': '503'}, {'id': '504'},
    ], trace
    assert any(path == '/api/v1/resource/comments/A_DJ_1_503' for path, _ in trace)
    assert not any(path in {'/api/djradio/v3/get', '/api/v6/dj/program/byradio',
                            '/api/song/enhance/privilege'} for path, _ in trace)
    pngs = sorted(output.glob('ux_*.png'))
    assert [p.stem for p in pngs] == ([p.stem for p in paths] if screenshots else [])
    if screenshots:
        for stage in paths:
            checks = read(stage.with_name('paint-' + stage.name))['samples']
            assert len(checks) == 2 and all(check['ok'] for check in checks), stage
    return {'ok': True, 'stages': list(LABELS), 'screenshots': len(pngs), 'actualWebView': True,
            'nativeNullPlayback': True, 'realMusicRequests': 0, 'realAccountWrites': 0,
            'physicalAudioVerified': False}


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
    env.update(CUMULUS_PROOF_DIR=str(root / 'run'), CUMULUS_PODCAST_RECENT_PROGRAM_MENU_PROOF='1',
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
    report = verify(output, args.screenshots)
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    main()
