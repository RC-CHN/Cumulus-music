"""Production quality menu, consent/reconciliation dialog and native playback."""
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
    env.update(CUMULUS_PROOF_DIR=str(root),CUMULUS_MOTION_PROOF='1',CUMULUS_QUALITY_UI_PROOF='1',CUMULUS_QUALITY_TRIAL_PROOF='1')
    for key,name in [('XDG_DATA_HOME','data'),('XDG_CONFIG_HOME','config'),('XDG_CACHE_HOME','cache'),('XDG_RUNTIME_DIR','runtime'),('APPDATA','roaming'),('LOCALAPPDATA','local')]:
        directory=root/'os-profile'/name;directory.mkdir(parents=True,mode=0o700);env[key]=str(directory)
    if args.runtime:env['PATH']=str(args.runtime.resolve())+os.pathsep+env.get('PATH','')
    if args.frontend_dist:env['CUMULUS_PROOF_FRONTEND_DIST']=str(args.frontend_dist.resolve())
    first=[f'{stage}_{width}_{theme}' for width,theme in [(1120,'light'),(760,'light'),(760,'dark'),(1120,'dark')] for stage in ['menu','confirm','cancel','member']]
    first+=['reading_metadata','partial_metadata','partial_notice','unavailable_assets','journal_damaged_menu','journal_damaged_notice','member_navigation','consent_pending','activated','active_menu','expired','playback_denied','playback_retried','retry_expired','lost_response','lost_reopened_menu','lost_reopened_notice','reconciled','active_retried']
    source=Path(__file__).resolve().parents[2]/'desktop/examples/quality_rights_ui_probe.js'
    reports=[]
    for reopened,name,labels in [(False,'first',first),(True,'reopen',['reopened_active','reopened_menu','reopened_expired'])]:
        script=root/f'{name}.js';script.write_text('window.__rightsReopen='+json.dumps(reopened)+';\n'+source.read_text(encoding='utf-8'),encoding='utf-8')
        env['CUMULUS_UX_SCRIPT']=str(script)
        if reopened:env['CUMULUS_PROOF_REOPEN']='1'
        with (root/f'{name}.log').open('x',encoding='utf-8') as log:code=launch(args.binary.resolve(),env,log,root/name,args.screenshots)
        failed=root/name/'failed.json'
        assert not failed.exists(),failed.read_text(encoding='utf-8') if failed.exists() else ''
        assert code==0,f'quality rights UI exited {code}'
        complete=json.loads((root/name/'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['stageCount']==len(labels)
        assert complete['consentDialogVerified']==(not reopened) and complete['dynamicRightsVerified']
        assert complete['manualScrollVerified']==(not reopened)
        assert complete['accountWrites']==complete['realMusicRequests']==0
        assert complete['syntheticQuality']['trialWrites']==(0 if reopened else 3)
        for index,label in enumerate(labels,1):
            path=root/name/f'ux_{index:02}_{label}.json';row=json.loads(path.read_text(encoding='utf-8'));player=row['nativePlayer']
            assert row['contained'] and player['state']=='paused' and abs(player['position']-10)<.2 and not player['error']
            assert player['actualQuality']==row['actual']=='standard'
            assert player['track']['id']==row['trackId']
            assert player['requestedQuality']==row['requested']
            assert player['qualityPreferences']['quality']==row['preferred']
            if row['dialog']:assert all(c['hit'] for c in row['dialog']['controls'])
            else:assert row['tagHit']
            if args.screenshots:assert path.with_suffix('.png').is_file()
        reports.append({'ok':True,'actualWebView':True,'consentDialogVerified':not reopened,'manualScrollVerified':not reopened,'dynamicRightsVerified':True,'states':len(labels),'reopened':reopened,'syntheticTrialWrites':0 if reopened else 3,'realAccountWrites':0,'physicalAudioOutputVerified':False,'physicalDisplayVerified':False,'screenshots':len(labels) if args.screenshots else 0})
    (root/'audit.json').write_text(json.dumps(reports,indent=2)+'\n',encoding='utf-8');print(json.dumps(reports))


if __name__=='__main__':main()
