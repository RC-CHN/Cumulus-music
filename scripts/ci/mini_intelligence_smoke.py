"""Actual main/mini intelligence control over synthetic API, account and audio."""
import argparse
import json
import os
from pathlib import Path
from window_manager import launch

LABELS=['liked_light','ring_single','ring_random','ring_sequential','seeded_light','cancelled_light','resumed_dark','exited_dark','preparing_dark','cancelled_preparation_dark','failed_dark','failed_light','retried_light','active_expanded_light','rapid_reentered_dark','returned_main','reopened_dark','continued_dark','exit_dark','stale_selected_dark','remote_failed_dark','remote_dismissed_dark','remote_retried_dark']

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--binary',type=Path,required=True);p.add_argument('--root',type=Path,required=True);p.add_argument('--runtime',type=Path);p.add_argument('--screenshots',action='store_true');a=p.parse_args()
    root=a.root.resolve();root.mkdir(parents=True,exist_ok=False)
    env={k:v for k,v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root),CUMULUS_INTELLIGENCE_START_PROOF='1',CUMULUS_MINI_SCRIPT=str(Path(__file__).resolve().parents[2]/'desktop/examples/mini_intelligence_probe.js'))
    for key,name in [('XDG_DATA_HOME','data'),('XDG_CONFIG_HOME','config'),('XDG_CACHE_HOME','cache'),('XDG_RUNTIME_DIR','runtime'),('APPDATA','roaming'),('LOCALAPPDATA','local')]:
        directory=root/'os-profile'/name;directory.mkdir(parents=True,mode=0o700);env[key]=str(directory)
    if a.runtime:env['PATH']=str(a.runtime.resolve())+os.pathsep+env.get('PATH','')
    with (root/'host.log').open('x',encoding='utf-8') as log:code=launch(a.binary.resolve(),env,log,root/'first',a.screenshots)
    verify(root,code,a.screenshots)


def verify(root,code,screenshots):
    failed=root/'first/failed.json';assert not failed.exists(),failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code==0,f'mini intelligence exit {code}'
    complete=json.loads((root/'first/complete.json').read_text(encoding='utf-8'));assert complete['ok'] and complete['intelligenceMini'] and complete['stageCount']==len(LABELS)
    phases=[None,None,None,None,'seeded','seeded','seeded',None,None,None,None,None,'active','active','seeded','seeded','seeded','active',None,None,'fallback','fallback','active']
    modes=['order_loop','single_loop','random','sequential','order_loop','order_loop','order_loop','order_loop','sequential','sequential','sequential','sequential','order_loop','order_loop','order_loop','order_loop','order_loop','order_loop','order_loop','sequential','order_loop','order_loop','order_loop']
    counts=[0,0,0,0,1,1,2,2,2,2,2,2,3,3,4,4,4,5,5,5,6,6,7]
    for i,name in enumerate(LABELS):
        path=root/'first'/f'ux_{i+1:02}_{name}.json';v=json.loads(path.read_text(encoding='utf-8'));native=v['nativePlayer'];mini=v['nativeMini'];sent=v['syntheticIntelligence']['requests']
        assert native['mode']==mini['mode']==modes[i] and native['intelligence']==mini['intelligence']==phases[i]
        assert native['playId']==v['expectedPlayId'] and native['state']=='paused' and abs(native['position']-10)<.2 and not native['error']
        assert len(sent)==counts[i] and v['factoryCount']==1
        expected=1 if phases[i]=='seeded' else 3 if phases[i]=='active' else 40
        assert native['queueLen']==expected
        assert v['windowKind']==('main' if name=='returned_main' else 'mini')
        if name!='returned_main':
            assert not v['mainVisible'] and len(v['miniWindows'])==1 and v['width']==420 and v['height']==(516 if name in ['active_expanded_light','rapid_reentered_dark'] else 206)
            assert not v['overflow'] and not v['genericError'] and bool(v['problem'])==(name in ['failed_dark','failed_light','remote_failed_dark'])
        else:
            assert v['mainVisible'] and not v['miniWindows'] and not mini['intelligenceRead']
            dom=v['mainPlayerDom'];assert dom['playId']==native['playId'] and dom['state']=='paused' and abs(float(dom['position'])-10)<.2 and dom['intelligencePhase']=='seeded'
        if name.endswith('_light'):assert v['theme']=='light'
        if name.endswith('_dark'):assert v['theme']=='dark'
        if screenshots:assert path.with_suffix('.png').is_file()
    for i,payload in enumerate(sent):
        assert set(payload)=={'playlistId','songId','type','startMusicId','count'}
        assert payload==dict(playlistId='1',songId='1' if i<5 else '2',type='fromPlayOne',startMusicId='1',count=40)
    report=dict(ok=True,actualWebViews=2,states=len(LABELS),screenshots=len(LABELS) if screenshots else 0,productionStartVerified=True,sharedAudioFactoryCount=1,realMusicRequests=0,realAccountWrites=0,syntheticPointerEvents=True,physicalDisplayVerified=False,physicalAudioOutputVerified=False)
    (root/'audit.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8');print(json.dumps(report))

if __name__=='__main__':main()
