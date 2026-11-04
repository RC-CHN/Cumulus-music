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
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--birthday', action='store_true', help='seed only the synthetic profile birthday')
    parser.add_argument('--style-write', action='store_true', help='simulate one lost style save reply without network')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/daily_history_ui_probe.js'
    script = root / 'daily-history.js'
    script.write_text(f'window.__cumulusDailyBirthdayProof = {str(args.birthday).lower()};\n'
                      f'window.__cumulusStyleWriteProof = {str(args.style_write).lower()};\n'
                      + source.read_text(encoding='utf-8'), encoding='utf-8')
    if args.birthday:
        today = datetime.now()
        birth = datetime(2000, today.month, today.day, 12)
        env['CUMULUS_DAILY_BIRTHDAY_FIXTURE'] = str(int(birth.timestamp() * 1000))
    if args.style_write:
        env['CUMULUS_STYLE_WRITE_PROOF'] = '1'
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
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    output = root / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, output, args.screenshots)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0
    complete = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
    stages = 13 if args.style_write else 11
    assert complete['ok'] and complete['stageCount'] == stages and complete['dailyPlayback'] and complete['stylePlayback']
    assert complete['styleWrite'] is args.style_write
    assert complete['stickyRecommendations']
    assert complete['styleStickyToggle']
    assert complete['dailyBatch']
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
        *[{'path': '/api/v1/discovery/recommend/songs', 'args': {'limit': 30}}] * 5,
    ]
    style_read = {'path': '/api/homepage/category/daily/song/list',
                  'args': {'source': '', 'categoryId': '', 'tagId': '', 'songId': ''}}
    style_config = {'path': '/api/homepage/daily/song/config/get', 'args': {'limit': 20}}
    style_save = {'path': '/api/homepage/daily/song/tag/save',
                  'args': {'tags': {'categoryId': 'c1', 'tagIds': ['t2']}}}
    style = [style_read, style_config]
    if args.style_write:
        style.extend([style_config, style_read, style_save, style_config, style_read, style_read])
    assert complete['styleRecommendationCalls'] == style
    assert len(list(output.glob('ux_*.png'))) == (stages if args.screenshots else 0)
    print(json.dumps({'ok': True, 'actualWebView': True, 'historyReads': len(calls),
                      'styleReads': len(style) - int(args.style_write), 'syntheticStyleWrites': int(args.style_write),
                      'dailyPlayback': True, 'stylePlayback': True,
                      'dailyBirthday': args.birthday,
                      'dailyBatch': True,
                      'stickyRecommendations': True,
                      'styleStickyToggle': True,
                      'realAccountWrites': 0}))


if __name__ == '__main__':
    main()
