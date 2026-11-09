"""Comment read recovery, preserved navigation and actual WebView focus in a real WebView."""
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
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root/'run'), CUMULUS_COMMENT_READ_PROOF='1',
               CUMULUS_UX_SCRIPT=str(Path(__file__).resolve().parents[2]/'desktop/examples/comment_read_ui_probe.js'))
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'), ('XDG_CACHE_HOME', 'cache'),
                      ('XDG_RUNTIME_DIR', 'runtime'), ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        path = root/'os-profile'/name
        path.mkdir(parents=True, mode=0o700)
        env[key] = str(path)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve())+os.pathsep+env.get('PATH', '')
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve())
    output = root/'run/first'
    with (root/'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, output, args.screenshots)
    failed = output/'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'comment read process exited {code}'
    complete = json.loads((output/'complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['commentReads']
    states = {value['stage'].split('_', 2)[2]: value for path in sorted(output.glob('ux_*.json'))
              if (value := json.loads(path.read_text(encoding='utf-8')))}
    assert len(states) == complete['stageCount'] == 41
    assert states['initial_light']['stickyComposer'] and states['small_latest_light']['stickyComposer']
    assert states['initial_light']['richCommentText']
    for state in states.values():
        assert state['visible'] and state['focused'] and state['focusValid'] and not state['overflow']
        assert state['cards'] <= 64 and all(control['hit'] for control in state['controls'])
    assert states['next_failed_light']['ids'][0] == '101' and states['next_retry_light']['ids'][0] == '121'
    assert states['cached_next_failure_light']['ids'][0] == '121' and states['cached_next_failure_light']['cacheWarnings']
    assert states['cache_return_light']['ids'][0] == '101'
    assert states['cached_next_retry_light']['scrollTop'] > 0 and '140' in states['cached_next_retry_light']['visibleIds']
    assert '540' in states['floor_cached_retry_light']['floorVisibleIds']
    a=states['next_failed_light']['commentReadFixture']['calls'][-1]
    c=states['next_retry_light']['commentReadFixture']['calls'][-1]
    assert a['args'] == c['args'] and c['args']['offset'] == 20
    assert states['hot_failed_dark']['sort'] == 'latest' and states['hot_retry_dark']['sort'] == 'hot'
    assert states['keyboard_latest_dark']['focusSort'] == 'latest'
    target = '900719925474099312345'
    assert states['target_retry_dark']['commentReadFixture']['calls'][-1]['args']['commentId'] == target
    targeted = [c for c in states['missing_target_dark']['commentReadFixture']['calls']
                if c['args'].get('commentId') not in (None, '0')]
    assert not any(c['args'].get('commentId') not in (None, '0')
                   for c in states['invalid_target_dark']['commentReadFixture']['calls'])
    assert [(c['args']['commentId'], c['mode']) for c in targeted] == [
        (target, 'fail-target'), (target, 'none'), ('777', 'missing-target'),
    ]
    assert targeted[0]['path'] == targeted[1]['path'] == '/api/v1/resource/comments/R_SO_4_1'
    assert targeted[0]['args'] == targeted[1]['args']
    assert states['floor_failed_first_light']['floorIds'] == [] and states['floor_retry_light']['floorCurrent'] == '602'
    assert states['floor_next_failed_light']['floorIds'][0] == '501' and states['floor_next_retry_light']['floorIds'][0] == '521'
    a=states['floor_next_failed_light']['commentReadFixture']['calls'][-1]
    c=states['floor_next_retry_light']['commentReadFixture']['calls'][-1]
    assert a['args'] == c['args'] and c['args']['time'] == 1000
    assert states['floor_closed_light']['focusComment'] == '101'
    assert states['late_read_inflight_dark']['commentReadFixture']['pending']
    assert not states['left_before_late_dark']['ids'] and not states['late_finished_dark']['ids']
    assert states['late_finished_dark']['commentReadFixture']['delayedCompleted'] == 1
    for state in list(states.values())[:27]: assert state['draft'] == '合成草稿：只检查保留，不发送。'
    assert states['author_return_light']['focusComment'] == '101'
    assert states['floor_author_return_light']['focusComment'] == '501'
    profile = [c for c in states['floor_author_return_light']['commentReadFixture']['calls']
               if c['path'] == '/api/w/v1/user/detail/3']
    assert len(profile) >= 1 and all(c['args']['userId'] == '3' for c in profile)
    assert states['reduced_floor_open_light']['motion'] == states['reduced_floor_close_light']['motion'] == 'reduced'
    assert not states['expired_light']['ids'] and states['expired_light']['cards'] == 0
    if args.screenshots: assert len(list(output.glob('ux_*.png'))) == 41
    report = dict(ok=True,states=41,screenshots=41 if args.screenshots else 0,exactReadRetryVerified=True,historyAndFocusVerified=True,stickyComposerVerified=True,richCommentTextVerified=True,authorProfileVerified=True,realMusicRequests=0,realAccountWrites=0,playbackVerified=False)
    (root/'report.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
