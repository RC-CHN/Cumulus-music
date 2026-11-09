"""Collection tab/filter/history ownership, read recovery and expiration in production WebViews."""
import argparse
import json
import os
from pathlib import Path
from window_manager import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root/'run'), CUMULUS_COLLECTION_PROOF='1',
               CUMULUS_COLLECTION_CONTEXT_PROOF='1', CUMULUS_UX_SCRIPT=str(
                   Path(__file__).resolve().parents[2]/'desktop/examples/collection_context_ui_probe.js'))
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'), ('XDG_CACHE_HOME', 'cache'),
                      ('XDG_RUNTIME_DIR', 'runtime'), ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        path = root/'os-profile'/name
        path.mkdir(parents=True, mode=0o700)
        env[key] = str(path)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve())+os.pathsep+env.get('PATH', '')
    output = root/'run/first'
    with (root/'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, output, args.screenshots)
    failed = output/'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'collection context process exited {code}'
    complete = json.loads((output/'complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['collectionContext'] and complete['stageCount'] == 44
    states = {value['stage'].split('_', 2)[2]: value for path in output.glob('ux_*.json')
              if (value := json.loads(path.read_text(encoding='utf-8')))}
    assert len(states) == 44
    recovered = states['saved_first_recovered_light']['collectionFixture']
    assert recovered['delayedFirstCompleted'] == 1 and not recovered['pending']
    assert [call['mode'] for call in recovered['calls']] == [1, 10]
    for state in states.values():
        assert state['visible'] and state['focused'] and state['focusValid'] and not state['overflow']
        assert state['cards'] <= 36 and all(control['hit'] for control in state['controls'])
    for before, after in [('saved_context_light', 'saved_return_light'), ('followed_context_dark', 'followed_return_dark')]:
        a, b = states[before], states[after]
        assert a['loaded'] == b['loaded'] and a['keyword'] == b['keyword'] and a['focus'] == b['focus']
        assert abs(a['scrollTop']-b['scrollTop']) < 1
        assert len(a['collectionFixture']['calls']) == len(b['collectionFixture']['calls'])
    assert states['saved_second_visit_light']['keyword'] == ''
    assert states['saved_second_return_light']['keyword'] == '专辑 2'
    assert states['saved_first_visit_return_dark']['keyword'] == '臺灣 Ａ [Live]'
    assert states['purchased_after_late_read_light']['collectionFixture']['delayedCompleted'] == 1
    assert states['saved_more_error_dark']['loaded'] == states['saved_error_return_light']['loaded'] == 100
    assert states['saved_refresh_error_dark']['loaded'] == states['saved_refresh_error_return_dark']['loaded'] == 123
    assert states['saved_refresh_recovered_light']['loaded'] == 50
    compact = states['followed_small_dark']
    assert compact['loaded'] == 20 and compact['focus'] == 'control:refresh'
    assert (compact['width'], compact['height']) == (760, 540)
    assert states['expired_light']['loaded'] is None and states['expired_light']['cards'] == 0
    if args.screenshots:
        assert len(list(output.glob('ux_*.png'))) == 44
    report = dict(ok=True, states=44, screenshots=44 if args.screenshots else 0,
                  historyPrefixVerified=True, sharedAlbumFilterVerified=True, failureRecoveryVerified=True,
                  realMusicRequests=0, realAccountWrites=0, playbackVerified=False)
    (root/'report.json').write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
