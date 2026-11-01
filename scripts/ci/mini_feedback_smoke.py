"""Two real WebViews exercise the mini FM feedback menu and shared journal."""
import argparse
import json
import os
from pathlib import Path
from motion_smoke import launch

LABELS = ['compact_light', 'reason_wait_light', 'reason_cancelled_light', 'ready_light',
          'ready_dark', 'expanded_dark', 'reason_error_dark', 'reason_retried_dark',
          'reason_empty_light', 'rejected_light', 'rejected_dark', 'observed_dark',
          'write_pending_dark', 'submitted_dark', 'changed_dark', 'changed_closed_dark',
          'artist_partial_light', 'main_partial_light', 'reopened_light', 'uncertain_light',
          'uncertain_observed_light', 'uncertain_reopened_dark', 'main_uncertain_dark',
          'reconciled_main_dark', 'unobserved_light', 'readonly_failed_light',
          'readonly_healed_dark', 'returned_main_dark']


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--binary', type=Path, required=True)
    p.add_argument('--root', type=Path, required=True)
    p.add_argument('--runtime', type=Path)
    p.add_argument('--screenshots', action='store_true')
    args = p.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k:v for k,v in os.environ.items() if not k.startswith('CUMULUS_')}
    script = Path(__file__).resolve().parents[2]/'desktop/examples/mini_feedback_probe.js'
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MINI_FM_PROOF='1', CUMULUS_MINI_SCRIPT=str(script))
    for key,name in [('XDG_DATA_HOME','data'),('XDG_CONFIG_HOME','config'),('XDG_CACHE_HOME','cache'),
                     ('XDG_RUNTIME_DIR','runtime'),('APPDATA','roaming'),('LOCALAPPDATA','local')]:
        directory = root/'os-profile'/name
        directory.mkdir(parents=True, mode=0o700)
        env[key]=str(directory)
    if args.runtime:
        env['PATH']=str(args.runtime.resolve())+os.pathsep+env.get('PATH','')
    with (root/'host.log').open('x', encoding='utf-8') as log:
        code=launch(args.binary.resolve(),env,log,root/'first',args.screenshots)
    failed=root/'first/failed.json'
    assert not failed.exists(),failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code==0,f'mini feedback exited {code}'
    complete=json.loads((root/'first/complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['miniFeedbackUi'] and complete['ipcIsolationVerified'] and complete['stageCount']==len(LABELS)
    assert complete['realMusicRequests']==complete['realAccountWrites']==0
    for i,label in enumerate(LABELS,1):
        path=root/'first'/f'ux_{i:02}_{label}.json'
        row=json.loads(path.read_text(encoding='utf-8'))
        native,api=row['nativePlayer'],row['syntheticFm']
        assert native['fm'] and native['mode']=='sequential' and not native['error']
        assert native['state']=='paused' and abs(native['position']-10)<.2
        assert native['playId']==row['expectedPlayId'] and row['factoryCount']==1
        assert native['track']['id']==str(101 if i<=13 else 102 if i==14 else 103 if i<=16 else 104 if i<=23 else 105)
        assert api['personalReads']==1 and api['zoneReads']==0
        assert api['skipWrites']==(0 if i<15 else 1)
        assert api['reasonReads']==[0,1,1,2,2,3,4,5,6,7,7,7,7,7,8,8,9,9,9,10,10,10,10,10,11,11,11,11][i-1]
        assert api['trashWrites']==(0 if i<10 else 1 if i<13 else 2 if i<17 else 3 if i<20 else 4 if i<25 else 5)
        assert api['artistWrites']==(0 if i<17 else 1 if i<19 else 2)
        assert api['remoteTrash']==([] if i<13 else ['101'] if i<17 else ['101','103'] if i<20 else ['101','103','104'])
        assert api['remoteArtists']==([] if i<19 else ['1'])
        assert api['corrupted']==(i in [25,26])
        assert bool(row.get('nativeFmFailure'))==(i in [25,26])
        main=i in [18,23,24,28]
        assert row['mainVisible']==main and len(row['miniWindows'])==(0 if main else 1)
        assert row['windowKind']==('main' if main else 'mini')
        assert row['theme']==('dark' if label.endswith('_dark') else 'light')
        if main:
            assert row['pageFm']
            assert row['mainFmTitle']==f"song-{native['track']['id']}"
            assert bool(row['recovery'])==(i in [18,23])
            assert row['recoveryVisible']==(i in [18,23])
            if i==28:
                assert row['stateIssue'] is None
        else:
            assert row['width']==420 and row['height']==(516 if 6<=i<=17 else 206)
            assert not row['overflow'] and row['focusInside'] and all(c['hit'] for c in row['controls'])
            assert row['canSubmit']==(i in [2,4,5,6,7,8,9,12,27])
            assert row['dialog']==(i not in [1,3,14,16,19])
            if i in [15,17]:
                assert row['entryId']!=row['nativeMini']['selection']['audioEntryId']
        if args.screenshots:
            assert path.with_suffix('.png').is_file()
    report=dict(ok=True,actualWebViews=2,states=len(LABELS),screenshots=len(LABELS) if args.screenshots else 0,
                realMusicRequests=0,realAccountWrites=0,physicalDisplayVerified=False,physicalAudioOutputVerified=False,
                mainJournalHandoffVerified=True,readonlyCacheRecoveryVerified=True)
    (root/'report.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
    print(json.dumps(report))


if __name__=='__main__':
    main()
