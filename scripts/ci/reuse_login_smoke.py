"""Actual WebView phone-reuse consent, registration and durable recovery."""
import argparse
import json
import os
import subprocess
import sys
from pathlib import Path
from window_manager import launch


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--binary',type=Path,required=True);p.add_argument('--root',type=Path,required=True);p.add_argument('--runtime',type=Path);p.add_argument('--legacy',action='store_true');p.add_argument('--screenshots',action='store_true');args=p.parse_args()
    root=args.root.resolve();root.mkdir(parents=True,exist_ok=False)
    env={k:v for k,v in os.environ.items() if not k.startswith('CUMULUS_')};env['CUMULUS_PROOF_DIR']=str(root/'run')
    if args.legacy:env['CUMULUS_PROOF_LEGACY_REUSE']='1'
    for key,name in [('XDG_DATA_HOME','data'),('XDG_CONFIG_HOME','config'),('XDG_CACHE_HOME','cache'),('APPDATA','roaming'),('LOCALAPPDATA','local')]:
        folder=root/'os-profile'/name;folder.mkdir(parents=True,mode=0o700);env[key]=str(folder)
    if args.runtime:env['PATH']=str(args.runtime.resolve())+os.pathsep+env.get('PATH','')
    output=root/'run/first'
    with(root/'host.log').open('x',encoding='utf-8')as log:code=launch(args.binary.resolve(),env,log,output,args.screenshots)
    failed=output/'failed.json';assert not failed.exists(),failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code==0,code
    complete=json.loads((output/'complete.json').read_text(encoding='utf-8'));assert complete['ok']
    states=[json.loads(f.read_text(encoding='utf-8'))for f in sorted(output.glob('ux_*.json'))];assert len(states)==complete['stageCount']==34
    assert all(s['focusValid']and s['focused']and not s['overflow']and s['realMusicRequests']==0 for s in states)
    by={s['stage'].split('_',2)[2]:s for s in states}
    assert by['choice_light']['avatarWidth']==60 and by['choice_light']['choiceCard']=={'width':300,'height':50}
    assert by['login_consent_light']['primaryWidth']==by['register_consent_light']['primaryWidth']==220
    paths=lambda name:by[name]['fixture']['paths'];confirm='/api/login/cellphone/twice/used/confirm';register='/api/w/register/cellphone'
    assert paths('login_consent_light').get(confirm,0)==0
    assert paths('register_consent_light').get(register,0)==paths('nickname_taken_dark').get(register,0)==0
    assert paths('registration_unknown_small_light')[register]==paths('registration_reconciled_light')[register]
    assert paths('register_inflight_dark')[register]==paths('retry_register_blocked_dark')[register]==paths('fresh_sms_resolved_light')[register]
    assert by['retry_register_blocked_dark']['blocked']
    assert by['closed_register_dropped_dark']['fixture']['dropped']==1
    assert by['closed_read_dropped_dark']['fixture']['dropped']==2
    assert by['returned_login_light']['fixture']['dropped']==3
    assert by['closed_reconcile_dropped_light']['fixture']['dropped']==complete['fixture']['dropped']==4
    assert by['invalid_sms_nickname_light']['blocked'] and by['invalid_sms_nickname_light']['nicknameLength']==0
    assert complete['fixture']['paths'][register]==5 and complete['fixture']['paths'][confirm]==4
    if args.screenshots:assert len(list(output.glob('ux_*.png')))==34
    audit=Path(__file__).resolve().parents[2]/'desktop/examples/audit_reuse_probe.py';subprocess.run([sys.executable,str(audit),str(root/'run')],check=True)
    result=dict(ok=True,actualWebView=True,states=34,screenshots=34 if args.screenshots else 0,realMusicRequests=0,realAccountWrites=0,legacy=args.legacy)
    (root/'report.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8');print(json.dumps(result))


if __name__=='__main__':main()
