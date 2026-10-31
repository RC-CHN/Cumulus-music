"""Actual download tabs, lists, native filtering and progress over isolated records."""
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
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--table', action='store_true', help='Verify column resizing, voice sort cycles and operation feedback')
    parser.add_argument('--batch', action='store_true', help='Verify full-category batch UI and real local file actions')
    parser.add_argument('--navigation', action='store_true', help='Verify download detail return, focus and resized anchors')
    parser.add_argument('--menu', action='store_true', help='Verify resource menus, confirmations and actual file operations')
    parser.add_argument('--seed-delay-ms', type=int, default=0, help='Inject a bounded synthetic setup delay; never affects the application')
    args = parser.parse_args()
    if not 0 <= args.seed_delay_ms <= 30000:
        parser.error('seed delay must be between 0 and 30000 ms')
    if sum([args.table, args.batch, args.navigation, args.menu]) > 1:
        parser.error('select a single proof')
    expected = 33 if args.menu else 24 if args.navigation else 53 if args.batch else 72 if args.table else 69
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1', CUMULUS_DOWNLOAD_PAGE_UI_PROOF='1')
    if args.seed_delay_ms:
        env['CUMULUS_DOWNLOAD_SEED_DELAY_MS'] = str(args.seed_delay_ms)
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'), ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'), ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        directory = root / 'os-profile' / name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    script = 'download_menu_ui_probe.js' if args.menu else 'download_navigation_ui_probe.js' if args.navigation else 'download_batch_ui_probe.js' if args.batch else 'download_table_ui_probe.js' if args.table else 'download_page_ui_probe.js'
    env['CUMULUS_UX_SCRIPT'] = str(Path(__file__).resolve().parents[2] / 'desktop/examples' / script)
    with (root / 'first.log').open('x', encoding='utf-8') as log:
        status = launch(args.binary.resolve(), env, log, root / 'first', args.screenshots)
    failed = root / 'first/failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert status == 0, f'probe exit {status}'
    result = json.loads((root / 'first/complete.json').read_text(encoding='utf-8'))
    assert result['ok'] and result['downloadMenuUi' if args.menu else 'downloadNavigationUi' if args.navigation else 'downloadBatchUi' if args.batch else 'downloadTableUi' if args.table else 'downloadPageUi']
    assert result['realMusicRequests'] == result['realAccountWrites'] == 0
    assert result['syntheticNativeRecords'] == 146
    assert len(list((root / 'first').glob('ux_*.json'))) == result['stageCount'] == expected
    if args.screenshots:
        assert len(list((root / 'first').glob('ux_*.png'))) == expected
    result.update(actualWebView=True, physicalDisplayVerified=False, windowsVisualVerified=False)
    (root / 'audit.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({k:result[k] for k in ['ok','stageCount','actualWebView','realAccountWrites']}))


if __name__ == '__main__':
    main()
