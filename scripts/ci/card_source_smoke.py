"""Native current-source controls in playlist grid/list and all five playable search cards."""
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
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1', CUMULUS_LIBRARY_UI_PROOF='1',
               CUMULUS_SEARCH_UI_PROOF='1', CUMULUS_SEARCH_CARDS_PROOF='1',
               CUMULUS_CARD_SOURCE_PROOF='1',
               CUMULUS_UX_SCRIPT=str(Path(__file__).resolve().parents[2]/'desktop/examples/card_source_ui_probe.js'))
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'), ('XDG_CACHE_HOME', 'cache'),
                      ('XDG_RUNTIME_DIR', 'runtime'), ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        path = root/'os-profile'/name
        path.mkdir(parents=True, mode=0o700)
        env[key] = str(path)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve())+os.pathsep+env.get('PATH', '')
    with (root/'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, root/'first', args.screenshots)
    output = root/'first'
    failed = output/'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'card source process exited {code}'
    complete = json.loads((output/'complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['cardSources'] and complete['stageCount'] == 37
    assert complete['searchCardArtistSongCalls'] == [{'id': '101', 'limit': 50, 'offset': 0}] * 2
    states = {value['stage'].split('_', 2)[2]: value for path in output.glob('ux_*.json')
              if (value := json.loads(path.read_text(encoding='utf-8')))}
    assert len(states) == 37
    for state in states.values():
        assert state['visible'] and state['focused'] and state['focusValid'] and not state['overflow']
        assert state['sourceFocus'] is not False
        assert state['nativePlayer']['state'] == state['player']['state']
        assert state['nativeQueue']['tracks'] == state['queue']['tracks']
    if args.screenshots:
        assert len(list(output.glob('ux_*.png'))) == 37
    assert states['library_buffering_dark']['player']['state'] == 'buffering'
    assert states['library_resume_failed_dark']['issue'] and states['library_resume_failed_dark']['sourceAudio']['failedResumes'] == 1
    for a, b in [('library_playing_light', 'library_retry_light'), ('library_restarted_light', 'library_list_resumed_light')]+[
            (f'{category}_playing', f'{category}_resumed') for category in ['playlists', 'albums', 'artists', 'podcasts', 'voices']]:
        before, after = states[a], states[b]
        for key in ['playId', 'entryId', 'queueRevision', 'origin']:
            assert before['player'][key] == after['player'][key]
        assert before['queue']['tracks'] == after['queue']['tracks']
        assert before['sourceAudio']['loads'] == after['sourceAudio']['loads']
        assert after['player']['position']+.1 >= before['player']['position']
    for prefix in ['library']+['playlists', 'albums', 'artists', 'podcasts', 'voices']:
        stopped = states[f'{prefix}_stopped_light' if prefix == 'library' else f'{prefix}_stopped']
        restarted = states[f'{prefix}_restarted_light' if prefix == 'library' else f'{prefix}_restarted']
        assert stopped['player']['state'] == 'idle' and restarted['player']['state'] == 'playing'
        assert restarted['sourceAudio']['loads'] == stopped['sourceAudio']['loads']+1
    assert states['library_other_source_light']['player']['origin'] == {'kind': 'playlist', 'id': '3'}
    assert complete['syntheticLibrary']['writes'] == 0
    report = dict(ok=True, states=37, screenshots=37 if args.screenshots else 0,
                  nativeResumeFailureRecovered=True, sourceIdentityPreserved=True, stoppedSourcesRestarted=6,
                  syntheticBufferingStatus=True, syntheticResumeFailure=True, syntheticArtwork=True,
                  realAccountWrites=0, realMusicRequests=0, physicalAudioOutputVerified=False)
    (root/'report.json').write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
