"""Production WebView: themes, menus, editor focus/layout and process-restart persistence.

The fixture implements a read allowlist and rejects all other music requests.
This verifies UI states, not live account writes or unimplemented service fixtures.
"""
import argparse
import json
import os
from pathlib import Path
from motion_smoke import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true', help='optional local Linux GTK captures')
    args = parser.parse_args()
    root = args.root.resolve()
    assert not root.exists(), 'proof root must be new'
    root.mkdir(parents=True)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env['CUMULUS_PROOF_DIR'] = str(root)
    env['CUMULUS_COVER_LAYOUT_PROOF'] = '1'
    for variable, directory in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                                ('XDG_CACHE_HOME', 'cache'), ('APPDATA', 'roaming'),
                                ('LOCALAPPDATA', 'local')]:
        path = root / 'os-profile' / directory
        path.mkdir(parents=True)
        env[variable] = str(path)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    for run, count in [('first', 39), ('reopen', 2)]:
        if run == 'reopen':
            env['CUMULUS_PROOF_REOPEN'] = '1'
        log_path = root / f'{run}.log'
        output = root / run
        with log_path.open('x', encoding='utf-8') as log:
            returncode = launch(args.binary.resolve(), env, log, output, args.screenshots)
        if (output / 'failed.json').exists():
            raise AssertionError((output / 'failed.json').read_text(encoding='utf-8'))
        assert returncode == 0, f'UX probe exited {returncode}; inspect {log_path}'
        complete = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['stageCount'] == count and complete['realMusicRequests'] == 0
        stages = list(output.glob('ux_*.json'))
        assert len(stages) == count
        if args.screenshots:
            assert len(list(output.glob('ux_*.png'))) == count
        for stage in stages:
            value = json.loads(stage.read_text(encoding='utf-8'))
            assert value['theme'] in ['light', 'dark'] and not value['overflow'], stage.name
        if run == 'first':
            assert all(complete[key] for key in ['menuKeyboard', 'modalFocusRestored',
                       'smallEditorSaveVisible', 'smallEditorActionsVisible', 'equalizerColumnsAligned',
                       'explicitThemeSaved', 'trackActionsContained', 'longDuration',
                       'confirmationFocus', 'unsavedEditsPreserved'])
        else:
            assert complete['reopened']
    result = {'ok': True, 'actualWebView': True, 'states': 41,
              'screenshots': 41 if args.screenshots else 0,
              'themeRestartVerified': True, 'realMusicRequests': 0}
    (root / 'report.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(result))


if __name__ == '__main__':
    main()
