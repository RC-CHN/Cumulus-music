"""Real UI start/continue/failure with synthetic API/account/audio, libmpv null."""
import argparse
import json
import os
from pathlib import Path
from motion_smoke import launch

LABELS = [f'entry_{width}_{theme}' for width, theme in [(1120,'light'),(760,'light'),(760,'dark'),(1120,'dark')]]
LABELS += ['pending','pending_transport','cancelled','active','exited','failed','restart_narrow']

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=Path,required=True)
    parser.add_argument('--root',type=Path,required=True)
    parser.add_argument('--runtime',type=Path)
    parser.add_argument('--frontend-dist',type=Path)
    parser.add_argument('--screenshots',action='store_true')
    args=parser.parse_args();root=args.root.resolve();root.mkdir(parents=True,exist_ok=False)
    env={k:v for k,v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root),CUMULUS_MOTION_PROOF='1',CUMULUS_QUEUE_UI_PROOF='1',CUMULUS_INTELLIGENCE_START_PROOF='1')
    for key,name in [('XDG_DATA_HOME','data'),('XDG_CONFIG_HOME','config'),('XDG_CACHE_HOME','cache'),('XDG_RUNTIME_DIR','runtime'),('APPDATA','roaming'),('LOCALAPPDATA','local')]:
        directory=root/'os-profile'/name;directory.mkdir(parents=True,mode=0o700);env[key]=str(directory)
    if args.runtime:env['PATH']=str(args.runtime.resolve())+os.pathsep+env.get('PATH','')
    if args.frontend_dist:env['CUMULUS_PROOF_FRONTEND_DIST']=str(args.frontend_dist.resolve())
    env['CUMULUS_UX_SCRIPT']=str(Path(__file__).resolve().parents[2]/'desktop/examples/intelligence_start_probe.js')
    with (root/'host.log').open('x',encoding='utf-8') as log:
        code=launch(args.binary.resolve(),env,log,root/'first',args.screenshots)
    failed=root/'first/failed.json'
    assert not failed.exists(),failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code==0,f'intelligence start exited {code}'
    complete=json.loads((root/'first/complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['intelligenceStart'] and complete['productionStartVerified'] and complete['stageCount']==len(LABELS)
    assert complete['realMusicRequests']==complete['accountWrites']==0
    previous=None
    for index,label in enumerate(LABELS,1):
        path=root/'first'/f'ux_{index:02}_{label}.json';row=json.loads(path.read_text(encoding='utf-8'))
        native,queue=row['nativePlayer'],row['nativeQueue'];requests=row['syntheticIntelligenceStart']['requests']
        phase=None if label.startswith('entry_') or label=='exited' else 'seeded' if label in ['pending','pending_transport','cancelled'] else 'fallback' if label=='failed' else 'active'
        total=0 if label.startswith('entry_') else 1 if phase=='seeded' else 40 if label in ['exited','failed'] else 3
        assert native['intelligence']==row['phase']==phase
        assert native['queueLen']==len(queue['tracks'])==row['total']==total
        assert queue['revision']==native['queueRevision']
        assert not native['error']
        for payload in requests:
            assert set(payload)=={'playlistId','songId','type','startMusicId','count'}
            assert payload['playlistId']==payload['startMusicId']=='1' and payload['count']==40 and payload['type']=='fromPlayAll'
            assert 1<=int(payload['songId'])<=40
        if label=='active':
            assert len(requests)==2 and requests[0]==requests[1]
            assert [r['track']['id'] for r in queue['tracks']]==[row['seed'],'41','42']
        if label in ['pending_transport','cancelled','active','exited']:
            assert native['playId']==row['playId'] and native['state']=='paused' and abs(native['position']-10)<.2
            if previous is not None:assert native['playId']==previous
            previous=native['playId']
        if label=='failed':assert len(requests)==3 and row['message'] and '开启失败' in row['message']
        if label=='restart_narrow':assert len(requests)==4
        if args.screenshots:assert path.with_suffix('.png').is_file()
    report=dict(ok=True,actualWebView=True,states=len(LABELS),screenshots=len(LABELS) if args.screenshots else 0,productionStartVerified=True,syntheticApi=True,realMusicRequests=0,realAccountWrites=0,physicalDisplayVerified=False,physicalAudioOutputVerified=False,syntheticPointerEvents=True)
    (root/'audit.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8');print(json.dumps(report))

if __name__=='__main__':main()
