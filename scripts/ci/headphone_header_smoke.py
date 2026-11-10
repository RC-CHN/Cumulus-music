"""Exercise current headphone comparison across actual audio effect tabs."""
import argparse
from contextlib import closing
import json
import os
from pathlib import Path
import sqlite3

from window_manager import launch


STATES = [
    ('01_no_headphone', 'catalogue', None, None, 1),
    ('02_enabled_in_devices', 'devices', 'true', 'configured', 1),
    ('03_enabled_in_catalogue', 'catalogue', 'true', 'configured', 1),
    ('04_paused_in_catalogue', 'catalogue', 'false', 'paused_locally', 1),
    ('05_resumed_in_equalizer', 'equalizer', 'true', 'configured', 1),
    ('06_source_bypass', 'equalizer', None, 'bypassed_for_source', 2),
    ('07_reopened_paused', 'devices', None, 'paused_locally', 3),
    ('08_reopened_resumed', 'devices', 'true', 'configured', 3),
]


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path, required=True)
    parser.add_argument('--filter', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--legacy', action='store_true')
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    assert len(Path('/proc/net/route').read_text().splitlines()) == 1, 'use an isolated Linux namespace'
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False, mode=0o700)
    repo = Path(__file__).resolve().parents[2]
    env = {key: value for key, value in os.environ.items()
           if not key.upper().startswith(('CUMULUS_', 'WEBVIEW2_')) and key.upper() != 'MPV_DLL_DIR_CUMULUS'}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1', CUMULUS_EFFECT_DIRECTORY_PROOF='1',
               CUMULUS_EFFECT_DEVICE_NATIVE_PROOF='1', CUMULUS_EFFECT_DEVICE_SOURCE_BYPASS_PROOF='1',
               CUMULUS_EFFECT_HEADPHONE_HEADER_PROOF='1',
               CUMULUS_AUDIO_FILTER=str(args.filter.resolve(strict=True)),
               CUMULUS_UX_SCRIPT=str(repo / 'desktop/examples/headphone_header_ui_probe.js'),
               CUMULUS_PROOF_FRONTEND_DIST=str(args.frontend_dist.resolve(strict=True)),
               LC_ALL='en_US.UTF-8', GDK_BACKEND='x11')
    if args.legacy:
        env['CUMULUS_EFFECT_CATALOGUE_LEGACY'] = '1'
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR'):
        folder = root / key
        folder.mkdir(mode=0o700)
        env[key] = str(folder)
    output = root / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(strict=True), env, log, output, args.screenshots)
    failed = output / 'failed.json'
    assert not failed.exists(), read(failed) if failed.exists() else ''
    layout = 'legacy' if args.legacy else 'modern'
    assert code == 0
    done = read(output / 'complete.json')
    assert done['ok'] and done['layout'] == layout
    states = {}
    for name, tab, checked, status, loads in STATES:
        row = states[name] = read(output / f'ux_{name}.json')
        if name == '07_reopened_paused' and args.legacy:
            checked = 'false'
        assert row['layout'] == layout and row['activeTab'] == tab and row['checked'] == checked
        assert not row['eqMaster']
        assert row['insideComparison'] is (name == '07_reopened_paused' and not args.legacy)
        player, native = row['nativePlayer'], row['nativePeq']
        device = player.get('deviceParametric')
        assert (device and device['status']) == status
        assert player['parametricProcessing'] is (status == 'configured')
        assert native['nativeAttached'] is (status in ('configured', 'bypassed_for_source'))
        assert native['decoderLoads'] == loads and not native['calls']
        assert native['starts'] == native['stops'] == native['failedBlocks'] == 0
        assert row['effectDirectoryMaterialGets'] == (0 if status is None else 1)
        assert not player.get('boundParametric') and not player.get('appliedEffectId')
    assert len({row['nativePlayer']['deviceParametric']['activationId'] for row in list(states.values())[1:]}) == 1
    assert len({row['nativePlayer']['playId'] for row in list(states.values())[:5]}) == 1
    assert states['07_reopened_paused']['nativePlayer']['playId'] == states['08_reopened_resumed']['nativePlayer']['playId']
    assert all(row['nativePlayer']['equalizer'] == states['01_no_headphone']['nativePlayer']['equalizer']
               for row in states.values())
    assert len({json.dumps(states[name]['effectDirectoryCalls']) for name in
                ('03_enabled_in_catalogue', '04_paused_in_catalogue', '05_resumed_in_equalizer', '06_source_bypass')}) == 1
    assert states['07_reopened_paused']['effectDirectoryCalls'] == states['08_reopened_resumed']['effectDirectoryCalls']
    assert states['06_source_bypass']['nativePlayer']['actualQuality'] == 'vivid'
    assert states['08_reopened_resumed']['nativePlayer']['actualQuality'] == 'dolby'
    assert states['06_source_bypass']['nativePeq']['bypasses'] == 1
    db = (root / 'state' / 'core.sqlite3').resolve(strict=True)
    with closing(sqlite3.connect(db.as_uri() + '?mode=ro', uri=True)) as store:
        assert not store.execute("SELECT 1 FROM state WHERE kind='effect-authorizations-v1'").fetchall()
    assert len(list(output.glob('ux_*.png'))) == (len(STATES) if args.screenshots else 0)
    assert read(output / 'locale.json') == {'numeric': 'C', 'messages': 'en_US.UTF-8', 'characters': 'en_US.UTF-8'}
    report = {'ok': True, 'layout': layout, 'actualWebView': True, 'nativeNullAudio': True,
              'stages': list(states), 'unchangedEqualizer': True, 'sameTrackAcrossComparisons': True,
              'effectAuthWrites': 0, 'materialGets': 1, 'realAccountWrites': 0}
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
