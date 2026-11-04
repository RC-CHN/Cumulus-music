"""Run the recently updated podcast page in a real WebView and null-output libmpv."""
import argparse
import json
import os
from pathlib import Path

from window_manager import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--root', required=True, type=Path)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    run = root / 'run'
    script = Path(__file__).resolve().parents[2] / 'desktop/examples/podcast_recent_ui_probe.js'
    env.update(CUMULUS_PROOF_DIR=str(run), CUMULUS_UX_SCRIPT=str(script))
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        path = root / 'os-profile' / name
        path.mkdir(parents=True, mode=0o700)
        env[key] = str(path)
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve(strict=True)) + os.pathsep + env.get('PATH', '')
    output = run / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        status = launch(args.binary.resolve(strict=True), env, log, output, args.screenshots)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert status == 0, f'probe exited {status}'
    host = (root / 'host.log').read_text(encoding='utf-8')
    assert 'panicked at' not in host, host
    complete = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['realMusicRequests'] == complete['remoteWrites'] == 0
    stages = {path.stem.split('_', 2)[2]: json.loads(path.read_text(encoding='utf-8'))
              for path in output.glob('ux_*.json')}
    assert len(stages) == complete['stageCount'] and all(s['actual'] for s in stages.values())
    assert list(stages['first_page']['ids']) == list(stages['back']['ids'])
    assert stages['next_error']['size'] == 18 and stages['next_page']['size'] == 20
    assert abs(stages['scrolled_back']['scroll'] - stages['next_page']['scroll']) < 2
    assert '520' in stages['scrolled_back']['ids']
    assert stages['playing']['state'] == 'playing' and stages['paused']['state'] == 'paused'
    calls = complete['calls']
    requests = lambda route: [body for target, body in calls if target == route]
    cursor = {'updateTime': 1_700_000_000_123}
    assert requests('/api/social/my/recent/update/voice/list/v1') == [{}, {}, cursor, cursor, {}], calls
    detail = '/api/dj/program/detail'
    permission = '/api/dj/play/url/check'
    media = '/api/song/enhance/player/url/v1'
    by_voice = {}
    for route, body in calls:
        if route == detail:
            id_ = body['id']
        elif route == permission:
            id_ = body['voiceId']
        elif route == media:
            id_ = str(int(json.loads(body['ids'])[0]) - 1000)
            assert body['level'] == 'standard', body
        else:
            continue
        by_voice.setdefault(id_, []).append(route)
    assert by_voice['501'] == [detail, permission, media], calls
    assert by_voice['503'] == [detail, detail, permission, media], calls
    assert by_voice['504'] == [detail, permission, media], calls
    # Normal native next-track prefetch can overlap the final screenshot.
    assert set(by_voice) <= {'501', '503', '504', '506'}, calls
    if '506' in by_voice:
        assert by_voice['506'] == [detail, permission, media][:len(by_voice['506'])], calls
    pictures = len(list(output.glob('ux_*.png')))
    assert not args.screenshots or pictures == len(stages)
    result = {'actualWebView': True, 'decoder': 'libmpv', 'audioOutput': 'null', 'screenshots': pictures,
              'states': list(stages), **complete}
    (root / 'report.json').write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n',
                                    encoding='utf-8')
    print(json.dumps(result, ensure_ascii=False))


if __name__ == '__main__':
    main()
