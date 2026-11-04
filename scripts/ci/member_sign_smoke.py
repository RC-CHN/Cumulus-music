"""Verify membership check-ins through production IPC against a closed synthetic service."""
import argparse
import json
import os
import sqlite3
from contextlib import closing
from pathlib import Path

from motion_smoke import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--write', choices=('confirmed', 'lost'))
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    name = 'member_sign_write_ui_probe.js' if args.write else 'member_sign_ui_probe.js'
    script = Path(__file__).resolve().parents[2] / 'desktop/examples' / name
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_UX_SCRIPT=str(script),
               CUMULUS_MOTION_PROOF='1', CUMULUS_MEMBER_SIGN_PROOF='1')
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR',
                'APPDATA', 'LOCALAPPDATA'):
        profile = root / 'profile' / key
        profile.mkdir(parents=True, mode=0o700)
        env[key] = str(profile)
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')

    def run(folder, extra, stages):
        output = root / folder
        with (root / f'{folder}.log').open('x', encoding='utf-8') as log:
            code = launch(args.binary.resolve(), {**env, **extra}, log, output, args.screenshots)
        failed = output / 'failed.json'
        assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
        assert code == 0
        complete = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
        assert complete['ok']
        assert len(list(output.glob('ux_*.png'))) == (stages if args.screenshots else 0)
        return complete

    prefix = '/api/vipnewcenter/app/'
    card = {'path': prefix + 'minidesk/music/sign/pc', 'args': {'type': 1}}
    history = {'path': prefix + 'level/user/checkin/history/list', 'args': {}}
    detail = lambda record: {'path': prefix + 'level/user/checkin/history/detail',
                             'args': {'type': 2, 'recordId': record}}
    past = detail('009007199254740993')
    today = detail(82)
    if args.write:
        def journal_phase():
            database = root / 'state' / 'core.sqlite3'
            with closing(sqlite3.connect(database.as_uri() + '?mode=ro', uri=True)) as store:
                rows = store.execute("SELECT owner,key,value FROM state WHERE kind='member_sign'").fetchall()
            assert len(rows) == 1 and rows[0][:2] == ('2', '2026-11-01')
            return json.loads(rows[0][2])

        sign = {'path': '/api/vip-center-bff/task/sign', 'args': {}}
        first = run('first', {'CUMULUS_MEMBER_SIGN_WRITE_PROOF': args.write}, 2)
        assert first['outcome'] == args.write
        assert first['memberSignCalls'].count(sign) == 1
        assert first['memberSignCalls'][:3] == [card, card, sign]
        if args.write == 'confirmed':
            first_detail = {'path': prefix + 'level/user/checkin/history/detail',
                            'args': {'type': 1, 'signDayTime': 1793491200000}}
            assert first['memberSignCalls'] == [card, card, sign, first_detail, card]
            assert journal_phase() == 'confirmed'
        else:
            assert first['memberSignCalls'] == [card, card, sign, card, history]
            assert journal_phase() == 'unknown'
            reopened = run('reopen', {'CUMULUS_MEMBER_SIGN_WRITE_PROOF': 'lost-reopen',
                                      'CUMULUS_PROOF_REOPEN': '1'}, 3)
            assert reopened['outcome'] == 'reconciled'
            assert sign not in reopened['memberSignCalls']
            assert reopened['memberSignCalls'][-3:] == [card, history, today]
            assert reopened['memberSignCalls'].count(history) == 2
            assert journal_phase() == 'confirmed'
        report = {'ok': True, 'actualWebView': True, 'outcome': args.write,
                  'stages': 2 if args.write == 'confirmed' else 5,
                  'syntheticWrites': 1, 'realAccountWrites': 0}
        (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
        print(json.dumps(report))
        return
    complete = run('first', {}, 9)
    assert complete['stageCount'] == 9 and complete['memberPortalQr']
    assert complete['memberOfferVerified'] and complete['memberOfferReads'] == 2
    assert complete['historyPlayback'] == '2' and complete['nativeModalFocus'] and complete['noLateDetail']
    assert complete['memberSignCalls'] == [card, card, history, past, history, past,
                                           history, past, history, today, card, card]
    print(json.dumps({'ok': True, 'actualWebView': True, 'stages': 9, 'signReads': 12,
                      'historyPlayback': True, 'nativeModalFocus': True,
                      'cachedRefresh': True, 'memberPortalQr': True, 'memberOfferVerified': True,
                      'realAccountWrites': 0}))


if __name__ == '__main__':
    main()
