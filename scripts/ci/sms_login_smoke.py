"""Production SMS login UI/IPC/core with an isolated synthetic HTTP service."""
import argparse
import json
import os
from pathlib import Path
from window_manager import launch

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=Path,required=True)
    parser.add_argument('--root',type=Path,required=True)
    parser.add_argument('--runtime',type=Path)
    parser.add_argument('--screenshots',action='store_true')
    parser.add_argument('--legacy',action='store_true')
    parser.add_argument('--send-failure',action='store_true')
    args=parser.parse_args();root=args.root.resolve();root.mkdir(parents=True,exist_ok=False)
    env={k:v for k,v in os.environ.items() if not k.startswith('CUMULUS_')};env['CUMULUS_PROOF_DIR']=str(root/'run')
    if args.legacy:env['CUMULUS_PROOF_LEGACY_SMS']='1'
    if args.send_failure:env['CUMULUS_PROOF_SMS_SEND_FAILURE']='1'
    for key,name in [('XDG_DATA_HOME','data'),('XDG_CONFIG_HOME','config'),('XDG_CACHE_HOME','cache'),('APPDATA','roaming'),('LOCALAPPDATA','local')]:
        folder=root/'os-profile'/name;folder.mkdir(parents=True,mode=0o700);env[key]=str(folder)
    if args.runtime:env['PATH']=str(args.runtime.resolve())+os.pathsep+env.get('PATH','')
    output=root/'run/first'
    with (root/'host.log').open('x',encoding='utf-8')as log:code=launch(args.binary.resolve(),env,log,output,args.screenshots)
    failed=output/'failed.json';assert not failed.exists(),failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code==0,code
    complete=json.loads((output/'complete.json').read_text(encoding='utf-8'));assert complete['ok']
    states=[json.loads(p.read_text(encoding='utf-8'))for p in sorted(output.glob('ux_*.json'))]
    assert len(states)==complete['stageCount']==(4 if args.send_failure else 25)
    assert all(s['focusValid']and s['focused']and not s['overflow']and s['realMusicRequests']==0 for s in states)
    by_name={s['stage'].split('_',2)[2]:s for s in states}
    path='/api/middle/account/'
    count=lambda name,route:by_name[name]['fixture']['paths'].get(path+route,0)
    if args.send_failure:
        assert complete['fixture']['paths'][path+'captcha/sent']==1
        assert complete['fixture']['paths'].get(path+'captcha/verify',0)==0
        assert by_name['downlink_edited_before_send']['codeLength']==3
        assert all(s['codeLength']==0 for s in states[1:])
        if args.screenshots:assert len(list(output.glob('ux_*.png')))==4
        report=dict(ok=True,actualWebView=True,states=4,screenshots=4 if args.screenshots else 0,realMusicRequests=0,realAccountWrites=0)
        (root/'report.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8');print(json.dumps(report));return
    assert count('uplink_ready_light','uplinkcaptcha/verify')==0
    assert count('uplink_ready_light','captcha/sent')==0
    assert by_name['checking_inflight']['fixture']['pending']
    assert by_name['stopped_inflight']['fixture']['pending']
    assert by_name['late_check_dropped']['fixture']['dropped']==1
    assert count('downlink_alternative','captcha/sent')==0
    assert count('downlink_sent','captcha/sent')==1
    assert count('downlink_wrong_code','captcha/verify')==1
    assert count('downlink_unknown','captcha/verify')==2
    assert by_name['downlink_editing']['codeLength']==3
    for name in ['downlink_wrong_code','downlink_unknown','downlink_inflight','closed_answer_dropped']:assert by_name[name]['codeLength']==0
    assert count('non86_downlink','uplinkcaptcha/get')==count('downlink_reconciled','uplinkcaptcha/get')
    assert count('missing_country_downlink','uplinkcaptcha/get')==count('non86_downlink','uplinkcaptcha/get')
    assert by_name['info_inflight']['fixture']['pending']
    assert by_name['downlink_inflight']['fixture']['pending']
    assert complete['fixture']['dropped']==3
    assert complete['fixture']['paths'][path+'captcha/sent']==1
    assert complete['fixture']['paths'][path+'captcha/verify']==3
    assert sum(v for k,v in complete['fixture']['paths'].items()if k.endswith('/cancel'))==0
    if args.screenshots:assert len(list(output.glob('ux_*.png')))==len(states)
    report=dict(ok=True,actualWebView=True,states=len(states),screenshots=len(states)if args.screenshots else 0,realMusicRequests=0,realAccountWrites=0)
    (root/'report.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8');print(json.dumps(report))
if __name__=='__main__':main()
