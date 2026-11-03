"""Verify history and style shortcuts from Daily Recommendations in a real WebView."""
import argparse
from datetime import datetime
import json
import os
from pathlib import Path

from motion_smoke import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--birthday', action='store_true', help='seed only the synthetic profile birthday')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/daily_history_ui_probe.js'
    script = root / 'daily-history.js'
    script.write_text(f'window.__cumulusDailyBirthdayProof = {str(args.birthday).lower()};\n'
                      + source.read_text(encoding='utf-8'), encoding='utf-8')
    if args.birthday:
        today = datetime.now()
        birth = datetime(2000, today.month, today.day, 12)
        env['CUMULUS_DAILY_BIRTHDAY_FIXTURE'] = str(int(birth.timestamp() * 1000))
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_UX_SCRIPT=str(script),
               CUMULUS_MOTION_PROOF='1', CUMULUS_DAILY_HISTORY_PROOF='1',
               CUMULUS_STYLE_RECOMMENDATIONS_PROOF='1')
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR',
                'APPDATA', 'LOCALAPPDATA'):
        profile = root / 'profile' / key
        profile.mkdir(parents=True, mode=0o700)
        env[key] = str(profile)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    output = root / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, output, args.screenshots)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0
    complete = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['stageCount'] == 5 and complete['dailyPlayback'] and complete['stylePlayback']
    assert complete['dailyCache'] and complete['dailyBirthday'] is args.birthday
    prefix = '/api/discovery/recommend/songs/history/'
    calls = [call for call in complete['dailyHistoryCalls'] if call['path'].startswith(prefix)]
    assert calls == [
        {'path': prefix + 'recent', 'args': {}},
        {'path': prefix + 'detail', 'args': {'date': '2026-10-26'}},
    ]
    daily = [call for call in complete['dailyHistoryCalls'] if call['path'] in (
        '/api/rtrs/abt/front/expinfo/list', '/api/v1/discovery/recommend/songs',
        '/api/v3/discovery/recommend/songs')]
    assert daily == [
        {'path': '/api/rtrs/abt/front/expinfo/list', 'args': {'expNames': 'PH-PC-Blacklist'}},
        *[{'path': '/api/v1/discovery/recommend/songs', 'args': {'limit': 30}}] * 3,
    ]
    assert complete['styleRecommendationCalls'] == [
        {'path': '/api/homepage/category/daily/song/list',
         'args': {'source': '', 'categoryId': '', 'tagId': '', 'songId': ''}},
        {'path': '/api/homepage/daily/song/config/get', 'args': {'limit': 20}},
    ]
    assert len(list(output.glob('ux_*.png'))) == (5 if args.screenshots else 0)
    print(json.dumps({'ok': True, 'actualWebView': True, 'historyReads': len(calls),
                      'styleReads': 2, 'dailyPlayback': True, 'stylePlayback': True,
                      'dailyBirthday': args.birthday,
                      'realAccountWrites': 0}))


if __name__ == '__main__':
    main()
