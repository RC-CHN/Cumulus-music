"""Real cloud-list filtering without any upstream request or fixture mutation."""
import argparse
import json
import os
from pathlib import Path
import sys
from window_manager import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    if sys.platform == 'linux' and len(Path('/proc/net/route').read_text().splitlines()) != 1:
        parser.error('cloud filter proof requires loopback_only.sh on Linux')
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/cloud_filter_probe.js'
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_CLOUD_FILTER_UI_PROOF='1',
               CUMULUS_MINI_SCRIPT=str(source))
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        directory = root / 'os-profile' / name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve(strict=True)) + os.pathsep + env.get('PATH', '')
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    first = root / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        result = launch(args.binary.resolve(strict=True), env, log, first, args.screenshots)
    failed = first / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert result == 0, f'cloud filter probe exit {result}'

    def read(path):
        return json.loads(path.read_text(encoding='utf-8'))

    complete = read(first / 'complete.json')
    all_ids = [str(id) for id in range(1008, 1000, -1)]
    expected = [
        ('initial', '', all_ids), ('width_literal', 'Ａ＋Ｂ', ['1001']),
        ('composing', '樂', ['1002', '1001']), ('translation', '台湾 录音', ['1003']),
        ('filename_excluded', '文件独有', []), ('cleared', '', all_ids),
    ]
    stages = [f'ux_{i:02}_cloud_filter_{label}' for i, (label, _, _) in enumerate(expected, 1)]
    assert complete['ok'] and complete['count'] == len(stages)
    assert sorted(path.stem for path in first.glob('ux_*.json')) == stages
    assert sorted(path.stem for path in first.glob('ux_*.png')) == (stages if args.screenshots else [])
    for name, (label, keyword, ids) in zip(stages, expected, strict=True):
        row = read(first / (name + '.json'))
        assert (row['label'], row['keyword'], row['ids']) == ('cloud_filter_' + label, keyword, ids), row
        if label not in ('initial', 'cleared'):
            assert row['inputFocused'], label
        assert row['syntheticCloud']['filterReads'] == [{'offset': 0, 'limit': 200}]
        assert row['nativePlayer']['queueRevision'] == complete['nativePlayer']['queueRevision']
    cloud = complete['syntheticCloud']
    assert cloud['filterReads'] == [{'offset': 0, 'limit': 200}]
    assert cloud['realRequests'] == 0 and not any(cloud[key] for key in ('writes', 'collectWrites', 'downloadAuths'))
    report = dict(ok=True, actualWebView=True, nativeIpc=True, stages=stages, catalogReads=cloud['filterReads'],
                  noReadsWhileTyping=True, noFixtureWrites=True, realAccountWrites=0)
    (root / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    main()
