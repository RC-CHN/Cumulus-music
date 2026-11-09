"""Check a sealed local batch Favorite against the actual Linux WebView."""
import argparse
import json
import os
from pathlib import Path
import sys

from motion_smoke import launch


STAGES = ('ux_01_local_favorite_picker', 'ux_02_local_favorite_pending',
          'ux_03_local_favorite_confirmed', 'ux_04_local_favorite_duplicate')


def run(binary, root, screenshots, script=None):
    root.mkdir(parents=True, exist_ok=False)
    target = root / 'run'
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env['CUMULUS_PROOF_DIR'] = str(target)
    if script is not None:
        env['CUMULUS_LOCAL_FAVORITE_SCRIPT'] = str(script)
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR'):
        directory = root / key
        directory.mkdir(mode=0o700)
        env[key] = str(directory)
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(binary, env, log, target, screenshots)

    def receipt(stage):
        return json.loads((target / f'{stage}.json').read_text(encoding='utf-8'))

    assert not (target / 'failed.json').exists(), receipt('failed') if (target / 'failed.json').exists() else ''
    assert code == 0
    first, pending, confirmed, duplicate = map(receipt, STAGES)
    wire = {'pid': '81', 'trackIds': '["802","801"]', 'op': 'add'}
    assert first['writes'] == []
    assert pending['writes'] == confirmed['writes'] == [wire]
    assert duplicate['writes'] == [wire, wire]
    assert [len(row['collectionReads']) for row in (first, pending, confirmed, duplicate)] == [1, 4, 5, 7]
    complete = receipt('complete')
    assert complete['ok'] and complete['paused'] == 'paused' and complete['position'] > 0
    assert complete['writes'] == duplicate['writes']
    locale = receipt('locale')
    assert locale == {'numericLocale': 'C', 'messageLocale': env['LC_ALL']}
    images = {path.stem for path in target.glob('ux_*.png')}
    assert images == (set(STAGES) if screenshots else set())
    summary = {'ok': True, 'stageCount': len(STAGES), 'screenshots': len(images),
               'nativeOrder': [802, 801], 'writes': len(complete['writes']), 'locale': locale}
    (root / 'report.json').write_text(json.dumps(summary, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(summary))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--script', type=Path)
    args = parser.parse_args()
    if sys.platform != 'linux':
        parser.error('requires the Linux WebKit and X11 capture helper')
    run(args.binary.resolve(strict=True), args.root.resolve(), args.screenshots,
        args.script.resolve(strict=True) if args.script else None)
