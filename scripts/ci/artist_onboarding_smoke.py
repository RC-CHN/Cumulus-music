"""Official artist onboarding through real QR EAPI transport and desktop WebView."""
import argparse
import json
import os
from pathlib import Path

from motion_smoke import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--mode', choices=['recall', 'new-uncertain'], default='recall')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    proof = root / 'run'
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(proof), CUMULUS_ARTIST_ONBOARDING_PROOF=args.mode)
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR',
                'APPDATA', 'LOCALAPPDATA'):
        profile = root / 'os-profile' / key
        profile.mkdir(parents=True, mode=0o700)
        env[key] = str(profile)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    output = proof / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        status = launch(args.binary.resolve(), env, log, output, args.screenshots)
    failure = output / 'failed.json'
    assert not failure.exists(), failure.read_text(encoding='utf-8') if failure.exists() else ''
    assert status == 0
    result = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
    assert result['ok'] and result['artistOnboarding'] == args.mode
    assert result['wire']['authorized'] and result['wire']['authenticatedIdentityReads'] >= 1
    assert result['realAccountWrites'] == result['realMusicRequests'] == 0
    recall = args.mode == 'recall'
    kind = '/api/recall/user/favor' if recall else '/api/newuser/favor'
    songs = '/api/recall/user/favor/artist/song' if recall else '/api/new/user/favor/artist/song'
    assert result['artistCalls'] == [
        kind + '/rcmd-artists', kind + '/similar-rcmd-artists', kind + '/similar-rcmd-artists',
        '/api/newuser/favor/rcmd-artists/save', *([songs, songs] if recall else [songs]),
    ], result['artistCalls']
    guest = json.loads((output / 'ux_guest.json').read_text(encoding='utf-8'))
    pending = json.loads((output / 'ux_qr_pending.json').read_text(encoding='utf-8'))
    assert not guest['authenticated'] and not pending['authenticated']
    assert not guest['artistCalls'] and not pending['artistCalls']
    transport = json.loads((output / 'ux_artist_transport.json').read_text(encoding='utf-8'))
    assert transport['state'] == ('idle' if recall else 'playing')
    assert transport['queueLen'] == (2 if recall else 1)
    for counter in ('sourceAuthorizations', 'sourceLoadAttempts', 'sourceLoads'):
        assert transport['fixture'][counter] == (0 if recall else 1), transport['fixture']

    stages = 8 if recall else 7
    assert len(list(output.glob('ux_*.json'))) == stages
    assert len(list(output.glob('ux_*.png'))) == (stages if args.screenshots else 0)
    result['actualWebView'] = True
    (root / 'report.json').write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding='utf-8')
    print(json.dumps(result, ensure_ascii=False))


if __name__ == '__main__':
    main()
