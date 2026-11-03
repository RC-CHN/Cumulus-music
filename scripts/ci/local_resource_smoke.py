"""Local query ownership and cloud/download/library/queue consistency in the real WebView."""
import argparse
import json
import os
from pathlib import Path
from window_manager import launch

LABELS = ['library_light', 'local_directory_menu_light', 'local_directory_light', 'composition_pending_light', 'composition_complete_dark', 'search_live_dark', 'search_returned_light', 'query_pending_dark',
          'query_latest_light', 'query_failed_dark', 'query_empty_light', 'group_cleared_dark',
          'queue_before_light', 'downloads_before_dark', 'cloud_confirm_light', 'cloud_removed_dark',
          'downloads_removed_dark', 'library_removed_light', 'queue_reconciled_dark', 'cloud_keep_light',
          'download_kept_playback_dark', 'library_kept_light', 'read_failed_cached_dark',
          'library_recovered_light', 'command_failure_persisted_dark', 'rescan_recovered_light']


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
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_LOCAL_RESOURCE_UI_PROOF='1', CUMULUS_CLOUD_UI_PROOF='1', CUMULUS_CLOUD_BATCH_UI_PROOF='1',
               CUMULUS_CLOUD_MENU_UI_PROOF='1', CUMULUS_MINI_SCRIPT=str(Path(__file__).resolve().parents[2]/'desktop/examples/local_resource_probe.js'))
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'), ('XDG_CACHE_HOME', 'cache'),
                      ('XDG_RUNTIME_DIR', 'runtime'), ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        path = root/'os-profile'/name
        path.mkdir(parents=True, mode=0o700)
        env[key] = str(path)
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve())
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve())+os.pathsep+env.get('PATH', '')
    if os.name != 'nt':
        launcher = root / 'test-launcher'
        launcher.mkdir()
        shim = launcher / 'xdg-open'
        shim.write_text('#!/bin/sh\n[ "$#" -eq 1 ] && [ -d "$1" ] || exit 1\nprintf "%s\\n" "$1" >> "$LOCAL_RESOURCE_DIRECTORY_LOG"\n', encoding='utf-8')
        shim.chmod(0o700)
        env['LOCAL_RESOURCE_DIRECTORY_LOG'] = str(root / 'opened-directory')
        env['PATH'] = str(launcher) + os.pathsep + env.get('PATH', '')
    source = Path(env['CUMULUS_MINI_SCRIPT']).read_text(encoding='utf-8')
    script = root / 'local-resource.js'
    script.write_text(f'window.__localDirectoryLauncherProof = {json.dumps(os.name != "nt")};\n' + source, encoding='utf-8')
    env['CUMULUS_MINI_SCRIPT'] = str(script)

    with (root/'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, root/'first', args.screenshots)
    failed = root/'first/failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'local resource UI process exit {code}'
    complete = json.loads((root/'first/complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['localResourceUi'] and complete['stageCount'] == len(LABELS)
    states = {}
    for number, label in enumerate(LABELS, 1):
        path = root/'first'/f'ux_{number:02}_{label}.json'
        state = json.loads(path.read_text(encoding='utf-8')); states[label] = state
        assert state['theme'] == ('dark' if label.endswith('_dark') else 'light')
        assert not state['overflow'] and state['focusValid'] and all(button['hit'] for button in state['buttons'])
        assert not state['nativePlayer']['error'] and not state['nativeMini']['error']
        if args.screenshots:
            assert path.with_suffix('.png').is_file()
    assert states['library_light']['localCount'] == 3
    assert states['local_directory_menu_light']['focusLabel'] == '打开文件所在目录'
    assert states['local_directory_light']['directoryRequests'] == 1
    assert states['local_directory_light']['directoryAccepted']
    if os.name != 'nt':
        opened = (root/'opened-directory').read_text(encoding='utf-8').splitlines()
        physical_source = Path(json.loads((root/'removal-path-1004.json').read_text(encoding='utf-8')))
        assert physical_source.is_relative_to(root / 'menu-originals')
        assert opened == [str(physical_source.parent)]

    assert states['composition_pending_light']['localSearch'] == 'zhong' and states['composition_pending_light']['imePending']
    assert states['composition_complete_dark']['localSearch'] == '合成' and states['composition_complete_dark']['imeCommitted']
    assert states['search_live_dark']['localSearch'] == '006' and states['search_live_dark']['localCount'] == 1
    assert states['search_returned_light']['localSearch'] == '005' and states['search_returned_light']['localCount'] == 1
    assert states['query_pending_dark']['localCount'] == 0 and states['query_pending_dark']['heldActualNativeRead']
    assert states['query_latest_light']['cueCount'] == 1
    assert states['query_failed_dark']['readError'] and states['query_failed_dark']['localCount'] == 0
    assert states['query_empty_light']['localEmpty'] == '没有匹配的本地音乐' and not states['query_empty_light'].get('readError')
    assert states['group_cleared_dark']['localSearch'] == '' and states['group_cleared_dark']['group'] == '歌手'
    assert len(states['queue_before_light']['queueEntries']) == 3
    assert len(states['downloads_before_dark']['downloadIds']) == 1
    assert states['downloads_removed_dark']['downloadIds'] == [] and states['downloads_removed_dark']['downloadSearch'] == '006'
    assert states['library_removed_light']['localCount'] == 2 and states['library_removed_light']['cueCount'] == 1
    assert len(states['queue_reconciled_dark']['queueEntries']) == 2
    assert states['library_kept_light']['localCount'] == 2 and states['library_kept_light']['cueCount'] == 1
    assert states['read_failed_cached_dark']['localCount'] == 2 and states['read_failed_cached_dark']['localPlayable'] == 0
    assert states['command_failure_persisted_dark']['commandError'] and states['command_failure_persisted_dark']['injectedCommandFailure']
    assert not states['rescan_recovered_light'].get('commandError')
    assert complete['cloudDeletes'] == [['1006'], ['1005']]
    assert complete['deletedLocalFiles'] == 1 and complete['remainingDownloads'] == complete['remainingLocalTracks'] == 2
    report = dict(ok=True, states=len(LABELS), screenshots=len(LABELS) if args.screenshots else 0,
                  actualWebViews=1, deletedSyntheticFiles=1, remainingDownloads=2, remainingLocalTracks=2,
                  exactCloudDeleteRequests=2, queryOwnershipVerified=True, retainedCueIdentityVerified=True,
                  localDirectoryActionVerified=True, nativeDirectoryLauncherIntercepted=os.name != 'nt',
                  realAccountWrites=0, realMusicRequests=0, physicalAudioOutputVerified=False)
    (root/'report.json').write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
