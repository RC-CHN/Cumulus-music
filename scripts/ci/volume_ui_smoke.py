"""Production vertical volume UI, failure recovery and native restart persistence."""
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
    args=parser.parse_args()
    root=args.root.resolve();root.mkdir(parents=True,exist_ok=False)
    env={k:v for k,v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root),CUMULUS_MOTION_PROOF='1',CUMULUS_VOLUME_UI_PROOF='1')
    for key,name in [('XDG_DATA_HOME','data'),('XDG_CONFIG_HOME','config'),('XDG_CACHE_HOME','cache'),('XDG_RUNTIME_DIR','runtime'),('APPDATA','roaming'),('LOCALAPPDATA','local')]:
        directory=root/'os-profile'/name;directory.mkdir(parents=True,mode=0o700);env[key]=str(directory)
    if args.runtime:env['PATH']=str(args.runtime.resolve())+os.pathsep+env.get('PATH','')
    if args.frontend_dist:env['CUMULUS_PROOF_FRONTEND_DIST']=str(args.frontend_dist.resolve())
    labels=[f'{stage}_{width}_{theme}' for width,theme in [(1120,'light'),(760,'light'),(760,'dark'),(1120,'dark')] for stage in ['hover','changed','muted','unmuted','keyboard']]
    labels+=['mode_single_loop','mode_random','mode_sequential','mode_order_loop','maximum','wheel','zero','wheel_up','external_muted','external_unmuted','pending','coalesced','save_failed','save_retried','persist_muted']
    source=Path(__file__).resolve().parents[2]/'desktop/examples/volume_ui_probe.js'
    reports=[]
    for reopened,name,stages in [(False,'first',labels),(True,'reopen',['reopened_muted','reopened_unmuted','reopened_remuted'])]:
        script=root/f'{name}.js';script.write_text('window.__volumeReopen='+json.dumps(reopened)+';\n'+source.read_text(encoding='utf-8'),encoding='utf-8')
        env['CUMULUS_UX_SCRIPT']=str(script)
        if reopened:env['CUMULUS_PROOF_REOPEN']='1'
        with (root/f'{name}.log').open('x',encoding='utf-8') as log:code=launch(args.binary.resolve(),env,log,root/name,args.screenshots)
        failed=root/name/'failed.json'
        assert not failed.exists(),failed.read_text(encoding='utf-8') if failed.exists() else ''
        assert code==0,f'volume UI exited {code}'
        complete=json.loads((root/name/'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['stageCount']==len(stages) and complete['volumeUiVerified']
        assert complete['initialKeyboardEntryVerified']==(not reopened) and complete['keyboardReentryVerified']==(not reopened)
        assert complete['accountWrites']==complete['realMusicRequests']==0
        for index,label in enumerate(stages,1):
            path=root/name/f'ux_{index:02}_{label}.json';row=json.loads(path.read_text(encoding='utf-8'));native=row['nativePlayer']
            assert row['contained'] and row['triggerHit']
            assert native['state']=='paused' and abs(native['position']-10)<.2 and not native['error']
            assert native['volume']==row['volume'] and native['unmuteVolume']==row['unmuteVolume']
            assert native['playId']==row['playId'] and native['mode']==row['mode']
            if row['popup']:assert row['popup']['sliderHit']
            if args.screenshots:assert path.with_suffix('.png').is_file()
        reports.append({'ok':True,'actualWebView':True,'volumeUiVerified':True,'states':len(stages),'reopened':reopened,'screenshots':len(stages) if args.screenshots else 0,'realAccountWrites':0,'realMusicRequests':0,'physicalAudioOutputVerified':False,'physicalDisplayVerified':False})
    (root/'audit.json').write_text(json.dumps(reports,indent=2)+'\n',encoding='utf-8');print(json.dumps(reports))


if __name__=='__main__':main()
