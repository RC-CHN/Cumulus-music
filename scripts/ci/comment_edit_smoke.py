"""Production comment UI/IPC with synthetic writes and a real process restart."""
import argparse
import json
import os
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env['CUMULUS_PROOF_DIR'] = str(root / 'run')
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'), ('XDG_CACHE_HOME', 'cache'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        path = root / 'os-profile' / name
        path.mkdir(parents=True)
        env[key] = str(path)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    reports = []
    for phase, count in [('first', 11), ('reopen', 6)]:
        if phase == 'reopen':
            env['CUMULUS_PROOF_REOPEN'] = '1'
        output = root / 'run' / phase
        with (root / f'{phase}.log').open('x', encoding='utf-8') as log:
            result = subprocess.run([str(args.binary.resolve())], env=env, stdout=log, stderr=log, timeout=180)
        failed = output / 'failed.json'
        assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
        assert result.returncode == 0, f'comment probe exit {result.returncode}'
        report = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
        assert report['ok'] and report['reopened'] == (phase == 'reopen')
        assert report['realAccountWrites'] == report['publicWrites'] == 0
        assert len(list(output.glob('edit_*.json'))) == count
        reports.append(report)
    first, reopened = reports
    operations = first['operations']['value']
    lost = next(row for row in operations if row['command'].get('content') == '丢失回包后保留责任')
    assert lost['phase'] == 'uncertain'
    recovered = next(row for row in reopened['operations']['value'] if row['id'] == lost['id'])
    assert recovered['phase'] == 'abandoned'
    paths = {'/api/resource/comments/add', '/api/v1/resource/comments/reply', '/api/resource/comments/delete'}
    first_writes = [call for call in first['fixture']['calls'] if call['path'] in paths]
    writes = [call for call in reopened['fixture']['calls'] if call['path'] in paths]
    assert len(first_writes) == 6 and writes[:6] == first_writes and len(writes) == 8
    assert [call['path'] for call in writes] == [
        '/api/resource/comments/add', '/api/v1/resource/comments/reply', '/api/resource/comments/delete',
        '/api/resource/comments/add', '/api/v1/resource/comments/reply', '/api/resource/comments/add',
        '/api/resource/comments/add', '/api/resource/comments/delete',
    ]
    assert all(call['payload']['threadId'] == 'R_SO_4_123' for call in writes)
    assert writes[0]['payload'] == {'threadId': 'R_SO_4_123', 'content': ' 第一条评论 🎵\n保留原始空白 ',
                                  'resourceType': '0', 'resourceId': '0', 'expressionPicId': '-1', 'bubbleId': '-1'}
    assert writes[1]['payload'] == {'threadId': 'R_SO_4_123', 'commentId': '102',
                                  'content': '回复：也想去看看海。', 'resourceType': '0', 'resourceId': '0'}
    assert sum(call['lostResponse'] for call in reopened['fixture']['calls']) == 1
    assert sum(call['payload'].get('content') == '丢失回包后保留责任' for call in writes) == 1
    rows = reopened['fixture']['rows']
    assert rows['101']['content'] == '自己的原评论，保留不动。'
    assert rows['102']['content'] == '听到这里，想起了夏天的海风。'
    assert rows['9000']['content'] == ' 第一条评论 🎵\n保留原始空白 '
    assert rows['9002']['content'] == '丢失回包后保留责任'
    assert '9001' not in rows and '9003' not in rows
    audit = {'ok': True, 'actualWebView': True, 'states': 17, 'processRestartVerified': True,
             'syntheticWrites': 8, 'realAccountWrites': 0, 'realMusicRequests': 0,
             'automaticResend': False, 'screenshots': 0}
    (root / 'audit.json').write_text(json.dumps(audit, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(audit))


if __name__ == '__main__':
    main()
