"""Actual WebView original-device verification, synthetic native EAPI only."""
import argparse
import json
import os
from pathlib import Path
from window_manager import launch

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--binary',type=Path,required=True)
    p.add_argument('--root',type=Path,required=True)
    p.add_argument('--runtime',type=Path)
    p.add_argument('--screenshots',action='store_true')
    p.add_argument('--legacy',action='store_true')
    args=p.parse_args();root=args.root.resolve();root.mkdir(parents=True,exist_ok=False)
    env={k:v for k,v in os.environ.items() if not k.startswith('CUMULUS_')};env['CUMULUS_PROOF_DIR']=str(root/'run')
    if args.legacy:env['CUMULUS_PROOF_LEGACY_DEVICE']='1'
    for k,n in [('XDG_DATA_HOME','data'),('XDG_CONFIG_HOME','config'),('XDG_CACHE_HOME','cache'),('APPDATA','roaming'),('LOCALAPPDATA','local')]:
        d=root/'os-profile'/n;d.mkdir(parents=True,mode=0o700);env[k]=str(d)
    if args.runtime:env['PATH']=str(args.runtime.resolve())+os.pathsep+env.get('PATH','')
    output=root/'run/first'
    with (root/'host.log').open('x',encoding='utf-8') as log:code=launch(args.binary.resolve(),env,log,output,args.screenshots)
    failed=output/'failed.json';assert not failed.exists(),failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code==0,code
    complete=json.loads((output/'complete.json').read_text(encoding='utf-8'));assert complete['ok']
    states=[json.loads(f.read_text(encoding='utf-8')) for f in sorted(output.glob('ux_*.json'))]
    assert len(states)==complete['stageCount']==39
    assert all(not s['overflow'] and s['focusValid'] and s['focused'] and s['realMusicRequests']==0 for s in states)
    by_name={s['stage'].split('_',2)[2]:s for s in states}
    assert by_name['binding_mobile_handoff']['fixture']['paths']==by_name['methods_light']['fixture']['paths']
    assert by_name['scan_inflight']['fixture']['pending']
    if not args.legacy:
        assert by_name['stopped_inflight']['fixture']['pending']
        assert sum(v for k,v in complete['fixture']['paths'].items() if k.endswith('/cancel'))==0
    else:
        assert sum(v for k,v in complete['fixture']['paths'].items() if k.endswith('/cancel'))>0
    assert by_name['late_authorization_ignored']['fixture']['completed']==2
    assert by_name['confirmed_account']['nativeAccount']['epoch']==by_name['start_unknown']['nativeAccount']['epoch']
    assert by_name['confirmation_reconciled']['nativeAccount']['epoch']==by_name['late_authorization_ignored']['nativeAccount']['epoch']
    assert by_name['confirmation_unknown']['codeLength']==0
    assert by_name['pin_partial_keyboard']['codeLength']==3
    assert by_name['pin_waits_for_read']['codeLength']==0
    assert by_name['pin_waits_for_read']['fixture']['paths'].get('/api/login/origin-device/apply/check',0)==0
    assert by_name['confirm_resent']['fixture']['deviceStarts']==by_name['confirm_expired']['fixture']['deviceStarts']+1
    assert by_name['scan_refreshed']['fixture']['deviceStarts']==by_name['scan_expired']['fixture']['deviceStarts']+1
    assert by_name['application_inflight']['fixture']['pendingStart']
    assert by_name['closed_application_inflight']['fixture']['pendingStart']
    assert by_name['late_application_ignored']['fixture']['completedStarts']==1
    assert complete['fixture']['paths']['/api/login/origin-device/apply/check']==4
    if args.screenshots:assert len(list(output.glob('ux_*.png')))==len(states)
    report=dict(ok=True,actualWebView=True,states=len(states),screenshots=len(states) if args.screenshots else 0,realMusicRequests=0,realAccountWrites=0)
    (root/'report.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8');print(json.dumps(report))
if __name__=='__main__':main()
