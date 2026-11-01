"""Production mini menu, mode submenu and actual desktop lyric windows."""
import argparse
import json
import os
from pathlib import Path
from window_manager import launch

LABELS = ['window_light', 'window_dark', 'modes_dark', 'child_escape_dark', 'menu_closed_dark',
          'modes_keyboard_light', 'selected_random_light', 'checked_random_light', 'pinned_off_light',
          'pinned_menu_dark', 'opening_lyrics_dark', 'startup_failed_dark', 'retry_menu_dark',
          'lyrics_open_light', 'lyrics_locked_light', 'lyrics_unlocked_dark', 'external_closed_dark',
          'external_reopened_light', 'save_failed_dark', 'save_recovered_light', 'damaged_dark',
          'repaired_light', 'lyrics_reopened_light', 'heart_modes_light', 'heart_active_light',
          'heart_checked_dark', 'heart_exit_dark', 'room_modes_light', 'fm_modes_dark',
          'source_changed_dark', 'returned_main_dark', 'reentered_context_light', 'account_closed_light']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--exit', action='store_true', help='verify the actual menu exit instead of account switch')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k:v for k,v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MINI_SCRIPT=str(Path(__file__).resolve().parents[2]/'desktop/examples/mini_menu_probe.js'),
               CUMULUS_MINI_FM_PROOF='1', CUMULUS_INTELLIGENCE_START_PROOF='1')
    if args.exit:
        env['CUMULUS_MINI_MENU_EXIT_PROOF'] = '1'
    for key,name in [('XDG_DATA_HOME','data'),('XDG_CONFIG_HOME','config'),('XDG_CACHE_HOME','cache'),('XDG_RUNTIME_DIR','runtime'),('APPDATA','roaming'),('LOCALAPPDATA','local')]:
        directory = root/'os-profile'/name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve())+os.pathsep+env.get('PATH','')
    with (root/'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, root/'first', args.screenshots)
    failed = root/'first/failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'mini menu exit {code}'
    complete = json.loads((root/'first'/('exit-armed.json' if args.exit else 'complete.json')).read_text(encoding='utf-8'))
    labels = LABELS[:-1]+['exit_ready_light'] if args.exit else LABELS
    assert complete['miniMenuUi'] and complete['stageCount'] == len(labels)
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    if not args.exit:
        assert complete['ok'] and complete['staleContextRejected'] and complete['ipcIsolationVerified'] and not complete['twoAccountRealtimeVerified']
    for i,label in enumerate(labels,1):
        path = root/'first'/f'ux_{i:02}_{label}.json'
        row = json.loads(path.read_text(encoding='utf-8'))
        native = row['nativePlayer']
        assert row['factoryCount'] == 1 and not native['error']
        lyric = row['lyricMenuStatus']
        visible = i in [14,15,16,18,19] or i>=23
        assert lyric['visible']==visible and lyric['opening']==(i==11) and lyric['locked']==(i==15), (label,lyric)
        assert len(row['lyricWindows']) == (1 if visible or i==11 else 0)
        assert bool(lyric['error'])==(i in [12,13])
        assert lyric['preferences']['damaged']==(i==21)
        if i==21:
            assert lyric['preferences']['current'] is None
        else:
            assert lyric['preferences']['current']['enabled']==(i in [11,12,13,14,15,16,18,19] or i>=23)
        if i!=33 or args.exit:
            assert native['state'] == 'paused' and abs(native['position']-10) < .2 and native['playId'] == row['expectedPlayId']
        else:
            assert native['track'] is None and native['playId'] is None
        if i!=33 or args.exit:
            assert native['track']['id']==('1' if i<29 else '101' if i==29 else '102')
        assert row['syntheticFm']['personalReads']==(0 if i<29 else 1)
        assert row['syntheticFm']['skipWrites']==(0 if i<30 else 1)
        assert row['syntheticFm']['trashWrites']==row['syntheticFm']['artistWrites']==0
        assert len(row['syntheticIntelligence']['requests'])==(0 if i<25 else 1)
        assert row['syntheticRoom']==dict(created=0 if i<28 else 1,ended=0 if i<29 else 1,inside=i==28,realRequests=0)
        assert row['theme'] == ('dark' if label.endswith('_dark') else 'light')
        if i==31:
            assert row['windowKind']=='main' and row['mainVisible'] and not row['miniWindows']
        else:
            assert row['windowKind']=='mini' and not row['mainVisible'] and len(row['miniWindows'])==1
            assert not row['overflow'] and row['width']==420 and row['height']==206
            assert row['focusInside'] and all(not p['inert'] for p in row['menus'])
            assert row['pin'] == ('true' if i<9 else 'false')
            assert bool(row['alert'])==(i in [12,13,19])
            assert row['root']==(i in [1,2,3,4,6,8,10,13,14,15,16,17,18,21,22,24,26,28,29,32] or (args.exit and i==33))
            assert row['submenu']==(i in [3,6,8,24,26,28,29])
        if i in [3,6,8,24,26,28,29]:
            options = row['menus'][1]['buttons']
            expected = 3 if i==28 else 2 if i==29 else 5
            assert len(options)==expected and sum(v['checked']=='true' for v in options)==1
            assert all(v['visible'] and v['hit'] and not v['disabled'] for v in options)
            assert row['activeKey'] in [v['key'] for v in options]
        if i==11:
            assert row['canTransport'] and row['lyricMenuStatus']['opening']
        if i==15:
            assert row['lyricMenuStatus']['locked']
        if i in [16,17]:
            assert not row['lyricMenuStatus']['locked']
        if i==21:
            assert row['lyricMenuStatus']['preferences']['damaged']
        if args.screenshots:
            assert path.with_suffix('.png').is_file()
    report = dict(ok=True, states=len(labels), screenshots=len(labels) if args.screenshots else 0,
                  actualWebViewsUpTo=3, actualMenuExitVerified=args.exit, actualLyricLifecycleVerified=True,
                  realMusicRequests=0, realAccountWrites=0, twoAccountRealtimeVerified=False,
                  physicalDisplayVerified=False, physicalAudioOutputVerified=False)
    (root/'report.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
    print(json.dumps(report))


if __name__=='__main__':
    main()
