"""Real WebView cloud capacity status from the native quota and membership cache."""

import argparse
import json
import os
from pathlib import Path

from window_manager import launch

STAGES = (
    'unknown_member', 'unknown_member_full', 'active', 'active_full', 'expiring',
    'positive_expiry', 'non_plus_full', 'known_zero_expiry', 'filtered_keeps_status',
)


def verify(output, screenshots):
    def read(name):
        return json.loads((output / name).read_text(encoding='utf-8'))

    result = read('complete.json')
    assert result['ok'] and result['stageCount'] == len(STAGES)
    assert result['realMusicRequests'] == result['realAccountWrites'] == 0
    assert len(list(output.glob('ux_*.json'))) == len(STAGES)
    if screenshots:
        assert len(list(output.glob('ux_*.png'))) == len(STAGES)
    stages = [read(f'ux_{index:02d}_{name}.json') for index, name in enumerate(STAGES, 1)]
    assert all(state['status'] == state['nativeStatus'] for state in stages)
    assert [state['status']['kind'] if state['status'] else None for state in stages] == [
        None, 'full', 'svip_active', 'full', 'svip_expiring', None, 'full', 'svip_offer', 'svip_offer',
    ]
    assert stages[-1]['visibleRows'] == 0
    assert all(state['fullLibraryCount'] == 2 for state in stages)
    records = [state['syntheticCloud'] for state in [*stages, result]]
    assert [record['capacityMembershipReads'] for record in records] == [
        [], [], [1], [1], [1, 2], [1, 2, 3], [1, 2, 3], [1, 2, 3, 4], [1, 2, 3, 4], [1, 2, 3, 4],
    ]
    assert all(record['capacityVersionReads'] == 0 for record in records)
    assert all(record['writes'] == record['collectWrites'] == [] for record in records)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path, required=True)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(
        CUMULUS_PROOF_DIR=str(root), CUMULUS_CLOUD_UI_PROOF='1', CUMULUS_CLOUD_CAPACITY_PROOF='1',
        CUMULUS_PROOF_FRONTEND_DIST=str(args.frontend_dist.resolve(strict=True)),
        CUMULUS_MINI_SCRIPT=str(Path(__file__).resolve().parents[2] / 'desktop/examples/cloud_capacity_probe.js'),
    )
    for key, name in [
        ('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'), ('XDG_CACHE_HOME', 'cache'),
        ('XDG_RUNTIME_DIR', 'runtime'), ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local'),
    ]:
        path = root / 'os-profile' / name
        path.mkdir(parents=True, mode=0o700)
        env[key] = str(path)
    output = root / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(strict=True), env, log, output, args.screenshots)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'cloud capacity probe exit {code}'
    result = verify(output, args.screenshots)
    result['actualWebView'] = True
    (root / 'report.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({k: result[k] for k in ('ok', 'stageCount', 'syntheticCloud')}))


if __name__ == '__main__':
    main()
