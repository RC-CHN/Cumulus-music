"""Verify message tabs and opt-in synthetic actions through a real desktop WebView."""
import argparse
import json
import os
from pathlib import Path

from motion_smoke import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--attachments', action='store_true')
    mode.add_argument('--deletion', action='store_true')
    mode.add_argument('--reply', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    script = Path(__file__).resolve().parents[2] / 'desktop/examples/message_drawer_ui_probe.js'
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1',
               CUMULUS_MESSAGE_DRAWER_PROOF='1', CUMULUS_UX_SCRIPT=str(script))
    if args.attachments:
        injected = root / 'attachment-probe.js'
        injected.write_text('window.__cumulusMessageAttachmentsProof = true;\n' +
                            script.read_text(encoding='utf-8'), encoding='utf-8')
        env.update(CUMULUS_UX_SCRIPT=str(injected), CUMULUS_MESSAGE_ATTACHMENTS_PROOF='1',
                   CUMULUS_SEARCH_UI_PROOF='1')
    if args.deletion:
        injected = root / 'deletion-probe.js'
        injected.write_text('window.__cumulusMessageDeletionProof = true;\n' +
                            script.read_text(encoding='utf-8'), encoding='utf-8')
        env.update(CUMULUS_UX_SCRIPT=str(injected), CUMULUS_MESSAGE_DELETION_PROOF='1')
    if args.reply:
        injected = root / 'reply-probe.js'
        injected.write_text('window.__cumulusMessageReplyProof = true;\n' +
                            script.read_text(encoding='utf-8'), encoding='utf-8')
        env.update(CUMULUS_UX_SCRIPT=str(injected), CUMULUS_MESSAGE_REPLY_PROOF='1')
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR',
                'APPDATA', 'LOCALAPPDATA'):
        profile = root / 'os-profile' / key
        profile.mkdir(parents=True, mode=0o700)
        env[key] = str(profile)
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    output = root / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, output, args.screenshots)
    failure = output / 'failed.json'
    assert not failure.exists(), failure.read_text(encoding='utf-8') if failure.exists() else ''
    assert code == 0
    report = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
    if args.reply:
        verify_reply(report, output, args.screenshots)
        report['actualWebView'] = True
        (root / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
        print(json.dumps(report, ensure_ascii=False))
        return
    if args.deletion:
        verify_deletion(report, output, args.screenshots)
        report['actualWebView'] = True
        (root / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
        print(json.dumps(report, ensure_ascii=False))
        return
    if args.attachments:
        verify_attachments(report, output, args.screenshots)
        report['actualWebView'] = True
        (root / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
        print(json.dumps(report, ensure_ascii=False))
        return
    assert report['ok'] and report['staleVisitDiscarded'] and report['count'] == 7
    assert report['realAccountWrites'] == report['realMusicRequests'] == 0
    assert not report['messageRequests']['blockedWrite']
    assert len(list(output.glob('ux_*.json'))) == 7
    assert len(list(output.glob('ux_*.png'))) == (7 if args.screenshots else 0)
    t = 1_700_000_100_000
    reads = {}
    for path, request in report['messageRequests']['requests']:
        reads.setdefault(path, []).append(request)
    assert reads == {
        '/api/pl/count': [{}, {}, {}],
        '/api/msg/private/users': [
            dict(limit=15, offset=0, time=-1),
            dict(limit=15, offset=0, time=t-14),
            dict(limit=15, offset=0, time=-1),
        ],
        '/api/msg/private/history': [
            dict(limit=10, offset=0, time=-1, userId='505'),
            dict(limit=10, offset=0, time=t, userId='505'),
        ],
        '/api/v1/user/comments/2': [
            dict(limit=10, beforeTime=-1),
            dict(limit=10, beforeTime=t),
            dict(limit=10, beforeTime=t),
        ],
        '/api/forwards/get': [
            dict(total=True, limit=10, offset=0, time=-1),
            dict(total=True, limit=10, offset=0, time=t-2),
        ],
        '/api/msg/notices': [
            dict(total=True, limit=10, offset=0, time=-1),
            dict(total=True, limit=10, offset=0, time=t-3),
            dict(total=True, limit=10, offset=0, time=-1),
            dict(total=True, limit=10, offset=0, time=-1),
        ],
    }, reads
    report['actualWebView'] = True
    (root / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


def verify_attachments(report, output, screenshots):
    assert report['ok'] and report['attachmentTargetsVerified'] and report['count'] == 9
    assert report['realAccountWrites'] == report['realMusicRequests'] == 0
    assert not report['messageRequests']['blockedWrite']
    assert len(list(output.glob('ux_*.json'))) == 9
    assert len(list(output.glob('ux_*.png'))) == (9 if screenshots else 0)
    requests = report['messageRequests']['requests']
    expected = [
        ('/api/v1/resource/comments/R_AL_3_7', '701'),
        ('/api/v1/resource/comments/R_SO_4_3', '411'),
        ('/api/v1/resource/comments/A_PL_0_2', '702'),
    ]
    assert [(path, args['commentId']) for path, args in requests
            if path.startswith('/api/v1/resource/comments/')] == expected
    assert sum(path == '/api/msg/private/history' for path, _ in requests) == 1
    assert sum(path == '/api/msg/notices' for path, _ in requests) == 1


def verify_deletion(report, output, screenshots):
    assert report['ok'] and report['privateDeletionVerified'] and report['count'] == 9
    assert report['realAccountWrites'] == report['realMusicRequests'] == 0
    assert not report['messageRequests']['blockedWrite']
    assert len(list(output.glob('ux_*.json'))) == 9
    assert len(list(output.glob('ux_*.png'))) == (9 if screenshots else 0)
    requests = report['messageRequests']['requests']
    assert [(path, args) for path, args in requests if '/delete' in path] == [
        ('/api/msg/private/delete', {'userId': 502}),
        ('/api/msg/private/delete', {'userId': 501}),
        ('/api/msg/private/delete/single', {'id': 301}),
        ('/api/msg/private/delete/single', {'id': '9007199254740997'}),
    ]
    history = [args for path, args in requests if path == '/api/msg/private/history']
    assert history == [dict(limit=10, offset=0, time=-1, userId='505')] * 5, history


def verify_reply(report, output, screenshots):
    assert report['ok'] and report['privateReplyVerified'] and report['count'] == 10
    assert report['realAccountWrites'] == report['realMusicRequests'] == 0
    assert not report['messageRequests']['blockedWrite']
    assert len(list(output.glob('ux_*.json'))) == 10
    assert len(list(output.glob('ux_*.png'))) == (10 if screenshots else 0)
    requests = report['messageRequests']['requests']
    sends = [args for path, args in requests if path == '/api/msg/private/send']
    assert sends == [dict(type='text', id=0, userIds=['505'], msg=text) for text in
                     ['  合成拦截正文\n', '  合成发送正文\n', '合成迟到发送正文', '合成未知正文']], sends
    assert [args for path, args in requests if path == '/api/w/v1/user/bindings/2'] == [{'userId': '2'}] * 5
    histories = [args['userId'] for path, args in requests if path == '/api/msg/private/history']
    assert histories == ['505', '506', '505', '505', '505', '505'], histories


if __name__ == '__main__':
    main()
