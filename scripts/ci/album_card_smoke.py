"""Album metadata, native source controls, menus and journaled membership in a real WebView."""
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
    env.update(CUMULUS_PROOF_DIR=str(root/'run'), CUMULUS_ALBUM_CARD_PROOF='1',
               CUMULUS_UX_SCRIPT=str(Path(__file__).resolve().parents[2]/'desktop/examples/album_card_ui_probe.js'))
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
    assert code == 0, f'album card process exited {code}'
    complete = json.loads((output/'complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['albumCards']
    states = {value['stage'].split('_', 2)[2]: value for path in output.glob('ux_*.json')
              if (value := json.loads(path.read_text(encoding='utf-8')))}
    assert len(states) == complete['stageCount'] == 39
    for state in states.values():
        assert state['visible'] and state['focused'] and state['focusValid'] and not state['overflow']
        assert state['cards'] <= 36 and all(control['hit'] for control in state['controls'])
    first = states['artist_cards_light']['metadata']
    assert first[0]['credits'] == '主要署名 / 测试歌手' and first[0]['date'].startswith('· 2023-11-')
    assert first[2]['fallback'] and first[2]['explicit'] and not first[2]['credits'] and not first[2]['date']
    oracle = json.loads((Path(__file__).resolve().parents[2]/'frontend/tests/fixtures/official-album-card-menu.json').read_text(encoding='utf-8'))
    common = {'open', 'play', 'next', 'collect', 'share', 'copy', 'remove'}
    for vector, stage in zip(oracle['vectors'], ['keyboard_menu_dark', 'saved_menu_light'], strict=True):
        assert [key for key in states[stage]['menuKeys'] if key in common] == vector['commonCardKeys']
    before = states['paused_dark']['playback']
    after = states['resumed_dark']['playback']
    assert (before['playId'], before['entryId'], before['queueRevision']) == (after['playId'], after['entryId'], after['queueRevision'])
    assert states['next_album_dark']['playback']['state'] == 'paused'
    assert [entry['track'] for entry in states['next_album_dark']['queue']] == ['1', '4', '5', '6', '2', '3']
    assert states['removed_empty_light']['cards'] == states['detail_removed_return_light']['cards'] == 0
    assert states['removed_empty_light']['keyword'] == '测试专辑'
    assert states['collect_read_failed_dark']['error'] and not states['collect_read_recovered_dark']['error']
    pending = states['pending_reopened_light']['albumCardFixture']
    reconciled = states['reconciled_light']['albumCardFixture']
    count = lambda fixture: sum(call['path'] == '/api/album/sub' for call in fixture['calls'])
    assert count(pending) == count(reconciled) and reconciled['albumCollected']
    if args.screenshots:
        assert len(list(output.glob('ux_*.png'))) == 39
    report = dict(ok=True, states=39, screenshots=39 if args.screenshots else 0,
                  nativeAudio='libmpv/null', membershipWrites='isolated synthetic API only',
                  uncertainWriteReadbackVerified=True, realMusicRequests=0, realAccountWrites=0,
                  physicalAudioOutputVerified=False)
    (root/'report.json').write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
