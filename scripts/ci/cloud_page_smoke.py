"""Actual cloud catalog, query/account ownership, modal feedback and native deletion."""
import argparse
import json
import os
from pathlib import Path
from window_manager import launch

LABELS = ['catalog_light','catalog_dark','sort_title_dark','second_page_dark','last_page_light','filtered_light','filtered_playing_light','empty_filter_light','locator_shown_light','locator_centered_light','query_failed_dark','query_recovered_dark','cached_remote_error_dark','remote_recovered_dark','tasks_empty_light','upload_light','upload_small_dark','upload_closed_small_dark','delete_confirm_light','delete_cancel_light','delete_rejected_dark','delete_partial_light','delete_confirmed_light','delete_unknown_dark','delete_unknown_reopened_dark','delete_unknown_read_dark','delete_reconciled_dark','account_replaced_light']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_CLOUD_UI_PROOF='1',
               CUMULUS_MINI_SCRIPT=str(Path(__file__).resolve().parents[2]/'desktop/examples/cloud_page_probe.js'))
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        path = root/'os-profile'/name
        path.mkdir(parents=True, mode=0o700)
        env[key] = str(path)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve())+os.pathsep+env.get('PATH', '')
    with (root/'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, root/'first', args.screenshots)
    failed = root/'first/failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'cloud page process exit {code}'
    complete = json.loads((root/'first/complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['cloudPageUi'] and complete['stageCount'] == len(LABELS)
    assert complete['cloudMetadataFallback'] and complete['writeCount'] == 4
    locator = complete['locatorProof']
    assert locator['sameCatalogIdRejected'] and locator['otherPageRejected'] and locator['playbackUnchanged']
    assert locator['centerError'] <= 1.5 and 2800 <= locator['hideMs'] < 5000
    rows = {}
    for i, label in enumerate(LABELS,1):
        path=root/'first'/f'ux_{i:02}_{label}.json'
        row=json.loads(path.read_text(encoding='utf-8')); rows[label]=row
        assert row['theme'] == ('dark' if label.endswith('_dark') else 'light')
        assert row['factoryCount']==1 and not row['nativePlayer']['error']
        assert not row['overflow'] and row['focusInside']
        assert len(row['rows'])<=200
        if row['uploadButton']: assert row['uploadButton']['width']==row['uploadButton']['height']==48
        assert all(v['box']['left']>=v['cell']['left'] and v['box']['right']<=v['cell']['right'] for v in row['checks'])
        assert all(v['height']==64 for v in row['rows'])
        if row['dialog']: assert all(b['hit'] for b in row['buttons'])
        if args.screenshots: assert path.with_suffix('.png').is_file()
    assert len(rows['catalog_light']['rows'])==200 and len(rows['last_page_light']['rows'])==5
    assert len(rows['filtered_light']['rows'])==1 and rows['empty_filter_light']['empty']
    assert not rows['query_failed_dark']['rows'] and rows['query_failed_dark']['alerts']
    assert rows['cached_remote_error_dark']['rows'] and rows['cached_remote_error_dark']['alerts']
    for name,phase in [('delete_rejected_dark','rejected'),('delete_partial_light','partial'),('delete_unknown_dark','uncertain'),('delete_unknown_reopened_dark','uncertain')]: assert rows[name]['phase']==phase
    assert rows['upload_closed_small_dark']['active']=='上传音乐'
    assert rows['delete_cancel_light']['active']=='删除所选'
    assert rows['delete_confirmed_light']['active']=='搜索云盘歌曲'
    assert not rows['account_replaced_light']['dialog'] and not rows['account_replaced_light']['keyword']
    assert complete['syntheticCloud']['writes']==[['1001','1002'],['1001','1002'],['1002'],['1003','1004']]
    assert complete['syntheticCloud']['realRequests']==0
    assert rows['filtered_playing_light']['nativePlayer']['track']['id']=='1005'
    for name,row in rows.items():
        if name!='account_replaced_light': assert row['nativePlayer']['state']=='paused' and abs(row['nativePlayer']['position']-10)<.2
    report=dict(ok=True,states=len(LABELS),screenshots=len(LABELS) if args.screenshots else 0,actualWebViews=1,syntheticDeleteRequests=4,cloudMetadataFallback=True,locator=locator,realMusicRequests=0,realAccountWrites=0,physicalDisplayVerified=False,physicalAudioOutputVerified=False)
    (root/'report.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8'); print(json.dumps(report))

if __name__ == '__main__': main()
