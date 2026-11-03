"""Official comment resource headers, navigation and actual single-program audio in a real WebView."""
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
    env.update(CUMULUS_PROOF_DIR=str(root/'run'), CUMULUS_COMMENT_READ_PROOF='1',
               CUMULUS_UX_SCRIPT=str(Path(__file__).resolve().parents[2]/'desktop/examples/comment_resource_ui_probe.js'))
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
    assert code == 0, f'comment resource process exited {code}'
    complete = json.loads((output/'complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['commentResources']
    states = {value['stage'].split('_', 2)[2]: value for path in sorted(output.glob('ux_*.json'))
              if (value := json.loads(path.read_text(encoding='utf-8')))}
    assert len(states) == complete['stageCount'] == 29
    assert states['track_initial_light']['countFont'] == '16px'
    for state in states.values():
        assert state['visible'] and state['focused'] and state['focusValid'] and not state['overflow']
        assert not state['mvTitleOverflow'] and state['cards'] <= 64 and all(control['hit'] for control in state['controls'])
    for name in ['track_initial_light','voice_header_dark','small_voice_light']:
        assert states[name]['coverWidth'] == states[name]['coverHeight'] == 64
    assert states['small_mv_dark']['coverWidth'] == 114 and states['small_mv_dark']['coverHeight'] == 64
    before,after=states['track_paused_dark']['playback'],states['track_resumed_dark']['playback']
    assert (before['playId'],before['entryId'],before['queueRevision']) == (after['playId'],after['entryId'],after['queueRevision'])
    assert states['track_stopped_dark']['playback']['state'] == 'idle'
    assert states['track_restarted_dark']['playback']['state'] == 'playing'
    assert states['resource_cached_failure_light']['cached'] and states['resource_cached_failure_light']['metadataError']
    assert not states['resource_cache_retry_light']['cached'] and not states['resource_cache_retry_light']['metadataError']
    assert states['resource_cache_retry_light']['draft'] == '合成草稿：资源重试不得修改'
    assert states['resource_empty_failure_light']['metadataError'] and not states['resource_empty_failure_light']['cached']
    assert not states['resource_empty_retry_light']['metadataError']
    assert states['held_resource_dark']['commentReadFixture']['pending']
    assert states['late_resource_dark']['commentReadFixture']['delayedCompleted'] == 1 and states['late_resource_dark']['kind'] is None
    playing,paused=states['voice_playing_dark']['playback'],states['voice_paused_dark']['playback']
    assert playing['track']['id'] == '181' and playing['track']['voiceId'] == '81' and playing['resourceKind'] == 'voice'
    assert (playing['playId'],playing['entryId'],playing['queueRevision']) == (paused['playId'],paused['entryId'],paused['queueRevision'])
    assert states['creator_return_dark']['resourceId'] == '81'
    assert states['radio_return_dark']['resourceId'] == states['small_voice_light']['resourceId'] == '81'
    if args.screenshots: assert len(list(output.glob('ux_*.png'))) == 29
    report=dict(ok=True,states=29,screenshots=29 if args.screenshots else 0,metadataRecoveryVerified=True,resourceNavigationVerified=True,nativeVoicePlaybackVerified=True,realMusicRequests=0,realAccountWrites=0,physicalAudioOutputVerified=False,videoPlaybackVerified=False)
    (root/'report.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8');print(json.dumps(report))


if __name__ == '__main__':
    main()
