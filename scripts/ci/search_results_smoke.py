"""Search result categories and interactions via production IPC and an isolated in-memory backend."""
import argparse
import json
import os
from pathlib import Path
from motion_smoke import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--v3', action='store_true')
    mode.add_argument('--voice-direct', action='store_true')
    mode.add_argument('--card-matches', action='store_true')
    mode.add_argument('--correction-original', action='store_true')
    mode.add_argument('--people-categories', action='store_true')
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1',
               CUMULUS_SEARCH_UI_PROOF='1')
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        directory = root / 'os-profile' / name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/search_results_ui_probe.js'
    script = root / 'search-results.js'
    prefix = ''
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if args.people_categories:
        prefix = 'window.__cumulusSearchPeopleCategoriesProof = true;\n'
    if args.correction_original:
        prefix = 'window.__cumulusSearchOriginalCorrectionProof = true;\n'
    if args.card_matches:
        prefix = 'window.__cumulusSearchCardMatchesProof = true;\n'
    if args.voice_direct:
        prefix = 'window.__cumulusSearchVoiceDirectProof = true;\n'
    if args.v3:
        env['CUMULUS_SEARCH_V3_PROOF'] = '1'
        prefix = 'window.__cumulusSearchV3Proof = true;\n'
    script.write_text(prefix + source.read_text(encoding='utf-8'), encoding='utf-8')
    env['CUMULUS_UX_SCRIPT'] = str(script)
    env['CUMULUS_SEARCH_USER_PROFILE_PROOF'] = '1'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, root / 'first', args.screenshots)
    failed = root / 'first/failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'probe exited {code}'
    if args.voice_direct or args.card_matches or args.correction_original or args.people_categories:
        complete = json.loads((root / 'first/complete.json').read_text(encoding='utf-8'))
        flag = ('peopleCategoriesVerified' if args.people_categories else
                'originalCorrectionVerified' if args.correction_original else
                'cardTitleMatchesVerified' if args.card_matches else 'voiceDirectVerified')
        assert complete['ok'] and complete[flag] and complete['stageCount'] == 2
        if args.voice_direct:
            assert complete['preparedTicketFenceVerified']
        assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
        assert len(list((root / 'first').glob('ux_*.png'))) == (2 if args.screenshots else 0)
        print(json.dumps(dict(complete, actualWebView=True)))
        return
    verify(root, args.screenshots, args.v3)

def verify(root, capture, v3=False):
    complete = json.loads((root / 'first/complete.json').read_text(encoding='utf-8'))
    if v3:
        assert complete['ok'] and complete['v3Verified'] and complete['stageCount'] == 3
        assert complete['syntheticV3Calls'] == 3 and complete['syntheticSearchCalls'] >= 4
        assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
        assert len(list((root / 'first').glob('ux_*.json'))) == 3
        assert len(list((root / 'first').glob('ux_*.png'))) == (3 if capture else 0)
        (root / 'audit.json').write_text(json.dumps(dict(complete, actualWebView=True), indent=2), encoding='utf-8')
        print(json.dumps(complete))
        return
    assert complete['ok'] and complete['responseTimingInjected'] and complete['shortPageOffsetsVerified']
    assert complete['searchResultPlaybackVerified'] and complete['searchMatchHighlightVerified']
    assert complete['searchCategoryWithoutFramesVerified'] and complete['searchServiceEmptyVerified']
    before_profile = json.loads((root / 'first/ux_01_all_1120_light.json').read_text(encoding='utf-8'))
    assert before_profile['searchProfileCalls'] == []
    assert complete['searchProfileWithoutFramesVerified']
    assert complete['searchProfileCalls'] == [
        {'all': True, 'userId': '501'},
        {'uid': '501', 'offset': 0, 'limit': 1000},
    ]
    assert complete['stageCount'] == 69 and complete['syntheticSearchCalls'] > 40
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    assert len(list((root / 'first').glob('ux_*.json'))) == complete['stageCount']
    if capture:
        assert len(list((root / 'first').glob('ux_*.png'))) == complete['stageCount']
    result = dict(complete, actualWebView=True,
                  screenshots=complete['stageCount'] if capture else 0,
                  physicalDisplayVerified=False, windowsVisualVerified=False)
    (root / 'audit.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({k: result[k] for k in ['ok', 'stageCount', 'screenshots', 'syntheticSearchCalls']}))


if __name__ == '__main__':
    main()
