"""Security email PIN/limits/cancellation in the actual production WebView."""
import argparse
import json
import os
import subprocess
import sys
from pathlib import Path
from window_manager import launch


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=Path,required=True)
    parser.add_argument('--root',type=Path,required=True)
    parser.add_argument('--runtime',type=Path)
    parser.add_argument('--legacy',action='store_true')
    parser.add_argument('--screenshots',action='store_true')
    args=parser.parse_args();root=args.root.resolve();root.mkdir(parents=True,exist_ok=False)
    env={k:v for k,v in os.environ.items()if not k.startswith('CUMULUS_')};env['CUMULUS_PROOF_DIR']=str(root/'run')
    if args.legacy:env['CUMULUS_PROOF_LEGACY_EMAIL']='1'
    for key,name in [('XDG_DATA_HOME','data'),('XDG_CONFIG_HOME','config'),('XDG_CACHE_HOME','cache'),('APPDATA','roaming'),('LOCALAPPDATA','local')]:
        folder=root/'os-profile'/name;folder.mkdir(parents=True,mode=0o700);env[key]=str(folder)
    if args.runtime:env['PATH']=str(args.runtime.resolve())+os.pathsep+env.get('PATH','')
    output=root/'run/first'
    with(root/'host.log').open('x',encoding='utf-8')as log:code=launch(args.binary.resolve(),env,log,output,args.screenshots)
    failed=output/'failed.json';assert not failed.exists(),failed.read_text(encoding='utf-8')if failed.exists()else''
    assert code==0,code
    complete=json.loads((output/'complete.json').read_text(encoding='utf-8'));assert complete['ok']
    states=[json.loads(p.read_text(encoding='utf-8'))for p in sorted(output.glob('ux_*.json'))]
    assert len(states)==complete['stageCount']==22
    by_name={s['stage'].split('_',2)[2]:s for s in states}
    assert all(s['focusValid']and s['focused']and not s['overflow']and s['realMusicRequests']==0 for s in states)
    assert by_name['editing_light']['codeLength']==by_name['draft_light']['codeLength']==3
    assert by_name['editing_dark']['codeLength']==1
    assert all(s['codeLength']==0 for s in states if s['stage'].split('_',2)[2]not in ['editing_light','editing_dark','draft_light'])
    path='/api/user/2fa/securityEmail/'
    count=lambda name,route:by_name[name]['fixture']['paths'].get(path+route,0)
    assert count('ready_light','send')==1 and count('ready_light','verify')==0
    assert count('editing_light','verify')==0
    assert count('wrong_light','verify')==1 and count('business_false_dark','verify')==2
    assert count('verify_limit_small_dark','verify')==3
    assert count('resent_small_light','send')==2
    assert by_name['send_limit_small_light']['pinCount']==0
    assert count('send_unknown_small_dark','send')==count('unknown_send_authorized','send')
    assert count('verify_unknown_light','verify')==count('reconciled_light','verify')
    assert by_name['send_inflight_dark']['fixture']['pending']
    assert by_name['closed_send_dropped_dark']['fixture']['dropped']==1
    assert by_name['verify_inflight_dark']['fixture']['pending']
    assert by_name['stopped_verify_dark']['fixture']['dropped']==2
    assert complete['fixture']['dropped']==2
    assert sum(v for p,v in complete['fixture']['paths'].items()if p.endswith('/cancel'))==0
    if args.screenshots:assert len(list(output.glob('ux_*.png')))==22
    audit=Path(__file__).resolve().parents[2]/'desktop/examples/audit_email_probe.py'
    subprocess.run([sys.executable,str(audit),str(root/'run'),*(['--legacy']if args.legacy else[])],check=True)
    report=dict(ok=True,actualWebView=True,states=22,screenshots=22 if args.screenshots else 0,realMusicRequests=0,realAccountWrites=0,legacy=args.legacy)
    (root/'report.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8');print(json.dumps(report))


if __name__=='__main__':main()
