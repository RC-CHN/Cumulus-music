"""Exercise live equalizer controls and durable state in two production WebViews."""
import argparse
import json
import os
import sqlite3
from contextlib import closing
from pathlib import Path
import subprocess


def run(binary, env, root, reopen=False):
    output = root/'run'/('reopen' if reopen else 'first')
    if reopen:
        env = {**env, 'CUMULUS_PROOF_REOPEN': '1'}
    with (root/('reopen.log' if reopen else 'host.log')).open('x', encoding='utf-8') as log:
        completed = subprocess.run([str(binary)], env=env, stdout=log, stderr=log,
                                   timeout=120, check=False)
    failed = output/'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert completed.returncode == 0
    return lambda name: json.loads((output/f'{name}.json').read_text(encoding='utf-8'))


def persisted(root):
    database = root/'run/state/core.sqlite3'
    with closing(sqlite3.connect(database.as_uri()+'?mode=ro', uri=True)) as store:
        rows = store.execute(
            "SELECT owner,value FROM state WHERE kind='equalizer-preferences-v1' AND key='current'"
        ).fetchall()
    assert len(rows) == 1 and rows[0][0] == 'local-guest'
    return json.loads(rows[0][1])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--filter-library', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env['CUMULUS_PROOF_DIR'] = str(root/'run')
    env['CUMULUS_AUDIO_FILTER'] = str(args.filter_library.resolve(strict=True))
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'APPDATA', 'LOCALAPPDATA'):
        directory = root/'os-profile'/key
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    binary = args.binary.resolve(strict=True)
    read = run(binary, env, root)
    custom = read('eq_custom')
    native = custom['native']['equalizer']
    assert custom['status'] == native['status'] == 'processing'
    assert native['settings']['enabled'] and native['settings']['gains'][4] == -3.5
    before = read('eq_enabled')['native']
    after = custom['native']
    assert before['state'] == after['state'] == 'playing'
    assert before['entryId'] == after['entryId'] and after['position'] > before['position']
    same_saved = read('eq_same_curve_saved')['native']['equalizer']
    same_builtin = read('eq_same_curve_builtin')['native']['equalizer']
    assert same_saved['preset'] == {'kind': 'saved'}
    assert same_builtin['preset'] == {'kind': 'built_in', 'index': 1}
    assert same_saved['settings'] == same_builtin['settings'] == before['equalizer']['settings']
    assert same_saved['saved_custom'] == same_builtin['saved_custom'] == same_builtin['settings']['gains']
    assert same_saved['status'] == same_builtin['status'] == 'processing'
    custom_saved = read('eq_custom_saved')['native']
    built_in = read('eq_custom_built_in')['native']
    recalled = read('eq_custom_recalled')['native']
    custom_gains = native['settings']['gains']
    assert int(custom_saved['equalizer']['revision']) == int(native['revision']) + 1
    assert native['preset'] == {'kind': 'manual'}
    assert custom_saved['equalizer']['preset'] == {'kind': 'saved'}
    assert built_in['equalizer']['preset'] == {'kind': 'built_in', 'index': 1}
    assert recalled['equalizer']['preset'] == {'kind': 'saved'}
    assert custom_saved['equalizer']['settings'] == native['settings']
    assert built_in['equalizer']['settings'] != native['settings']
    assert recalled['equalizer']['settings'] == native['settings']
    assert recalled['equalizer']['status'] == 'processing'
    for stage in (custom_saved, built_in, recalled):
        assert stage['equalizer']['saved_custom'] == custom_gains
        assert stage['entryId'] == before['entryId'] and stage['state'] == 'playing'
    off = read('eq_off')['native']
    reenabled = read('eq_reenabled')['native']
    flat = read('eq_flat')['native']
    assert off['equalizer']['settings'] == {**native['settings'], 'enabled': False}
    assert reenabled['equalizer']['settings'] == native['settings']
    assert reenabled['equalizer']['status'] == 'processing'
    assert flat['equalizer']['settings'] == {'enabled': True, 'gains': [0] * 10}
    assert flat['equalizer']['status'] in ('configured', 'processing')
    progression = [before, after, off, reenabled, flat]
    assert all(stage['state'] == 'playing' and stage['entryId'] == before['entryId']
               for stage in progression)
    assert all(b['position'] > a['position'] and
               int(b['equalizer']['revision']) > int(a['equalizer']['revision'])
               for a, b in zip(progression, progression[1:]))
    tone = read('eq_tone')
    tone_settings = tone['native']['equalizer']['settings']
    assert tone['tone'] == [6, -3]
    assert (tone_settings['bass_db'], tone_settings['treble_db']) == (6, -3)
    assert tone['native']['equalizer']['status'] == 'processing'
    assert tone['native']['entryId'] == before['entryId'] and tone['native']['state'] == 'playing'
    saved = read('eq_saved')['native']
    assert saved['equalizer']['settings'] == tone_settings
    assert tone_settings['gains'][4] == 4
    assert saved['state'] == 'paused' and saved['position'] >= 11
    assert read('complete')['ok']
    record = persisted(root)
    assert record == {'revision': int(saved['equalizer']['revision']),
                      'settings': saved['equalizer']['settings'],
                      'preset': {'kind': 'built_in', 'index': 1},
                      'saved_custom': native['settings']}

    reopened = run(binary, env, root, reopen=True)
    restored = reopened('eq_reopened')['native']
    resumed = reopened('eq_reopened_playing')['native']
    assert restored['state'] == 'paused' and restored['position'] >= 11
    assert resumed['state'] == 'playing' and resumed['position'] > restored['position']
    assert resumed['equalizer']['status'] == 'processing'
    for stage in (restored, resumed):
        assert stage['equalizer']['settings'] == record['settings']
        assert stage['equalizer']['preset'] == record['preset']
        assert int(stage['equalizer']['revision']) == record['revision']
        assert stage['equalizer']['saved_custom'] == custom_gains
    recalled_reopen = reopened('eq_reopened_custom_recalled')['native']
    assert recalled_reopen['state'] == 'playing' and recalled_reopen['entryId'] == resumed['entryId']
    assert recalled_reopen['equalizer']['status'] == 'processing'
    assert recalled_reopen['equalizer']['saved_custom'] == custom_gains
    assert recalled_reopen['equalizer']['settings'] == {
        **record['saved_custom'], 'bass_db': 6, 'treble_db': -3,
    }
    assert int(recalled_reopen['equalizer']['revision']) == record['revision'] + 1
    reset = reopened('eq_reopened_tone_reset')
    assert reset['tone'] == [0, 0]
    assert reset['native']['entryId'] == resumed['entryId'] and reset['native']['state'] == 'playing'
    assert reset['native']['equalizer']['settings'] == record['saved_custom']
    assert persisted(root) == {**record, 'revision': record['revision'] + 2,
                               'settings': record['saved_custom'], 'preset': {'kind': 'saved'}}
    assert reopened('complete')['ok']
    report = {'ok': True, 'actualWebView': True, 'inputOnly': True, 'confirmedGain': -3.5,
              'samePlayingEntryAdvanced': True, 'nativeFlatReset': True,
              'persistedRevisionVerified': True, 'freshProcessResumedProcessing': True,
              'savedCustomPresetRecalledAfterRestart': True,
              'sameCurvePresetIdentityPreserved': True,
              'toneProcessedPersistedAndReset': True}
    (root/'report.json').write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
