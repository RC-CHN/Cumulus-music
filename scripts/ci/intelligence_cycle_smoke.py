"""Five-state mode entry/exit through production UI, with synthetic API/audio."""
import argparse
import json
import os
from pathlib import Path
from motion_smoke import launch

LABELS=['ring_single','ring_random','ring_sequential','ring_intelligence','ring_exited','held_seed','held_reentered','held_cancelled','held_resumed','held_exited','preparing_entry','cancelled_burst','footer_cancelled','entry_failed_1120_light','entry_failed_760_dark','entry_retried_760_dark','entry_retried_1120_light','stale_waiting','stale_replaced','ineligible_loop','removed_current_seed']

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=Path,required=True)
    parser.add_argument('--root',type=Path,required=True)
    parser.add_argument('--runtime',type=Path)
    parser.add_argument('--frontend-dist',type=Path)
    parser.add_argument('--screenshots',action='store_true')
    args=parser.parse_args();root=args.root.resolve();root.mkdir(parents=True,exist_ok=False)
    env={k:v for k,v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root),CUMULUS_MOTION_PROOF='1',CUMULUS_QUEUE_UI_PROOF='1',CUMULUS_MODE_UI_PROOF='1',CUMULUS_INTELLIGENCE_START_PROOF='1')
    for key,name in [('XDG_DATA_HOME','data'),('XDG_CONFIG_HOME','config'),('XDG_CACHE_HOME','cache'),('XDG_RUNTIME_DIR','runtime'),('APPDATA','roaming'),('LOCALAPPDATA','local')]:
        directory=root/'os-profile'/name;directory.mkdir(parents=True,mode=0o700);env[key]=str(directory)
    if args.runtime:env['PATH']=str(args.runtime.resolve())+os.pathsep+env.get('PATH','')
    if args.frontend_dist:env['CUMULUS_PROOF_FRONTEND_DIST']=str(args.frontend_dist.resolve())
    env['CUMULUS_UX_SCRIPT']=str(Path(__file__).resolve().parents[2]/'desktop/examples/intelligence_cycle_probe.js')
    with (root/'host.log').open('x',encoding='utf-8') as log:
        code=launch(args.binary.resolve(),env,log,root/'first',args.screenshots)
    failed=root/'first/failed.json';assert not failed.exists(),failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code==0,f'five-mode UI exited {code}'
    complete=json.loads((root/'first/complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['intelligenceCycle'] and complete['stageCount']==len(LABELS)
    assert complete['realMusicRequests']==complete['accountWrites']==0
    modes=['single_loop','random','sequential','order_loop','order_loop','order_loop','order_loop','order_loop','order_loop','order_loop','sequential','random','sequential','sequential','sequential','order_loop','order_loop','sequential','sequential','order_loop','order_loop']
    phases=[None,None,None,'active',None,'seeded','seeded','seeded','seeded',None,None,None,None,None,None,'active','active',None,None,None,'active']
    requests=[0,0,0,1,1,2,3,3,4,4,4,4,4,4,4,5,5,5,5,5,6]
    for i,label in enumerate(LABELS):
        path=root/'first'/f'ux_{i+1:02}_{label}.json';row=json.loads(path.read_text(encoding='utf-8'))
        native,queue=row['nativePlayer'],row['nativeQueue'];sent=row['syntheticIntelligenceStart']['requests']
        assert native['mode']==modes[i] and native['intelligence']==row['phase']==phases[i]
        assert len(sent)==requests[i] and queue['revision']==native['queueRevision']
        expected=3 if phases[i]=='active' else 1 if phases[i]=='seeded' else 2 if label in ['stale_replaced','ineligible_loop'] else 40
        assert len(queue['tracks'])==native['queueLen']==row['total']==expected
        assert native['playId']==row['playId'] and native['state']=='paused' and abs(native['position']-10)<.2 and not native['error']
        assert row['problem']==label.startswith('entry_failed_')
        if phases[i]:
            assert native['intelligenceOperation']==row['operation'] and row['operation']
        if label in ['preparing_entry','stale_waiting']:assert row['modePending'] and row['target']=='intelligence'
        else:assert not row['modePending']
        if label=='cancelled_burst':assert row['syntheticIntelligenceStart']['catalogStarted']
        if label.endswith('1120_light'):assert row['width']==1120 and row['theme']=='light'
        if label.endswith('760_dark'):assert row['width']==760 and row['theme']=='dark'
        if args.screenshots:assert path.with_suffix('.png').is_file()
    for i,payload in enumerate(sent):
        assert set(payload)=={'playlistId','songId','type','startMusicId','count'}
        assert payload['playlistId']==payload['songId']=='1' and payload['type']=='fromPlayOne'
        assert payload['count']==(39 if i==5 else 40) and payload['startMusicId']==('2' if i==5 else '1')
    report=dict(ok=True,actualWebView=True,states=len(LABELS),screenshots=len(LABELS) if args.screenshots else 0,productionStartVerified=True,intelligenceCycle=True,syntheticApi=True,syntheticPointerEvents=True,realMusicRequests=0,realAccountWrites=0,physicalDisplayVerified=False,physicalAudioOutputVerified=False)
    (root/'audit.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8');print(json.dumps(report))

if __name__=='__main__':main()
