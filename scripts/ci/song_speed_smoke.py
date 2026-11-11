"""Real Linux WebView and null-audio proof of the signed ordinary song-speed panel."""
import argparse
import json
import os
from pathlib import Path

from motion_smoke import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--frontend-dist', required=True, type=Path)
    parser.add_argument('--root', required=True, type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1',
               CUMULUS_SEARCH_UI_PROOF='1', CUMULUS_SONG_SPEED_PROOF='1',
               CUMULUS_PC_PRESENTATION_PROOF='1',
               CUMULUS_PROOF_FRONTEND_DIST=str(args.frontend_dist.resolve()))
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime')]:
        directory = root / 'os-profile' / name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    source = (Path(__file__).resolve().parents[2] /
              'desktop/examples/song_speed_ui_probe.js').read_text(encoding='utf-8')
    captures = []
    expected_first = ['custom_same_as_preset', 'remembered_custom', 'control_cohort',
                      'restored_cohort', 'voice_separate']
    expected_reopen = ['persisted_custom']
    for index, (name, expected) in enumerate([('first', expected_first), ('reopen', expected_reopen)]):
        output = root / name
        if index:
            env['CUMULUS_PROOF_REOPEN'] = '1'
        script = root / f'{name}.js'
        script.write_text(f'window.__songSpeedReopen={json.dumps(bool(index))};\n' + source,
                          encoding='utf-8')
        env['CUMULUS_UX_SCRIPT'] = str(script)
        with (root / f'{name}.log').open('x', encoding='utf-8') as log:
            code = launch(args.binary.resolve(), env, log, output, args.screenshots)
        failed = output / 'failed.json'
        assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
        assert code == 0, f'song speed probe exited {code}'
        complete = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['stageCount'] == len(expected)
        signed = complete['signedSearch']
        assert signed['transport'] == 'MusicHttpClient' and signed['syntheticOnly']
        assert signed['sameProfileAccountRestore']
        assert all(call['signedPcContextVerified'] and call['sameAccountCredential']
                   for call in signed['calls'])
        if index:
            assert complete['restored']['customTenths'] == 13 and complete['restored']['customSelected']
        else:
            assert complete['staleRejected'] and complete['voiceRate'] == 1.5
            assert complete['catalogIdentity'] == {
                'matchedSong': '2', 'playedVoice': '402', 'backingSong': '2',
                'rowStillVisible': True, 'rowHighlighted': False,
            }
            assert [ratio['rate'] for ratio in complete['ratios']] == [1.3, 1, 1.3]
            assert all(abs(ratio['delta'] - ratio['elapsed'] * ratio['rate']) < .45
                       for ratio in complete['ratios'])
        locale = json.loads((output / 'locale.json').read_text(encoding='utf-8'))
        assert locale['numeric'] == 'C' and locale['messages'] != 'C' and locale['characters'] != 'C'
        rows = sorted(output.glob('ux_*.json'))
        assert [path.stem[6:] for path in rows] == expected
        for path in rows:
            row = json.loads(path.read_text(encoding='utf-8'))
            assert row['documentContained']
            if row['panel'] is not None:
                panel, anchor = row['panel'], row['anchor']
                assert abs(panel['width'] - 218) < 1 and abs(panel['right'] - anchor['right']) < 1
                assert abs(panel['top'] - (anchor['top'] - 246)) < 1
        if args.screenshots:
            assert len(list(output.glob('ux_*.png'))) == len(rows)
        captures.extend(str(path.relative_to(root)) for path in rows)
    report = {'actualWebView': True, 'actualNullAudio': True, 'signedSyntheticTransport': True,
              'catalogVoiceIdentityVerified': True,
              'coldProcessRestart': True, 'physicalAudioOutputVerified': False,
              'realMusicRequests': 0, 'realAccountWrites': 0, 'captures': captures}
    (root / 'audit.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
