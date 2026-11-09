"""Capture the no-network signed account/search/pinned AAC desktop proof."""

import argparse
import hashlib
import json
import os
from pathlib import Path

from window_manager import launch


STAGES = ('account_resources', 'search_song', 'playing', 'paused', 'resumed', 'stopped')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(mode=0o700)
    # Never inherit proxies, any real desktop account data, or asset overrides.
    env = {name: os.environ[name] for name in (
        'PATH', 'DISPLAY', 'XAUTHORITY', 'DBUS_SESSION_BUS_ADDRESS', 'LC_ALL', 'GDK_BACKEND',
        'LIBGL_ALWAYS_SOFTWARE', 'WEBKIT_FORCE_DMABUF_RENDERER',
        'WEBKIT_DMABUF_RENDERER_FORCE_SHM',
    ) if name in os.environ}
    for name in ('HOME', 'XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR'):
        path = root / name.lower()
        path.mkdir(mode=0o700)
        env[name] = str(path)
    env['CUMULUS_PROOF_DIR'] = str(root / 'run')
    output = root / 'run/first'
    binary = args.binary.resolve(strict=True)
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        status = launch(binary, env, log, output, args.screenshots, argv=('--synthetic',))
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert status == 0
    stages = {name: json.loads((output / f'ux_{name}.json').read_text(encoding='utf-8'))
              for name in STAGES}
    assert len(list(output.glob('ux_*.json'))) == len(STAGES)
    receipt = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
    assert all(stage['ok'] and stage['assets'] == receipt['assets'] for stage in stages.values())
    assert receipt['ok'] and receipt['closedNetwork'] and receipt['syntheticOnly']
    assert receipt['mediaReads'] == receipt['anonymousPinnedLoads'] == 1
    assert receipt['observedLibmpvNullClock'] and receipt['gate']['searchV3']
    assert stages['playing']['clock'] > 0.25
    assert stages['resumed']['clock'] > stages['paused']['clock'] + 0.15
    screenshots = len(list(output.glob('ux_*.png')))
    assert screenshots == (len(STAGES) if args.screenshots else 0)
    with binary.open('rb') as stream:
        receipt['binarySha256'] = hashlib.file_digest(stream, 'sha256').hexdigest()
    receipt['stages'] = list(STAGES)
    receipt['screenshots'] = screenshots
    (root / 'report.json').write_text(json.dumps(receipt, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(receipt))


if __name__ == '__main__':
    main()
