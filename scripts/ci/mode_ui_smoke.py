"""Production mode switching, native queue fences, FM recovery and restart."""
import argparse
import json
import os
from pathlib import Path
from motion_smoke import launch


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=Path,required=True)
    parser.add_argument('--root',type=Path,required=True)
    parser.add_argument('--runtime',type=Path)
    parser.add_argument('--frontend-dist',type=Path)
    parser.add_argument('--screenshots',action='store_true')
    parser.add_argument('--empty-fm',action='store_true')
    args=parser.parse_args();root=args.root.resolve();root.mkdir(parents=True,exist_ok=False)
    env={k:v for k,v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root),CUMULUS_MOTION_PROOF='1',CUMULUS_MODE_UI_PROOF='1')
    if args.empty_fm:env['CUMULUS_EMPTY_FM_UI_PROOF']='1'
    for key,name in [('XDG_DATA_HOME','data'),('XDG_CONFIG_HOME','config'),('XDG_CACHE_HOME','cache'),('XDG_RUNTIME_DIR','runtime'),('APPDATA','roaming'),('LOCALAPPDATA','local')]:
        p=root/'os-profile'/name;p.mkdir(parents=True,mode=0o700);env[key]=str(p)
    if args.runtime:env['PATH']=str(args.runtime.resolve())+os.pathsep+env.get('PATH','')
    if args.frontend_dist:env['CUMULUS_PROOF_FRONTEND_DIST']=str(args.frontend_dist.resolve())
    labels=[f'{stage}_{width}_{theme}' for width,theme in [(1120,'light'),(760,'light'),(760,'dark'),(1120,'dark')] for stage in ['single','random','sequential','loop','failed','closed','reopened','retried']]
    labels+=['pending','coalesced','stale_ordinary','fresh_ordinary','fm_infinite','fm_single','fm_infinite_again','fm_failed','fm_recovered','fm_resumed','fm_warning_paused','left_fm','late_fm_rejected']
    reopened_labels=['reopened_normal','reopened_switched','reopened_restored']
    if args.empty_fm:
        labels=['normal_backup','empty_initial','empty_refetched']+[f'empty_{stage}_{width}_{theme}' for width,theme in [(1120,'light'),(760,'light'),(760,'dark'),(1120,'dark')] for stage in ['infinite','single']]+['empty_failed','empty_retried']
        reopened_labels=['empty_reopened','empty_infinite_after_reopen','left_empty_fm','normal_switched','normal_restored']
    source=Path(__file__).resolve().parents[2]/'desktop/examples'/('empty_fm_ui_probe.js' if args.empty_fm else 'mode_ui_probe.js');reports=[]
    for reopened,name,stages in [(False,'first',labels),(True,'reopen',reopened_labels)]:
        script=root/f'{name}.js';script.write_text('window.__modeReopen='+json.dumps(reopened)+';\n'+source.read_text(encoding='utf-8'),encoding='utf-8');env['CUMULUS_UX_SCRIPT']=str(script)
        if reopened:env['CUMULUS_PROOF_REOPEN']='1'
        with (root/f'{name}.log').open('x',encoding='utf-8') as log:code=launch(args.binary.resolve(),env,log,root/name,args.screenshots)
        failed=root/name/'failed.json';assert not failed.exists(),failed.read_text(encoding='utf-8') if failed.exists() else ''
        assert code==0,f'mode UI exited {code}'
        complete=json.loads((root/name/'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['stageCount']==len(stages) and complete['modeUiVerified'] and complete['ordinaryVerified']
        assert complete['fmVerified']==(args.empty_fm or not reopened) and not complete['togetherPresentationVerified']
        assert complete.get('emptyFmVerified',False)==args.empty_fm
        assert complete['accountWrites']==complete['realMusicRequests']==complete['syntheticMode']['skipWrites']==0
        if args.empty_fm and not reopened:
            before=json.loads((root/name/'ux_02_empty_initial.json').read_text(encoding='utf-8'))
            after=json.loads((root/name/'ux_03_empty_refetched.json').read_text(encoding='utf-8'))
            assert after['syntheticMode']['fmReads']>before['syntheticMode']['fmReads']
        for i,label in enumerate(stages,1):
            path=root/name/f'ux_{i:02}_{label}.json';row=json.loads(path.read_text(encoding='utf-8'));native=row['nativePlayer']
            assert row['contained'] and row['triggerHit'] and native['mode']==row['mode'] and native['fm']==row['fm']
            assert not native['error']
            if args.empty_fm and row['fm']:
                assert native['state']=='idle' and native['queueLen']==0 and native['track'] is native['playId'] is native['position'] is None
                assert row['family']=='fm' and row['returnHit'] and row['emptyControlsDisabled']
            else:
                assert native['state']=='paused' and abs(native['position']-10)<.2
                assert native['playId']==row['playId'] and native['track']['id']==row['trackId']
            if row['feedback']:assert all(c['hit'] for c in row['feedback']['buttons'])
            if args.screenshots:assert path.with_suffix('.png').is_file()
        reports.append({'ok':True,'actualWebView':True,'modeUiVerified':True,'ordinaryVerified':True,'fmVerified':args.empty_fm or not reopened,'emptyFmVerified':args.empty_fm,'togetherPresentationVerified':False,'states':len(stages),'reopened':reopened,'screenshots':len(stages) if args.screenshots else 0,'realAccountWrites':0,'realMusicRequests':0,'physicalAudioOutputVerified':False,'physicalDisplayVerified':False})
    (root/'audit.json').write_text(json.dumps(reports,indent=2)+'\n',encoding='utf-8');print(json.dumps(reports))


if __name__=='__main__':main()
