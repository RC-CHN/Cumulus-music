"""Real ranking cards, period/year history and read recovery in production WebViews."""
import argparse
import json
import os
from pathlib import Path
from catalog_smoke import launch

LABELS = ['home_light', 'card_keyboard_light', 'card_playing_light', 'card_paused_dark', 'card_resumed_light',
          'card_next_light', 'card_next_paused_dark', 'card_next_resumed_light', 'other_card_playing_dark',
          'global_dark', 'global_playing_dark', 'global_paused_light', 'global_resumed_dark', 'home_return_light',
          'sales_daily_light', 'weekly_singles_dark', 'album_dark', 'weekly_return_light', 'year_menu_light',
          'historical_singles_dark', 'historical_album_dark', 'historical_return_light', 'historical_week_light',
          'annual_history_dark', 'year_keyboard_dark', 'song_sales_light', 'songs_playing_dark',
          'song_comments_light', 'song_return_dark', 'cached_error_dark', 'cached_recovered_light',
          'home_empty_dark', 'home_recovered_light', 'first_read_failed_dark', 'first_read_recovered_light',
          'period_empty_dark', 'period_recovered_light']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--frontend-dist', type=Path, help='use a local production asset snapshot')
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root/'run'), CUMULUS_RANKINGS_PROOF='1',
               CUMULUS_UX_SCRIPT=str(Path(__file__).resolve().parents[2]/'desktop/examples/rankings_consistency_probe.js'))
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if args.screenshots:
        env['CUMULUS_PROOF_CAPTURE_ACK'] = '1'
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'), ('XDG_CACHE_HOME', 'cache'),
                      ('XDG_RUNTIME_DIR', 'runtime'), ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        path = root/'os-profile'/name
        path.mkdir(parents=True, mode=0o700)
        env[key] = str(path)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve())+os.pathsep+env.get('PATH', '')
    with (root/'host.log').open('x', encoding='utf-8') as log:
        code, screenshots = launch(args.binary.resolve(), env, log, root, args.screenshots)
    output = root/'run/first'
    failed = output/'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'rankings process exit {code}'
    complete = json.loads((output/'complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['rankingsUi'] and complete['stageCount'] == len(LABELS)
    states = {}
    for number, label in enumerate(LABELS, 1):
        path = output/f'ux_{number:02}_{label}.json'
        state = json.loads(path.read_text(encoding='utf-8')); states[label] = state
        assert state['theme'] == ('dark' if label.endswith('_dark') else 'light')
        assert not state['overflow'] and state['focusValid'] and all(item['hit'] for item in state['buttons'])
        if args.screenshots:
            assert path.with_suffix('.png').is_file()
        if state.get('playing'):
            player = state['nativePlayer']
            assert player['state'] == 'playing' and player['position'] > .2 and not player['error']
            assert player['queueLen'] == state['expectedQueue'] == len(state['nativeQueue']['tracks']) == 5
    assert len(states['home_light']['cardIds']) == 8
    assert {item['id'] for item in states['home_light']['cardControls']} == {'19723756', '19723757', '101', '102', '103', '104'}
    assert states['global_dark']['sameAccountRefreshVerified'] and complete['sameAccountRefreshVerified']
    card = states['home_light']['topCard']
    assert card['rowFont'] == '15px' and [image['width'] for image in card['images']] == [78, 58, 38]
    assert max(image['bottom'] for image in card['images'])-min(image['bottom'] for image in card['images']) < .1
    assert abs((card['heading']['top']+card['heading']['height']/2)-(card['frequency']['top']+card['frequency']['height']/2)) < 1
    assert states['home_return_light']['focusedCard'] == '101'
    assert states['home_return_light']['expectedScrollTop'] > 0
    for name in ['home_return_light', 'weekly_return_light']:
        assert abs(states[name]['expectedScrollTop']-states[name]['scrollTop']) < 1
    for label in ['weekly_singles_dark', 'weekly_return_light', 'historical_week_light', 'period_recovered_light']:
        assert states[label]['period'] == '周榜' and states[label]['singles'] == '数字单曲'
        assert 'week单曲' in states[label]['firstSale']
    assert states['weekly_return_light']['focusedAlbum'] == '7'
    for label in ['historical_singles_dark', 'historical_return_light', 'annual_history_dark']:
        assert states[label]['year'] == '2018年榜' and states[label]['singles'] == '数字单曲'
    year = states['year_menu_light']['yearMenu']
    assert abs(year['rect']['width']-90) < .1 and all(abs(option['rect']['height']-40) < .1 for option in year['options'])
    assert year['options'][0]['label'] == '2018年榜'
    assert states['song_return_dark']['focusedSong'] == '1' and states['song_return_dark']['focusedAction'] == 'comments'
    for before, after in [('card_paused_dark', 'card_resumed_light'), ('card_next_paused_dark', 'card_next_resumed_light'), ('global_paused_light', 'global_resumed_dark')]:
        a, b = states[before]['sourceControl'], states[after]['sourceControl']
        assert a['player']['state'] == 'paused' and b['player']['state'] == 'playing'
        for key in ['playId', 'entryId', 'queueRevision', 'origin']:
            assert a['player'][key] == b['player'][key]
        assert a['queue']['tracks'] == b['queue']['tracks'] and a['queue']['current'] == b['queue']['current']
        assert b['player']['position'] >= a['player']['position']
    assert states['card_paused_dark']['sourceControl']['player']['position'] >= 4
    assert states['card_next_resumed_light']['sourceControl']['player']['track']['id'] == '2'
    assert states['other_card_playing_dark']['staleControlRejected']
    assert states['other_card_playing_dark']['sourceControl']['player']['origin'] == {'kind': 'playlist', 'id': '19723757'}
    for label in ['card_resumed_light', 'card_next_light', 'card_next_resumed_light', 'other_card_playing_dark', 'global_playing_dark', 'global_resumed_dark']:
        state = states[label]
        identity = state['sourceControl']['player']['origin']['id']
        assert [item['id'] for item in state['cardControls'] if item['pressed'] == 'true'] == [identity]
        control = next(item for item in state['cardControls'] if item['id'] == identity)
        assert control['label'].startswith('暂停榜单') and control['opacity'] == '1'
    for label in ['card_paused_dark', 'card_next_paused_dark', 'global_paused_light']:
        state = states[label]
        identity = state['sourceControl']['player']['origin']['id']
        assert all(item['pressed'] == 'false' for item in state['cardControls'])
        assert next(item for item in state['cardControls'] if item['id'] == identity)['label'].startswith('继续播放榜单')
    assert sum(bool(state.get('playing')) for state in states.values()) == 8
    assert len(states['cached_error_dark']['cardIds']) == 8 and states['cached_error_dark']['readError']
    assert states['home_empty_dark']['empty'] == '暂无榜单' and not states['home_empty_dark'].get('readError')
    assert states['first_read_failed_dark']['readError'] and not states['first_read_failed_dark']['saleIds'] and not states['first_read_failed_dark'].get('empty')
    assert '2019年year单曲' in states['first_read_recovered_light']['firstSale']
    assert states['period_empty_dark']['empty'] == '该周期暂无榜单数据' and not states['period_empty_dark'].get('readError')
    report = dict(ok=True, states=len(LABELS), screenshots=screenshots, historySelectionVerified=True,
                  sourceFocusVerified=True, nativePlayingStates=8, sourceTransportVerified=True, realAccountWrites=0,
                  realMusicRequests=0, physicalAudioOutputVerified=False)
    (root/'report.json').write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
