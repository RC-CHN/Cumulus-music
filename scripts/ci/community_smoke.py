"""Run the actual community read surface against a closed synthetic desktop API."""
import argparse
import json
import os
from pathlib import Path

from window_manager import launch


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
    env.update(CUMULUS_PROOF_DIR=str(root / 'run'),
               CUMULUS_UX_SCRIPT=str(Path(__file__).resolve().parents[2] /
                                     'desktop/examples/community_ui_probe.js'))
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        path = root / 'os-profile' / name
        path.mkdir(parents=True, mode=0o700)
        env[key] = str(path)
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    output = root / 'run/first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        status = launch(args.binary.resolve(strict=True), env, log, output, args.screenshots)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert status == 0, f'community process exited {status}'
    complete = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['composerVerified'] and complete['realMusicRequests'] == complete['remoteWrites'] == 0
    stages = {path.stem.split('_', 2)[2]: json.loads(path.read_text(encoding='utf-8'))
              for path in output.glob('ux_*.json')}
    assert set(stages) == {'first_error', 'feed', 'notice_return', 'detail_error', 'comment_error', 'comments',
                           'comment_next', 'back_to_feed', 'unavailable', 'next_error',
                           'next_recovered', 'empty', 'composer_draft', 'composer_search',
                           'composer_restored'}
    assert len(stages) == complete['stageCount'] == 15
    assert all(stage['width'] == 760 for stage in stages.values())
    assert stages['feed']['theme'] == 'light' and stages['empty']['theme'] == 'dark'
    assert stages['next_error']['feedCount'] == 5 and stages['next_recovered']['feedCount'] == 6
    native = complete['native']
    assert native['feed'] == [{'cursor': '-1'}, {'cursor': '-1'},
                              {'cursor': '90071992547409933331'}] + [
                                  {'cursor': '90071992547409933331'}, {'cursor': -1}]
    assert native['details'] == 2 and native['unavailable'] == 1
    assert native['commentOffsets'] == [0, 0, 2]
    assert native['notice'][:2] == [{}, {}] and len(native['notice']) == 3
    assert list(native['notice'][2]) == ['time'] and native['notice'][2]['time'] > 1700000000000
    assert native['releaseReads'] == 2
    assert [path for path, _ in native['composer']] == [
        '/api/search/song/list/page', '/api/search/song/list/page', '/api/search/song/list/page',
        '/api/v1/search/artist/get', '/api/v1/search/playlist/get', '/api/search/voicelist/get',
        '/api/v1/search/album/get',
    ]
    assert [body['offset'] for _, body in native['composer']] == [0, 8, 8, 0, 0, 0, 0]
    if args.screenshots:
        assert len(list(output.glob('ux_*.png'))) == len(stages)
    report = {'ok': True, 'actualWebView': True, 'states': len(stages),
              'screenshots': len(stages) if args.screenshots else 0,
              'realMusicRequests': 0, 'remoteAccountWrites': 0, 'composerVerified': True}
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
