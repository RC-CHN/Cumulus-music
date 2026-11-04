"""Real comment report WebView and encrypted PC request proof with a closed synthetic account."""
import argparse
import json
import os
from pathlib import Path

from window_manager import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--root', required=True, type=Path)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    script = Path(__file__).resolve().parents[2] / 'desktop/examples/comment_report_ui_probe.js'
    run = root / 'run'
    env.update(CUMULUS_PROOF_DIR=str(run), CUMULUS_UX_SCRIPT=str(script))
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        path = root / 'os-profile' / name
        path.mkdir(parents=True, mode=0o700)
        env[key] = str(path)
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve(strict=True)) + os.pathsep + env.get('PATH', '')
    output = run / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        status = launch(args.binary.resolve(strict=True), env, log, output, args.screenshots)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert status == 0, f'probe exited {status}'
    host = (root / 'host.log').read_text(encoding='utf-8')
    assert 'panicked at' not in host, host
    complete = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['realMusicRequests'] == complete['remoteWrites'] == 0
    stages = {path.stem.split('_', 2)[2]: json.loads(path.read_text(encoding='utf-8'))
              for path in sorted(output.glob('ux_*.json'))}
    assert list(stages) == ['own_menu', 'direct_reasons', 'preset_confirm', 'cancelled',
                            'guest_context', 'accepted', 'custom_floor', 'uncertain', 'duplicate'], stages
    assert len(stages) == complete['stageCount'] == 9
    assert [stage['reportCount'] for stage in stages.values()] == [0, 0, 0, 0, 0, 1, 1, 2, 3], stages
    assert all(stages[name]['open'] for name in ('preset_confirm', 'accepted', 'custom_floor', 'uncertain', 'duplicate'))
    assert complete['reportCount'] == 3
    payloads = [call['payload'] for call in complete['calls']]
    assert all(row['threadId'] == 'R_SO_4_123' and 'userid' not in row for row in payloads), payloads
    assert [(row['commentId'], row['reason']) for row in payloads] == [
        ('102', '段子或者无意义的评论'),
        ('201', ' \u3000合成楼层举报 100%\n末行 \t'),
        ('202', '政治敏感言论'),
    ], payloads
    assert [call['lostResponse'] for call in complete['calls']] == [False, True, False]
    pictures = len(list(output.glob('ux_*.png')))
    assert not args.screenshots or pictures == len(stages)
    result = {'actualWebView': True, 'screenshots': pictures, 'states': list(stages), **complete}
    (root / 'report.json').write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n',
                                    encoding='utf-8')
    print(json.dumps(result, ensure_ascii=False))


if __name__ == '__main__':
    main()
