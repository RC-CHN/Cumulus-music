"""Verify the real Together leave-record UI with sealed local API/IM/audio."""
import argparse
import json
import os
from pathlib import Path

from history_row_next_smoke import share_qr_payload
from motion_smoke import launch
from vinyl_lyric_copy_smoke import require_xvfb

STAGES = (
    'leave_cancelled', 'leave_waiting', 'leave_malformed', 'leave_uncertain',
    'leave_recovered', 'leave_direct', 'leave_dismissed', 'leave_queue_clear',
    'leave_queue_dismissed',
)
END = {'roomId': 'test-room', 'shareInfo': '{"thisListenCount":1}'}
PUBLIC_ACTIVITY = 'https://music.163.com/m/at/5f0880bdd6666f3545196353'


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def audit(output, screenshots):
    assert {path.stem for path in output.glob('leave_*.json')} == set(STAGES)
    complete = read(output / 'complete.json')
    assert complete['ok'] and complete['stageCount'] == len(STAGES)
    host = read(output / 'host-final.json')
    assert all(host[key] is True for key in (
        'ui_ok', 'local_restored', 'fixture_api', 'fixture_im', 'fixture_audio',
    ))
    assert host['end_requests'] == 5 and complete['endRequests'] == [END] * 5
    assert host['im_closed'] > 0 and not host['real_peer_verified']
    data = {name: read(output / f'{name}.json') for name in STAGES}
    for name, count in zip(STAGES, (0, 1, 2, 3, 3, 4, 4, 5, 5), strict=True):
        assert data[name]['endRequests'] == [END] * count, name
    for name in ('leave_cancelled', 'leave_waiting', 'leave_malformed', 'leave_uncertain', 'leave_recovered'):
        assert data[name]['modalOpen'] is False, name
    assert data['leave_cancelled']['roomOpen'] is True
    assert all(data[name]['roomOpen'] is False for name in ('leave_waiting', 'leave_malformed', 'leave_recovered'))
    uncertain = data['leave_uncertain']
    assert uncertain['pending'] is True and uncertain['pendingPhase'] == 'uncertain'
    assert uncertain['acknowledged'] is False
    operations = []
    for stage, close, gesture in (
        ('leave_direct', 'leave_dismissed', 'dialog_cancel'),
        ('leave_queue_clear', 'leave_queue_dismissed', 'close_button'),
    ):
        row = data[stage]
        receipt = dict(row['receipt'])
        operation = receipt.pop('operationId')
        assert isinstance(operation, str) and operation
        operations.append(operation)
        assert receipt == {
            'thisListenCount': 3, 'thisDuration': 35, 'totalListenCount': 27, 'totalDuration': 3665,
            'selfAvatarUrl': 'https://p1.music.126.net/avatar?param=320y320',
            'peerAvatarUrl': 'https://p2.music.126.net/peer?param=320y320',
        }
        assert row['labels'] == ['本次一起听了', '本次陪伴彼此']
        assert row['values'] == ['3首歌曲', '1分钟']
        assert row['total'] == '累计27首歌曲，1小时1分钟'
        assert row['avatars'] == ['我的头像', '听友头像'] and row['focused'] == '关闭一起听记录'
        assert row['community'] == 'CumulusMusic' and row['layout'] == [442, 404]
        assert all(abs(actual - expected) <= .5 for actual, expected in zip(row['rect'], (442, 404), strict=True))
        assert len(row['overflow']) == 2 and all(0 <= excess <= 1 for excess in row['overflow'])
        assert abs(row['qrWidth'] - 72) <= .5 and share_qr_payload(row['qr']) == PUBLIC_ACTIVITY
        assert data[close]['operationId'] == operation and data[close]['gesture'] == gesture
        assert data[close]['modalOpen'] is False and data[close]['activeOutsideModal'] is True
        focus = ('H1', '一起听') if close == 'leave_dismissed' else ('BUTTON', '关闭播放队列')
        assert (data[close]['activeTag'], data[close]['activeLabel']) == focus
    assert len(set(operations)) == 2
    if screenshots:
        assert {path.stem for path in output.glob('leave_*.png')} == set(STAGES)
        for name in ('leave_direct', 'leave_queue_clear'):
            paint = read(output / f'paint-{name}.json')
            assert len(paint['samples']) == 1 and paint['samples'][0]['ok'] is True
    return {
        'ok': True, 'actualWebView': True, 'stages': len(STAGES),
        'screenshots': len(STAGES) if screenshots else 0, 'confirmedEndRequests': 5,
        'replayedRecord': False, 'queueClearPolling': True, 'fixedPublicQr': True,
        'realPeerVerified': False, 'fixtureApiImAudio': True,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    if args.screenshots:
        require_xvfb()
    root = args.root.resolve()
    root.mkdir(parents=True, mode=0o700)
    output = root / 'run'
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(output), CUMULUS_TOGETHER_LEAVE_RECORD_PROOF='1')
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR',
                'APPDATA', 'LOCALAPPDATA'):
        directory = root / key
        directory.mkdir(mode=0o700)
        env[key] = str(directory)
    if args.runtime:
        env['PATH'] = os.pathsep.join([str(args.runtime.resolve(strict=True)), env.get('PATH', '')])
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(strict=True), env, log, output, args.screenshots,
                      capture_glob='leave_*.json', acknowledge_receipts=True)
    failure = output / 'failed.json'
    assert not failure.exists(), read(failure) if failure.exists() else ''
    assert code == 0, code
    report = audit(output, args.screenshots)
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
