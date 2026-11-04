"""Verify retained style recommendations, native playback and explicit rereads in the real WebView."""
import argparse
import json
import os
import sys
from pathlib import Path

from motion_smoke import launch


STYLE = '/api/homepage/category/daily/song/list'
MIX = '/api/pc/daily/rcmd/block'
DEFAULT = {'source': '', 'categoryId': '', 'tagId': '', 'songId': ''}
SEED_A = {'source': 'homepage', 'categoryId': '18', 'tagId': '21', 'songId': '81,82'}
SEED_B = {'source': 'homepage', 'categoryId': '19', 'tagId': '22', 'songId': '91,92'}
STAGES = (
    ('default_and_cards', [DEFAULT], 71, [71, 72]),
    ('seed_a', [DEFAULT, SEED_A], 71, [71, 81, 72]),
    ('seed_b', [DEFAULT, SEED_A, SEED_B], 91, [91, 92]),
    ('back_seed_a', [DEFAULT, SEED_A, SEED_B], 82, [81, 82]),
    ('refresh_seed_a', [DEFAULT, SEED_A, SEED_B, SEED_A], 87, [86, 87]),
    ('back_default', [DEFAULT, SEED_A, SEED_B, SEED_A], 72, [71, 72]),
    ('refresh_default', [DEFAULT, SEED_A, SEED_B, SEED_A, DEFAULT], 77, [76, 77]),
)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/style_home_ui_probe.js'
    script = root / 'style-home.js'
    script.write_text(source.read_text(encoding='utf-8'), encoding='utf-8')
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_UX_SCRIPT=str(script),
               CUMULUS_MOTION_PROOF='1', CUMULUS_STYLE_RECOMMENDATIONS_PROOF='1',
               CUMULUS_STYLE_HOME_PROOF='1')
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR',
                'APPDATA', 'LOCALAPPDATA'):
        profile = root / 'profile' / key
        profile.mkdir(parents=True, mode=0o700)
        env[key] = str(profile)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve())
    output = root / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, output, args.screenshots)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, (root / 'host.log').read_text(encoding='utf-8')

    def read(stage):
        return json.loads((output / f'{stage}.json').read_text(encoding='utf-8'))

    def check_calls(snapshot, reads, name):
        calls = snapshot['styleHomeCalls']
        assert [call for call in calls if call['path'] == STYLE] == [
            {'path': STYLE, 'args': item} for item in reads
        ], (name, calls)
        mix_calls = [call for call in calls if call['path'] == MIX]
        assert mix_calls and all(set(call['args']) == {'clientTime'} for call in mix_calls), (name, calls)
        assert len(calls) == len(mix_calls) + len(reads), (name, calls)
        assert snapshot['styleRecommendationCalls'] == [], (name, snapshot['styleRecommendationCalls'])

    report = []
    for index, (name, reads, track, queue) in enumerate(STAGES, start=1):
        stage = read(f'ux_{index:02d}_style_{name}')
        check_calls(stage, reads, name)
        assert stage['track'] == str(track) and stage['queue'] == list(map(str, queue)), (name, stage)
        report.append({'stage': name, 'styleReads': len(reads), 'track': track, 'queue': queue})
    complete = read('complete')
    assert complete['ok'] and complete['stageCount'] == len(STAGES)
    check_calls(complete, STAGES[-1][1], 'complete')
    assert len(list(output.glob('ux_*.png'))) == (len(STAGES) if args.screenshots else 0)
    summary = {'ok': True, 'actualWebView': True, 'styleSaves': 0, 'stages': report}
    if sys.platform == 'linux':
        observed = read('locale')
        assert observed['numeric'] == 'C', observed
        for category, key in [('LC_MESSAGES', 'messages'), ('LC_CTYPE', 'characters')]:
            expected = env.get('LC_ALL') or env.get(category) or env.get('LANG') or 'C'
            assert observed[key].casefold().replace('-', '') == expected.casefold().replace('-', ''), observed
        summary['locale'] = observed
    (root / 'report.json').write_text(json.dumps(summary, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(summary))


if __name__ == '__main__':
    main()
