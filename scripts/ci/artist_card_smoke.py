"""Artist cards, native source controls, menus and journaled follows in a real WebView."""
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
    env.update(CUMULUS_PROOF_DIR=str(root/'run'), CUMULUS_ARTIST_CARD_PROOF='1',
               CUMULUS_UX_SCRIPT=str(Path(__file__).resolve().parents[2]/'desktop/examples/artist_card_ui_probe.js'))
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
    assert code == 0, f'artist card process exited {code}'
    complete = json.loads((output/'complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['artistCards']
    states = {value['stage'].split('_', 2)[2]: value for path in output.glob('ux_*.json')
              if (value := json.loads(path.read_text(encoding='utf-8')))}
    assert len(states) == complete['stageCount'] == 37
    for state in states.values():
        assert state['visible'] and state['focused'] and state['focusValid'] and not state['overflow']
        assert state['cards'] <= 36 and state['menuContained'] and all(control['hit'] for control in state['controls'])
    first = states['cards_light']['metadata']
    assert first[0]['name'].startswith('长名称') and first[0]['description'] == '单曲：55'
    assert first[1]['fallback'] and first[2]['description'] == '单曲：0'
    oracle = json.loads((Path(__file__).resolve().parents[2]/'frontend/tests/fixtures/official-artist-card-menu.json').read_text(encoding='utf-8'))
    for vector, stage in zip(oracle['vectors'], ['keyboard_menu_dark', 'follow_menu_light'], strict=True):
        assert [key for key in states[stage]['menuKeys'] if key != 'separator'] == vector['keys']
    a, c = states['paused_light']['playback'], states['resumed_light']['playback']
    assert (a['playId'],a['entryId'],a['queueRevision']) == (c['playId'],c['entryId'],c['queueRevision'])
    assert states['unfollowed_return_light']['cards'] == 0 and states['unfollowed_return_light']['keyword'] == '长名称'
    pending = states['pending_reopened_light']['albumCardFixture']
    reconciled = states['reconciled_light']['albumCardFixture']
    count = lambda value: sum(call['path'] == '/api/artist/sub' for call in value['calls'])
    assert count(pending) == count(reconciled) and reconciled['artistCollected']
    before, after = states['other_artist_paused_light']['playback'], states['empty_artist_failed_light']['playback']
    assert before['origin']['id'] == after['origin']['id'] == '14'
    assert before['playId'] == after['playId'] and after['state'] == 'paused'
    if args.screenshots:
        assert len(list(output.glob('ux_*.png'))) == 37
    report = dict(ok=True, states=37, screenshots=37 if args.screenshots else 0,
                  nativeAudio='libmpv/null', membershipWrites='isolated synthetic API only',
                  uncertainWriteReadbackVerified=True, realMusicRequests=0, realAccountWrites=0,
                  physicalAudioOutputVerified=False)
    (root/'report.json').write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
