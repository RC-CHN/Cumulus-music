"""Production system-media settings in a real WebView, with synthetic native PCM.

Linux invocation: GDK_BACKEND=x11 dbus-run-session -- xvfb-run -a /usr/bin/python3 ...
The companion mpris_smoke/smtc_smoke independently verify actual OS consumers.
"""
import argparse
import json
import os
from pathlib import Path
import sqlite3
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
    env.update(CUMULUS_PROOF_DIR=str(root),CUMULUS_MOTION_PROOF='1',CUMULUS_SYSTEM_MEDIA_PROOF='1',
               CUMULUS_UX_SCRIPT=str(Path('desktop/examples/system_media_ui_probe.js').resolve()))
    for key,name in [('XDG_DATA_HOME','data'),('XDG_CONFIG_HOME','config'),('XDG_CACHE_HOME','cache'),('XDG_RUNTIME_DIR','runtime'),('APPDATA','roaming'),('LOCALAPPDATA','local')]:
        directory=root/'os-profile'/name
        directory.mkdir(parents=True,mode=0o700)
        env[key]=str(directory)
    if args.runtime:
        env['PATH']=str(args.runtime.resolve())+os.pathsep+env.get('PATH','')
    reports=[]
    for phase,count in [('first',12),('reopen',2)]:
        if phase=='reopen':env['CUMULUS_PROOF_REOPEN']='1'
        output=root/phase
        with (root/f'{phase}.log').open('x',encoding='utf-8') as log:
            result=launch(args.binary.resolve(),env,log,output,args.screenshots)
        if (output/'failed.json').exists():raise AssertionError((output/'failed.json').read_text(encoding='utf-8'))
        assert result==0,result
        complete=json.loads((output/'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['stageCount']==count and complete['realMusicRequests']==0
        snapshots=[json.loads(p.read_text(encoding='utf-8')) for p in sorted(output.glob('ux_*.json'))]
        assert len(snapshots)==count
        baseline=snapshots[0]['audioFactoryCount']
        assert baseline>=1
        assert complete['audioFactoryCount']==baseline,'setting change must not recreate audio'
        if args.screenshots:assert len(list(output.glob('ux_*.png')))==count
        for snapshot in snapshots:
            assert snapshot['systemMediaStatus']['phase']==snapshot['phase']
            if snapshot['checked'] is not None:
                assert snapshot['systemMediaPreferences']['enabled']==snapshot['checked']
            assert snapshot['audioFactoryCount']==baseline
        if phase=='first':
            playing=snapshots[1:]
            assert all(s['nativePlayer']['state']=='playing' for s in playing)
            assert len({s['nativePlayer']['playId'] for s in playing})==1
            assert complete['systemMediaPreferences']['enabled'] is False
        else:
            assert complete['reopened'] and complete['systemMediaPreferences']['enabled'] is True
        reports.append(complete)
    databases=list((root/'state').rglob('core.sqlite3'))
    records=[]
    for path in databases:
        with sqlite3.connect(path) as connection:
            records.extend(connection.execute("SELECT kind,key,value FROM state WHERE owner='local-device' AND kind IN ('system-media-preferences-v1','system-media-recovery-v1')").fetchall())
            assert connection.execute("SELECT count(*) FROM sqlite_master WHERE type='trigger' AND name='reject_media_ui'").fetchone()[0]==0
    backups=[json.loads(value) for kind,key,value in records if kind=='system-media-recovery-v1']
    assert len(backups)==2
    assert any((b.get('current') or {}).get('enabled')=='invalid' for b in backups)
    assert any(b.get('current_raw')=='{broken' and b['current'] is None for b in backups)
    result={'ok':True,'actualWebView':True,'states':14,'screenshots':14 if args.screenshots else 0,
            'malformedJsonRecoveryVerified':True,
            'nativePlaybackContinues':True,'preferenceRestartVerified':True,'realMusicRequests':0,
            'physicalAudioOutputVerified':False,'physicalShellPanelVerified':False}
    (root/'report.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
    print(json.dumps(result))


if __name__=='__main__':main()
