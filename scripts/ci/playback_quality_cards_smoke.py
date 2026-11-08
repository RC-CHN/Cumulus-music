"""Real Linux WebView quality layouts with a sealed signed PC assignment."""
import argparse
import json
import os
from pathlib import Path
import sys

from window_manager import launch

STAGES = {
    'layout_vivid': ('paired', 'right_card_focused', 'singleton'),
    'layout_only': ('unobserved_legacy', 'paired', 'right_card_focused', 'singleton', 'trial_consent'),
    'control': ('signed_control',),
}
ASSIGNMENT = '/api/rtrs/abt/front/expinfo/list'
ACCOUNT = '/api/w/nuser/account/get'


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--case', choices=STAGES, required=True)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    if sys.platform != 'linux':
        parser.error('this native receipt uses Linux WebKit and X11')
    assert len(Path('/proc/net/route').read_text().splitlines()) == 1, 'use a sealed network namespace'
    repo = Path(__file__).resolve().parents[2]
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False, mode=0o700)
    script = root / 'scenario.js'
    source = repo / 'desktop/examples/playback_quality_cards_ui_probe.js'
    script.write_text(f'window.__cumulusQualityCardsCase={json.dumps(args.case)};\n' +
                      source.read_text(encoding='utf-8'), encoding='utf-8')
    env = {key: value for key, value in os.environ.items()
           if not key.upper().startswith(('CUMULUS_', 'WEBVIEW2_')) and key.upper() != 'MPV_DLL_DIR_CUMULUS'}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1', CUMULUS_QUALITY_UI_PROOF='1',
               CUMULUS_QUALITY_CARDS_PROOF=args.case, CUMULUS_UX_SCRIPT=str(script),
               CUMULUS_PROOF_FRONTEND_DIST=str(args.frontend_dist.resolve(strict=True)),
               LC_ALL='en_US.UTF-8', GDK_BACKEND='x11')
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR'):
        folder = root / key
        folder.mkdir(mode=0o700)
        env[key] = str(folder)
    out = root / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        status = launch(args.binary.resolve(strict=True), env, log, out, args.screenshots)
    failed = out / 'failed.json'
    assert not failed.exists(), read(failed) if failed.exists() else None
    done = read(out / 'complete.json')
    stages = STAGES[args.case]
    assert status == 0 and done['ok'] and done['stageCount'] == len(stages) and done['closedFocus']
    rows = {path.stem.split('_', 2)[2]: read(path) for path in sorted(out.glob('ux_*.json'))}
    assert tuple(rows) == stages
    identities, positions, preferences = set(), set(), set()
    requests = done['syntheticQuality']['requests']
    assert all(item['quality'] == 'standard' for item in requests)
    assert sum(str(item['id']) == '1' for item in requests) == 1
    for name, row in [*rows.items(), ('complete', done)]:
        signed = row['signedSearch']
        assert signed['transport'] == 'MusicHttpClient' and signed['syntheticOnly'] and signed['sameProfileAccountRestore']
        paths = [ACCOUNT] + ([] if name == 'unobserved_legacy' else [ASSIGNMENT])
        assert [call['path'] for call in signed['calls']] == paths
        assert all(call['signedPcContextVerified'] and call['sameAccountCredential'] and call['responseCode'] == 200
                   for call in signed['calls'])
        if len(paths) == 2:
            exp = signed['calls'][1]['args']['expNames'].split(',')
            assert exp.count('PH-PC-hyyinzhi') == exp.count('PH-PC-zhenyingn') == 1
        quality = row['syntheticQuality']
        assert quality['trialWrites'] == 0 and quality['requests'] == requests
        player = row['nativePlayer']
        assert player['state'] == 'paused' and player['actualQuality'] == 'standard' and player['error'] is None
        assert player['playId'] and player['entryId'] and player['track']['id'] == '1'
        assert player['position'] > .25 and not player['nativeSeeking']
        if name != 'complete':
            assert player['position'] == row['observedPosition']
        positions.add(player['position'])
        identities.add((player['epoch'], player['playId'], player['entryId'], player['queueRevision']))
        preferences.add(player['qualityPreferences']['revision'])
    assert len(positions) == len(identities) == len(preferences) == 1
    if args.case == 'control':
        row = rows['signed_control']
        assert not row['paired'] and row['keys'][:3] == ['vivid', 'sky', 'jymaster'] and row['focus'] == 'vivid'
    else:
        for name, focus in [('paired', 'sky'), ('right_card_focused', 'jymaster')]:
            row = rows[name]
            assert row['paired'] and row['keys'][:2] == ['sky', 'jymaster'] and row['focus'] == focus
            assert ('vivid' in row['keys']) is (args.case == 'layout_vivid')
            sky, master = row['geometry']['sky'], row['geometry']['jymaster']
            assert abs(sky['top'] - master['top']) <= 1 and sky['right'] < master['left']
            if args.case == 'layout_vivid':
                assert row['geometry']['vivid']['top'] >= master['bottom']
        row = rows['singleton']
        assert not row['paired'] and row['keys'][0] == row['focus'] == 'jymaster' and 'sky' not in row['keys']
        assert row['geometry']['jymaster']['width'] > rows['paired']['geometry']['jymaster']['width'] * 1.5
        if args.case == 'layout_only':
            assert rows['unobserved_legacy']['keys'][:3] == ['vivid', 'sky', 'jymaster']
            assert not rows['unobserved_legacy']['paired'] and rows['trial_consent']['trialDialog']
    locale = read(out / 'locale.json')
    assert locale == {'numeric': 'C', 'messages': 'en_US.UTF-8', 'characters': 'en_US.UTF-8'}
    screenshots = len(list(out.glob('ux_*.png')))
    assert screenshots == (len(stages) if args.screenshots else 0)
    report = dict(ok=True, actualWebView=True, actualProductionUiAndIpc=True, platform='linux',
                  case=args.case, stages=stages, screenshots=screenshots, locale=locale, pausedNullAudio=True,
                  assignmentReads=1, sameNativeIdentity=True, samePausedPosition=next(iter(positions)),
                  sourceRequests=requests, metadataReads=done['syntheticQuality']['metadataReads'],
                  trialWrites=0, sealedSyntheticAccount=True, realAccountWrites=0)
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
