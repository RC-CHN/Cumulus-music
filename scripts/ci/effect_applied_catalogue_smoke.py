"""Drive the actual old/new catalogue across one ordinary native PEQ registration."""
import argparse
from collections import Counter
import json
import os
from pathlib import Path

from window_manager import launch


STATES = [
    ('01_viewed_only', None, False, 1),
    ('02_other_viewed', 'configured', True, 1),
    ('03_reopened', 'configured', True, 1),
    ('04_offline', 'configured', True, 1),
    ('05_source_bypass', 'bypassed_for_source', False, 2),
    ('06_source_restored', 'configured', True, 3),
    ('07_stopped', None, False, 3),
    ('08_restored_idle', 'configured', False, 3),
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
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1', CUMULUS_EFFECT_PEQ_NATIVE_PROOF='1',
               CUMULUS_EFFECT_PEQ_SOURCE_BYPASS_PROOF='1', CUMULUS_EFFECT_APPLIED_CATALOGUE_PROOF='1',
               CUMULUS_AUDIO_FILTER=str(args.filter.resolve(strict=True)),
               CUMULUS_UX_SCRIPT=str(repo / 'desktop/examples/effect_applied_catalogue_ui_probe.js'),
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
    for name, binding, processing, loads in STATES:
        row = states[name] = read(output / f'ux_{name}.json')
        assert row['layout'] == layout
        player, native = row['nativePlayer'], row['nativePeq']
        assert player.get('boundParametric') == binding
        assert player.get('appliedEffectId') == (300 if binding else None)
        assert player['parametricProcessing'] is processing and native['decoderLoads'] == loads
        count = 0 if not binding or name == '04_offline' else (1 if args.legacy else 2)
        assert row['markers'] == [[300, binding]] * count
        assert native['starts'] == (0 if name == '01_viewed_only' else 1) and native['stops'] == 0
        assert native['failedBlocks'] == 0
    before = states['01_viewed_only']
    other = states['02_other_viewed']
    reopened = states['03_reopened']
    offline = states['04_offline']
    dry = states['05_source_bypass']
    wet = states['06_source_restored']
    stopped = states['07_stopped']
    resumed = states['08_restored_idle']
    assert before['viewed'] == [300] and other['viewed'] == [301] and reopened['viewed'] == []
    assert stopped['viewed'] == [300] and resumed['nativePlayer']['state'] == 'idle'
    assert len({state['nativePlayer']['playId'] for state in [before, other, reopened, offline]}) == 1
    assert offline['nativePeq']['calls'] == reopened['nativePeq']['calls']
    assert dry['nativePlayer']['actualQuality'] == 'vivid' and wet['nativePlayer']['actualQuality'] == 'dolby'
    assert dry['nativePeq']['bypasses'] == wet['nativePeq']['bypasses'] == 1
    assert dry['nativePeq']['nativeAttached'] and not stopped['nativePeq']['nativeAttached']
    assert resumed['nativePeq']['nativeAttached'] and resumed['nativePeq']['materialGets'] == 1
    assert resumed['nativePeq']['calls'] == done['nativePeq']['calls']
    calls = [row['path'] for row in done['nativePeq']['calls']]
    old = ['/api/usertool/sound/mobile/promote', '/api/usertool/sound/mobile/all']
    new = '/api/usertool/sound/queryAllSound'
    catalogue = [call for call in calls if call in old or call == new]
    assert catalogue == (old * 3 if args.legacy else [new] * 9)
    assert Counter(calls)['/api/vipauth/app/interests/auth/set'] == 1
    assert Counter(calls)['/api/vipauth/app/interests/auth/stop'] == 0
    assert len(list(output.glob('ux_*.png'))) == (len(STATES) if args.screenshots else 0)
    assert read(output / 'locale.json') == {'numeric': 'C', 'messages': 'en_US.UTF-8', 'characters': 'en_US.UTF-8'}
    report = {'ok': True, 'layout': layout, 'actualWebView': True, 'realLibmpvNull': True,
              'stages': list(states), 'samePlaybackWhileViewing': True,
              'syntheticRegistrationWrites': 1, 'syntheticStopWrites': 0, 'realAccountWrites': 0}
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
