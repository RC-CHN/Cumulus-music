"""Native quality-trial IPC/expiry and actual decoder; consent UI is separate."""
import argparse
import json
import os
from pathlib import Path
from motion_smoke import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k:v for k,v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root),CUMULUS_MOTION_PROOF='1',
               CUMULUS_QUALITY_UI_PROOF='1',CUMULUS_QUALITY_TRIAL_PROOF='1')
    for key,name in [('XDG_DATA_HOME','data'),('XDG_CONFIG_HOME','config'),('XDG_CACHE_HOME','cache'),
                     ('XDG_RUNTIME_DIR','runtime'),('APPDATA','roaming'),('LOCALAPPDATA','local')]:
        directory=root/'os-profile'/name
        directory.mkdir(parents=True,mode=0o700)
        env[key]=str(directory)
    if args.runtime:
        env['PATH']=str(args.runtime.resolve())+os.pathsep+env.get('PATH','')
    source=Path(__file__).resolve().parents[2]/'desktop/examples/quality_trial_native_probe.js'
    reports=[]
    for reopened,name,count in [(False,'first',6),(True,'reopen',2)]:
        script=root/f'{name}.js'
        script.write_text('window.__trialReopen='+json.dumps(reopened)+';\n'+source.read_text(encoding='utf-8'),encoding='utf-8')
        env['CUMULUS_UX_SCRIPT']=str(script)
        if reopened:env['CUMULUS_PROOF_REOPEN']='1'
        with (root/f'{name}.log').open('x',encoding='utf-8') as log:
            code=launch(args.binary.resolve(),env,log,root/name,args.screenshots)
        failed=root/name/'failed.json'
        assert not failed.exists(),failed.read_text(encoding='utf-8') if failed.exists() else ''
        assert code==0,f'trial native probe exited {code}'
        complete=json.loads((root/name/'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['stageCount']==count and complete['nativeTrialIpcVerified']
        assert not complete['consentDialogVerified']
        assert complete['accountWrites']==complete['realMusicRequests']==0
        assert complete['syntheticQuality']['trialWrites']==(0 if reopened else 2)
        stages=[json.loads(p.read_text(encoding='utf-8')) for p in sorted((root/name).glob('ux_*.json'))]
        assert len(stages)==count
        for row in stages:
            player=row['nativePlayer']
            assert player['state']=='paused' and abs(player['position']-10)<.2 and not player['error']
            assert player['actualQuality']==row['actual']=='standard'
            assert player['requestedQuality']==row['requested']
            assert player['qualityPreferences']['quality']==row['preferred']
        if args.screenshots:assert len(list((root/name).glob('ux_*.png')))==count
        reports.append({'ok':True,'actualWebView':True,'nativeTrialIpcVerified':True,
                        'consentDialogVerified':False,'states':count,'reopened':reopened,
                        'syntheticTrialWrites':0 if reopened else 2,'realAccountWrites':0,
                        'physicalAudioOutputVerified':False,'physicalDisplayVerified':False})
    (root/'audit.json').write_text(json.dumps(reports,indent=2)+'\n',encoding='utf-8')
    print(json.dumps(reports))


if __name__=='__main__':main()
