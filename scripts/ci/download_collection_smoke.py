"""Real Downloads collection over completed records and a sealed playlist transport."""

import argparse
import json
import os
from pathlib import Path

from window_manager import launch


STAGES = (
    'ordinary_picker', 'canceled_before_write', 'ordinary_confirmed',
    'private_hidden', 'legacy_hidden', 'matched_uncertain', 'matched_reconciled',
)


def verify(output, screenshots):
    def read(name):
        return json.loads((output / name).read_text(encoding='utf-8'))

    result = read('complete.json')
    assert result['ok'] and result['stageCount'] == len(STAGES)
    assert result['realMusicRequests'] == result['realAccountWrites'] == 0
    assert result['nativeInventory'] == [
        {'id': str(id_), 'source': 'song' if id_ <= 4 else 'cloud'}
        for id_ in range(1, 8)
    ]
    assert len(list(output.glob('ux_*.json'))) == len(STAGES)
    if screenshots:
        assert len(list(output.glob('ux_*.png'))) == len(STAGES)
    rows = [read(f'ux_{i:02d}_{stage}.json') for i, stage in enumerate(STAGES, 1)]
    report = [row['syntheticCollection'] for row in [*rows, result]]
    assert [value['writes'] for value in report] == [0, 0, 1, 1, 1, 2, 2, 2]
    assert all(value['creates'] == 0 for value in report)
    assert rows[0]['song'] == '1' and rows[3]['song'] == '6' and rows[4]['song'] == '7'
    assert rows[1]['exactReturnFocus']
    assert rows[5]['song'] == '5'
    expected = [
        {'pid': '2', 'trackIds': '["1"]', 'op': 'add'},
        {'pid': '2', 'trackIds': '["9010"]', 'op': 'add'},
    ]
    assert all(value['inlineAddPayloads'] == expected[:value['writes']] for value in report)
    assert all(value['sortedPages'] for value in report)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path, required=True)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    if os.name != 'posix':
        parser.error('this proof runner currently targets Linux GTK/WebKit')
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(
        CUMULUS_PROOF_DIR=str(root),
        CUMULUS_PROOF_FRONTEND_DIST=str(args.frontend_dist.resolve(strict=True)),
        CUMULUS_UX_SCRIPT=str(Path(__file__).resolve().parents[2] /
                              'desktop/examples/download_collection_ui_probe.js'),
        CUMULUS_MOTION_PROOF='1',
        CUMULUS_DOWNLOAD_PAGE_UI_PROOF='1',
        CUMULUS_DOWNLOAD_COLLECTION_PROOF='1',
        CUMULUS_PLAYLIST_COLLECT_PROOF='1',
    )
    for key, name in (
        ('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'), ('XDG_CACHE_HOME', 'cache'),
        ('XDG_RUNTIME_DIR', 'runtime'),
    ):
        directory = root / 'os-profile' / name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    output = root / 'first'
    with (root / 'first.log').open('x', encoding='utf-8') as log:
        status = launch(args.binary.resolve(strict=True), env, log, output, args.screenshots)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert status == 0, f'probe exit {status}'
    result = verify(output, args.screenshots)
    result.update(actualWebView=True, windowsVisualVerified=False)
    (root / 'audit.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({key: result[key] for key in ('ok', 'stageCount', 'syntheticCollection')}))


if __name__ == '__main__':
    main()
