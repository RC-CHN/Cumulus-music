"""Production download settings with isolated directories, no download or migration."""
import argparse
import json
import os
from pathlib import Path
import sqlite3
from motion_smoke import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--baseline', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env['CUMULUS_PROOF_DIR'] = str(root)
    for variable, directory in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                                ('XDG_CACHE_HOME', 'cache'), ('APPDATA', 'roaming'),
                                ('LOCALAPPDATA', 'local')]:
        path = root / 'os-profile' / directory
        path.mkdir(parents=True)
        env[variable] = str(path)
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve())
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    script = root / 'download-layout.js'
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/download_layout_ui_probe.js'
    script.write_text('window.__downloadProofRoot=' + json.dumps(str(root / 'downloads')) + ';\n'
                      + 'window.__downloadLayoutBaseline=' + json.dumps(args.baseline) + ';\n'
                      + source.read_text(encoding='utf-8'), encoding='utf-8')
    env['CUMULUS_UX_SCRIPT'] = str(script)
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, root / 'first', args.screenshots)
    failed = root / 'first/failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'probe exited {code}'
    complete = json.loads((root / 'first/complete.json').read_text(encoding='utf-8'))
    expected = 12 if args.baseline else 20
    assert complete['ok'] and complete['stageCount'] == expected
    assert complete['realMusicRequests'] == complete['downloadsStarted'] == complete['accountWrites'] == 0
    with sqlite3.connect((root / 'state/core.sqlite3').as_uri() + '?mode=ro', uri=True) as db:
        directory = json.loads(db.execute("SELECT value FROM state WHERE owner='2' AND kind='download_directory_v1' AND key='settings'").fetchone()[0])
        assert Path(directory['root']) == root / 'downloads', 'cancelled directory change was persisted'
        assert db.execute("SELECT COUNT(*) FROM state WHERE kind IN ('download_tasks_v1', 'download_migrations_v1', 'download_file_operations_v1', 'media_transfers_v1')").fetchone()[0] == 0
    if not args.baseline:
        assert all(row['radiosCompact'] and row['choicesReadable'] and row['footerReachable'] for row in complete['checks']), complete['checks']
        for path in (root / 'first').glob('ux_*.json'):
            row = json.loads(path.read_text(encoding='utf-8'))
            assert row['header']['bottom'] <= row['body']['y'] <= row['body']['bottom'] <= row['footer']['y'], path.name
            assert row['dialog']['x'] >= 0 and row['dialog']['right'] <= row['width'], path.name
            assert row['dialog']['y'] >= 0 and row['dialog']['bottom'] <= row['height'], path.name
            assert all(control['hit'] for control in row['controls'] if control['label'] in ['关闭下载对话框', '保存设置', '保存并迁移', '开始下载', '取消']), path.name
    if args.screenshots:
        assert len(list((root / 'first').glob('ux_*.png'))) == expected
    report = dict(complete, actualWebView=True, screenshots=expected if args.screenshots else 0,
                  layoutPassed=not args.baseline, physicalDisplayVerified=False)
    (root / 'audit.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
