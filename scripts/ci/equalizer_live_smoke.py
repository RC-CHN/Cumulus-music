"""Exercise input-only equalizer adjustments in the production WebView."""
import argparse
import json
import os
from pathlib import Path
import subprocess


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
    with (root/'host.log').open('x', encoding='utf-8') as log:
        completed = subprocess.run([str(args.binary.resolve(strict=True))], env=env,
                                   stdout=log, stderr=log, timeout=120, check=False)
    output = root/'run/first'
    failed = output/'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert completed.returncode == 0
    read = lambda name: json.loads((output/f'{name}.json').read_text(encoding='utf-8'))
    custom = read('eq_custom')
    native = custom['native']['equalizer']
    assert custom['status'] == native['status'] == 'processing'
    assert native['settings']['enabled'] and native['settings']['gains'][4] == -3.5
    before = read('eq_enabled')['native']
    after = custom['native']
    assert before['state'] == after['state'] == 'playing'
    assert before['entryId'] == after['entryId'] and after['position'] > before['position']
    assert read('eq_off')['native']['equalizer']['settings']['enabled'] is False
    assert read('eq_saved')['native']['equalizer']['settings']['gains'][4] == 4
    assert read('complete')['ok']
    report = {'ok': True, 'actualWebView': True, 'inputOnly': True, 'confirmedGain': -3.5,
              'samePlayingEntryAdvanced': True}
    (root/'report.json').write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
