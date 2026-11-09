"""Real Linux WebView and libmpv, with a caller-owned synthetic local library."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import struct
import sys

from motion_smoke import launch


STAGES = {
    'ux_01_batch_menu', 'ux_02_batch_cross_page', 'ux_03_batch_hidden', 'ux_04_batch_restored',
}


def run(binary, root, screenshots):
    root.mkdir(parents=True, exist_ok=False)
    target = root / 'run'
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(target), CUMULUS_LOCAL_BATCH_HIDE_PROOF='1')
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR'):
        profile = root / 'os-profile' / key
        profile.mkdir(parents=True, mode=0o700)
        env[key] = str(profile)
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(binary, env, log, target, screenshots)
    failed = target / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0
    completed = json.loads((target / 'complete.json').read_text(encoding='utf-8'))
    assert completed['ok'] and completed['counts'] == [105, 103, 105]
    assert len(set(completed['selected'])) == 2
    assert completed['pausedDecoderUnchanged'] and completed['retainedFilterOnBack']
    assert completed['restoredMenuFocus']
    assert completed['realMusicRequests'] == 0
    locale = json.loads((target / 'native-startup.json').read_text(encoding='utf-8'))
    assert locale['afterGuiInitialization'] and locale['numericLocale'] == 'C'
    assert locale['messageLocale'] == env['LC_ALL'] and env['LC_ALL'] not in ('C', 'POSIX')

    media = target / 'media'
    expected = bytearray((media / '雨声.wav').read_bytes()[:44 + 16_000])
    struct.pack_into('<I', expected, 4, 16_036)
    struct.pack_into('<I', expected, 40, 16_000)
    digest = hashlib.sha256(expected).digest()
    files = sorted(media.glob('qx-local-selection *.wav'))
    assert [p.name for p in files] == [f'qx-local-selection {i:03d}.wav' for i in range(105)]
    assert all(hashlib.sha256(path.read_bytes()).digest() == digest for path in files)
    stages = {path.stem for path in target.glob('ux_*.png')}
    assert stages == (STAGES if screenshots else set())
    report = {
        'ok': True, 'actualWebView': True, 'nativeNullOutput': True,
        'counts': completed['counts'], 'sourceFilesIntact': len(files),
        'pausedDecoderUnchanged': True, 'retainedFilterOnBack': True, 'restoredMenuFocus': True,
        'realMusicRequests': 0, 'screenshots': len(stages), 'locale': locale,
    }
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    if sys.platform != 'linux':
        parser.error('this probe uses the Linux WebKit and X11 capture helper')
    run(args.binary.resolve(strict=True), args.root.resolve(), args.screenshots)
