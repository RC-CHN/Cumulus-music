"""Official cloud song columns, row operations, local mapping and capacity UI."""
import argparse
import json
import os
from pathlib import Path
from window_manager import launch
LABELS = ['catalog_light','catalog_dark','title_resized_dark','pointer_resized_dark',
          'resize_cancelled_dark','sorted_resized_dark','tabs_restored_dark',
          'pointer_selected_light','range_selected_light','range_subtracted_light',
          'all_page_selected_dark','edge_keyboard_dark','delete_keyboard_dark',
          'keyboard_cancel_dark','keyboard_played_dark','public_menu_light',
          'mapped_menu_light','mapped_opened_light','changed_file_menu_light',
          'damaged_local_menu_dark','damaged_local_played_dark','capacity_full_dark',
          'capacity_unknown_light','capacity_terabyte_light','capacity_open_failed_dark',
          'capacity_opened_dark','empty_library_light','catalog_small_light',
          'catalog_small_dark','menu_small_dark','capacity_pending_dark','account_replaced_light']


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--binary',type=Path,required=True)
    p.add_argument('--root',type=Path,required=True)
    p.add_argument('--runtime',type=Path)
    p.add_argument('--screenshots',action='store_true')
    args=p.parse_args();root=args.root.resolve();root.mkdir(parents=True,exist_ok=False)
    env={k:v for k,v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root),CUMULUS_CLOUD_UI_PROOF='1',CUMULUS_CLOUD_MENU_UI_PROOF='1',CUMULUS_CLOUD_BATCH_UI_PROOF='1',CUMULUS_MINI_SCRIPT=str(Path(__file__).resolve().parents[2]/'desktop/examples/cloud_song_probe.js'))
    for key,name in [('XDG_DATA_HOME','data'),('XDG_CONFIG_HOME','config'),('XDG_CACHE_HOME','cache'),('XDG_RUNTIME_DIR','runtime'),('APPDATA','roaming'),('LOCALAPPDATA','local')]:
        path=root/'os-profile'/name;path.mkdir(parents=True,mode=0o700);env[key]=str(path)
    if args.runtime:env['PATH']=str(args.runtime.resolve())+os.pathsep+env.get('PATH','')
    with (root/'host.log').open('x',encoding='utf-8') as log:code=launch(args.binary.resolve(),env,log,root/'first',args.screenshots)
    failed=root/'first/failed.json'
    assert not failed.exists(),failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code==0,f'cloud song exit {code}'
    complete=json.loads((root/'first/complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['cloudSongUi'] and complete['stageCount']==len(LABELS)
    states={}
    for i,label in enumerate(LABELS,1):
        path=root/'first'/f'ux_{i:02}_{label}.json';state=json.loads(path.read_text(encoding='utf-8'));states[label]=state
        assert state['theme']==('dark' if label.endswith('_dark') else 'light')
        assert not state['overflow'] and state['focusInside'] and len(state['rows'])<=200
        assert all(b['hit'] for b in state['buttons'])
        if args.screenshots:assert path.with_suffix('.png').is_file()
    assert len(states['all_page_selected_dark']['selected'])==200
    assert len(states['range_selected_light']['selected'])==3
    assert len(states['range_subtracted_light']['selected'])==1
    assert complete['browserCalls']==3 and complete['directoryCalls']==1
    assert complete['syntheticCloud']['writes']==[] and complete['syntheticCloud']['collectWrites']==[]
    physical=root/'first/physical.json'
    if physical.is_file():
        capture=json.loads(physical.read_text(encoding='utf-8'));assert capture['ok'] and capture['states']==len(states)
    report=dict(ok=True,states=len(states),screenshots=len(states) if args.screenshots else 0,actualWebViews=1,actualScannedMediaAndMapping=True,actualFfprobe=True,serviceSynthetic=True,osBrowserInjected=True,osFileManagerInjected=True,realMusicRequests=0,realAccountWrites=0,physicalDisplayVerified=physical.is_file(),physicalAudioOutputVerified=False)
    (root/'report.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8');print(json.dumps(report))


if __name__=='__main__':main()
