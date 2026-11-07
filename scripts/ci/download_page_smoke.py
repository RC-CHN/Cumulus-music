"""Actual download tabs, lists, native filtering and progress over isolated records."""
import argparse
import json
import os
from pathlib import Path
from motion_smoke import launch


def verify_mv_coverage(value):
    native = value['nativeIds']
    assert len(native) == len(set(native)) == 129, 'unique frozen native MV inventory'
    found = {}
    for viewport in value['viewportSamples']:
        assert abs(viewport['y'] - viewport['scrollTop']) <= 1, 'requested MV viewport settled'
        assert viewport['cards'], 'nonempty rendered MV viewport'
        for card in viewport['cards']:
            index = card['index']
            assert isinstance(index, int) and 0 <= index < len(native)
            assert card['id'] == native[index], 'rendered MV ID matches frozen native index'
            found[index] = card['id']
    assert len(found) == len(set(found.values())) == 129, 'all 129 MV records reachable'


def verify_download_quality(output, result, screenshots):
    stages = ('startup_unknown', 'manual_failure', 'membership_recovered', 'default_saved',
              'single_download_choice', 'default_preserved')
    values = [json.loads((output / f'ux_{index:02d}_{stage}.json').read_text(encoding='utf-8'))
              for index, stage in enumerate(stages, 1)]
    assert result['ok'] and result['downloadQualityUi']
    assert result['realMusicRequests'] == result['realAccountWrites'] == 0
    assert result['stageCount'] == len(list(output.glob('ux_*.json'))) == len(stages)
    if screenshots:
        assert len(list(output.glob('ux_*.png'))) == len(stages)
    reports = [value['downloadQualityFixture'] for value in values]
    assert [report['membershipReads'] for report in reports] == [1, 2, 3, 3, 3, 3]
    assert [report['savedQuality'] for report in reports] == ['vivid'] * 3 + ['lossless'] * 3
    assert all(value['retained'] for value in values[:3])
    assert values[1]['message']
    assert [row['name'] for row in values[2]['rows'] if not row['disabled']] == [
        '高清臻音', 'Hi-Res', '无损', '极高', '标准',
    ]
    assert values[4]['selected'] == values[5]['requested'] == 'exhigh'
    assert values[5]['actual'] == 'standard'
    assert all(not report['downloadReads'] and not report['tasks'] for report in reports[:-1])
    assert reports[-1]['downloadReads'] == [{'id': '1', 'level': 'exhigh', 'immerseType': 'c51'}]
    assert reports[-1]['tasks'] == [{
        'songId': '1', 'status': 'complete', 'requested': 'exhigh', 'actual': 'standard',
        'authorizations': 1, 'published': True,
    }]
    assert result['downloadQualityFixture'] == reports[-1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--mv-replay', action='store_true', help='Verify real local MV EOF and replay')
    parser.add_argument('--table', action='store_true', help='Verify column resizing, voice sort cycles and operation feedback')
    parser.add_argument('--batch', action='store_true', help='Verify full-category batch UI and real local file actions')
    parser.add_argument('--navigation', action='store_true', help='Verify download detail return, focus and resized anchors')
    parser.add_argument('--menu', action='store_true', help='Verify resource menus, confirmations and actual file operations')
    parser.add_argument('--quality-settings', action='store_true', help='Verify native default-download preferences, membership recovery and a separate single download')
    parser.add_argument('--seed-delay-ms', type=int, default=0, help='Inject a bounded synthetic setup delay; never affects the application')
    args = parser.parse_args()
    if not 0 <= args.seed_delay_ms <= 30000:
        parser.error('seed delay must be between 0 and 30000 ms')
    if sum([args.table, args.batch, args.navigation, args.menu, args.mv_replay, args.quality_settings]) > 1:
        parser.error('select a single proof')
    if args.quality_settings and args.seed_delay_ms:
        parser.error('--seed-delay-ms is for inventory fixtures')
    expected = 41 if args.menu else 24 if args.navigation else 53 if args.batch else 72 if args.table else 69
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1', CUMULUS_DOWNLOAD_PAGE_UI_PROOF='1')
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if args.seed_delay_ms:
        env['CUMULUS_DOWNLOAD_SEED_DELAY_MS'] = str(args.seed_delay_ms)
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'), ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'), ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        directory = root / 'os-profile' / name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    script = 'download_quality_ui_probe.js' if args.quality_settings else 'mv_replay_ui_probe.js' if args.mv_replay else 'download_menu_ui_probe.js' if args.menu else 'download_navigation_ui_probe.js' if args.navigation else 'download_batch_ui_probe.js' if args.batch else 'download_table_ui_probe.js' if args.table else 'download_page_ui_probe.js'
    if args.quality_settings:
        env['CUMULUS_DOWNLOAD_QUALITY_UI_PROOF'] = '1'
    if script == 'download_page_ui_probe.js':
        env['CUMULUS_DOWNLOAD_LYRIC_REPAIR_PROOF'] = '1'
    if args.menu:
        env['CUMULUS_DOWNLOAD_MENU_COMMENT_PROOF'] = '1'
    env['CUMULUS_UX_SCRIPT'] = str(Path(__file__).resolve().parents[2] / 'desktop/examples' / script)
    with (root / 'first.log').open('x', encoding='utf-8') as log:
        status = launch(args.binary.resolve(), env, log, root / 'first', args.screenshots)
    failed = root / 'first/failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert status == 0, f'probe exit {status}'
    result = json.loads((root / 'first/complete.json').read_text(encoding='utf-8'))
    if args.quality_settings:
        verify_download_quality(root / 'first', result, args.screenshots)
        result.update(actualWebView=True, physicalDisplayVerified=False, windowsVisualVerified=False)
        (root / 'audit.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
        print(json.dumps({k: result[k] for k in ['ok', 'stageCount', 'actualWebView', 'downloadQualityFixture']}))
        return
    if args.mv_replay:
        assert result['ok'] and result['nativeMvReplay'] and result['local'] and result['frames']
        ended, restarted = [
            json.loads((root / 'first' / f'{stage}.json').read_text(encoding='utf-8'))
            for stage in ('ux_01_mv_ended', 'ux_02_mv_restarted')
        ]
        assert abs(ended['position'] - ended['duration']) < .1
        assert 0 < restarted['position'] < 3 and restarted['duration'] == ended['duration']
        assert not args.screenshots or len(list((root / 'first').glob('ux_*.png'))) == 2
        print(json.dumps(dict(ok=True, nativeMvReplay=True, actualWebView=True)))
        return
    assert result['ok'] and result['downloadMenuUi' if args.menu else 'downloadNavigationUi' if args.navigation else 'downloadBatchUi' if args.batch else 'downloadTableUi' if args.table else 'downloadPageUi']
    assert result['realMusicRequests'] == result['realAccountWrites'] == 0
    assert result['syntheticNativeRecords'] == 146
    assert len(list((root / 'first').glob('ux_*.json'))) == result['stageCount'] == expected
    if args.menu:
        assert result['playerDownloadStateVerified']
        assert result['downloadMenuCommentVerified']
        assert result['downloadMenuCommentReads'][:7] == [
            {'resourceType': 4, 'resourceIds': json.dumps([song], separators=(',', ':'))}
            for song in ('1', '2', '3', '1', '1', '1', '1')
        ]
        plays = set()
        for status in ('waiting', 'working', 'complete'):
            stage, = (root / 'first').glob(f'ux_*_player_download_{status}.json')
            evidence = json.loads(stage.read_text(encoding='utf-8'))
            player = evidence['nativePlayer']
            assert evidence['status'] == status and evidence['playId'] == player['playId']
            assert player['resourceKind'] == 'catalog' and player['track']['id'] == '10'
            plays.add(player['playId'])
        assert len(plays) == 1 and None not in plays
    if not any([args.table, args.batch, args.navigation, args.menu]):
        assert result['tabWithoutFramesVerified']
        assert result['virtualKeyboardWithoutFramesVerified']
        assert result['completedLyricsRepairVerified']
        paths = list((root / 'first').glob('ux_*_mv_offline_playback.json'))
        assert len(paths) == 1, 'one decoded local MV checkpoint'
        video = json.loads(paths[0].read_text(encoding='utf-8'))
        assert all(video[key] for key in ('exactMenu', 'routeOpened', 'local', 'retryKeptPlayId'))
        assert video['id'] == '133' and video['frames'] > 0 and video['nonBlackPixels'] >= 100
        for width in [1120, 760]:
            paths = list((root / 'first').glob(f'ux_*_mv_continuous_{width}.json'))
            assert len(paths) == 1, 'one complete MV viewport sweep per width'
            verify_mv_coverage(json.loads(paths[0].read_text(encoding='utf-8')))
    if args.screenshots:
        assert len(list((root / 'first').glob('ux_*.png'))) == expected
    result.update(actualWebView=True, physicalDisplayVerified=False, windowsVisualVerified=False)
    (root / 'audit.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({k:result[k] for k in ['ok','stageCount','actualWebView','realAccountWrites']}))


if __name__ == '__main__':
    main()
