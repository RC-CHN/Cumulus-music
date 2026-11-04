"""Actual settings WebView and native audio filter, using isolated synthetic files."""
import argparse
from contextlib import closing
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import time


def launch(binary, env, log, output, screenshots):
    if screenshots:
        import gi
        gi.require_version('Gdk', '3.0')
        from gi.repository import Gdk
    process = subprocess.Popen([str(binary)], env=env, stdout=log, stderr=log)
    seen = set()
    try:
        deadline = time.monotonic() + 180
        while process.poll() is None:
            if time.monotonic() > deadline:
                raise TimeoutError('loudness WebView timed out')
            if screenshots:
                for stage in output.glob('loudness_*.json'):
                    if stage in seen:
                        continue
                    window = Gdk.get_default_root_window()
                    picture = Gdk.pixbuf_get_from_window(window, 0, 0, window.get_width(), window.get_height())
                    assert picture is not None
                    picture.savev(str(stage.with_suffix('.png')), 'png', [], [])
                    seen.add(stage)
            time.sleep(.08)
        assert not (output / 'failed.json').exists(), (output / 'failed.json').read_text(encoding='utf-8') if (output / 'failed.json').exists() else ''
        assert process.returncode == 0, f'probe exited {process.returncode}'
        return len(seen)
    finally:
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()


def read_closed_loudness_backups(path):
    """Inspect the synthetic database only after its probe has exited."""
    def read(mode):
        with closing(sqlite3.connect(path.as_uri() + f'?mode={mode}', uri=True)) as db:
            db.execute('PRAGMA query_only = ON')
            return db.execute(
                "SELECT value FROM state WHERE owner='local-device' AND kind='loudness-recovery-v1'"
            ).fetchall()

    try:
        return read('ro')
    except sqlite3.OperationalError as error:
        if error.sqlite_errorcode != sqlite3.SQLITE_READONLY_ROLLBACK:
            raise
    # A probe can exit with an interrupted background transaction. Let SQLite
    # recover its hot journal in the existing synthetic file; query_only still
    # blocks SQL writes. Reopen read-only to verify the original proof database
    # is now independently readable by the strict diagnostics verifier too.
    read('rw')
    return read('ro')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--filter-library', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--runtime-fault', action='store_true', help='verify typed GEQ and loudness faults during native Pause')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    base = {key:value for key,value in os.environ.items() if not key.startswith('CUMULUS_')}
    if args.runtime:
        base['PATH'] = str(args.runtime.resolve()) + os.pathsep + base.get('PATH','')
    audit = []
    modes = [('runtime_fault', 6)] if args.runtime_fault else [('normal',10), ('reopen',2), ('missing',4), ('damaged',3)]
    for mode, expected in modes:
        group = 'normal' if mode == 'reopen' else mode
        directory = root / group
        directory.mkdir(exist_ok=mode == 'reopen')
        env = dict(base)
        env['CUMULUS_PROOF_DIR'] = str(directory / 'run')
        env['CUMULUS_LOUDNESS_CASE'] = mode
        env['CUMULUS_AUDIO_FILTER'] = str(root / 'missing-filter' if mode == 'missing' else args.filter_library.resolve(strict=True))
        for variable, subdir in [('XDG_DATA_HOME','data'),('XDG_CONFIG_HOME','config'),('XDG_CACHE_HOME','cache'),('APPDATA','roaming'),('LOCALAPPDATA','local')]:
            path = directory / 'os-profile' / subdir
            path.mkdir(parents=True, exist_ok=mode == 'reopen')
            env[variable] = str(path)
        output = directory / 'run' / mode
        with (directory / f'{mode}.log').open('x', encoding='utf-8') as log:
            captures = launch(args.binary.resolve(strict=True), env, log, output, args.screenshots)
        complete = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['stageCount'] == expected
        assert complete['actualDecoder'] and complete['realMusicRequests'] == 0
        assert not complete['physicalAudioOutputVerified'] and not complete['physicalDisplayVerified']
        stages = [json.loads(path.read_text(encoding='utf-8')) for path in sorted(output.glob('loudness_*.json'))]
        assert len(stages) == expected and all(stage['contained'] and stage['hittable'] for stage in stages)
        assert not args.screenshots or captures == expected
        assert all(stage['native']['loudness']['status'] == stage['expect']['status'] for stage in stages)
        if mode == 'normal':
            assert {(s['width'],s['height'],s['theme']) for s in stages if 'processing_' in s['stage']} == {(1120,760,'light'),(1120,760,'dark'),(760,540,'dark'),(760,540,'light')}
        if mode == 'damaged':
            backups = read_closed_loudness_backups(directory / 'run/state/core.sqlite3')
            assert len(backups) == 1 and json.loads(backups[0][0])['current']['enabled'] == 'broken'
        if mode == 'runtime_fault':
            wet, paused, resumed, reapplied = (stage['native'] for stage in stages[2:])
            assert wet['state'] == resumed['state'] == reapplied['state'] == 'playing' and paused['state'] == 'paused'
            assert wet['equalizer']['status'] == reapplied['equalizer']['status'] == 'processing'
            assert paused['equalizer']['status'] == resumed['equalizer']['status'] == 'failed'
            assert all(stage['playId'] == wet['playId'] and stage['entryId'] == wet['entryId']
                       and stage['error'] is None for stage in (paused, resumed, reapplied))
            assert wet['position'] < paused['position'] < resumed['position'] < reapplied['position']
        audit.append({'mode':mode,'states':expected,'screenshots':captures})
    result = {'ok':True,'actualWebView':True,'actualDecoder':True,'audioOutput':'null','physicalDisplayVerified':False,'physicalAudioOutputVerified':False,'realMusicRequests':0,'cases':audit}
    (root / 'audit.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
    print(json.dumps(result))


if __name__ == '__main__':
    main()
