"""Exercise the actual audio effects modal, offline routing, and paused decoder."""
import argparse
import json
import os
from pathlib import Path
import sys

from window_manager import launch

STAGES = {
    'offline_default': ('catalogue', False, 0),
    'offline_equalizer': ('equalizer', False, 0),
    'online_catalogue': ('catalogue', True, 1),
    'device_brands': ('devices', True, 2),
    'device_models': ('devices', True, 3),
    'online_equalizer': ('equalizer', True, 3),
    'menu_default': ('catalogue', True, 4),
}
CATALOGUE = {'path': '/api/usertool/sound/queryAllSound', 'args': {'exposeType': 5}}
WIRE = [
    CATALOGUE,
    {'path': '/api/usertool/home/soundeffect/devicebrand/list', 'args': {}},
    {'path': '/api/usertool/home/soundeffect/devicesound/list', 'args': {'deviceManagerId': 4100}},
    CATALOGUE,
]


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    if sys.platform != 'linux':
        parser.error('this real WebView recipe uses the sealed Linux X11 environment')
    assert len(Path('/proc/net/route').read_text().splitlines()) == 1, 'use a network namespace without routes'
    repo = Path(__file__).resolve().parents[2]
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False, mode=0o700)
    env = {key: value for key, value in os.environ.items()
           if not key.upper().startswith(('CUMULUS_', 'WEBVIEW2_')) and key.upper() != 'MPV_DLL_DIR_CUMULUS'}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1', CUMULUS_EFFECT_DIRECTORY_PROOF='1',
               CUMULUS_AUDIO_EFFECT_TABS_PROOF='1',
               CUMULUS_UX_SCRIPT=str(repo / 'desktop/examples/audio_effect_tabs_ui_probe.js'),
               CUMULUS_PROOF_FRONTEND_DIST=str(args.frontend_dist.resolve(strict=True)),
               LC_ALL='en_US.UTF-8', GDK_BACKEND='x11')
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR'):
        folder = root / key
        folder.mkdir(mode=0o700)
        env[key] = str(folder)
    out = root / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(strict=True), env, log, out, args.screenshots)
    failed = out / 'failed.json'
    assert not failed.exists(), read(failed) if failed.exists() else None
    done = read(out / 'complete.json')
    assert code == 0 and done['ok'] and done['stageCount'] == len(STAGES)
    assert done['closedFocus'] == '当前歌曲更多操作'
    rows = {path.stem.split('_', 2)[2]: read(path) for path in sorted(out.glob('ux_*.json'))}
    assert tuple(rows) == tuple(STAGES)
    identities, positions, revisions, queue_revisions = set(), set(), set(), set()
    for name, row in rows.items():
        tab, online, calls = STAGES[name]
        assert row['active'] == tab and row['online'] is online
        assert row['connectedTabs'] == ['音效', '设备适配', '均衡器']
        assert row['effectDirectoryCalls'] == WIRE[:calls] and row['effectDirectoryMaterialGets'] == 0
        assert row['audioFactoryCount'] == 1
        player, queue = row['nativePlayer'], row['nativeQueue']
        assert player['state'] == 'paused' and player['error'] is None and not player['nativeSeeking']
        assert player['track']['id'] == '1' and player['resourceKind'] == 'catalog'
        assert queue['current'] == player['entryId'] and queue['tracks']
        assert player['playId'] and player['entryId']
        assert player['position'] == row['observedPosition'] and player['position'] > .25
        positions.add(player['position'])
        identities.add((player['epoch'], player['playId'], player['entryId']))
        revisions.add(player['equalizer']['revision'])
        queue_revisions.add(player['queueRevision'])
    assert len(identities) == len(positions) == len(revisions) == len(queue_revisions) == 1
    assert done['effectDirectoryCalls'] == WIRE and done['effectDirectoryMaterialGets'] == 0
    assert done['nativePlayer']['position'] == done['observedPosition'] == next(iter(positions))
    assert done['nativePlayer']['state'] == 'paused'
    assert read(out / 'locale.json') == {'numeric': 'C', 'messages': 'en_US.UTF-8', 'characters': 'en_US.UTF-8'}
    screenshots = len(list(out.glob('ux_*.png')))
    assert screenshots == (len(STAGES) if args.screenshots else 0)
    report = {'ok': True, 'actualWebView': True, 'actualProductionUiAndIpc': True, 'platform': 'linux',
              'nativeNullAudio': True, 'sealedSyntheticAccount': True, 'stages': list(STAGES),
              'screenshots': screenshots, 'pausedPosition': next(iter(positions)), 'sameNativeIdentity': True,
              'browserOfflineStateSimulated': True, 'wire': WIRE, 'materialGets': 0, 'realAccountWrites': 0}
    (root / 'report.json').write_text(json.dumps(report, indent=2, ensure_ascii=False) + '\n', encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    main()
