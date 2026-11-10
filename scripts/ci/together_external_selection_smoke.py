"""Run the production WebView external selection flow with sealed room fixtures."""
import argparse
import json
import os
from pathlib import Path

from motion_smoke import launch
from vinyl_lyric_copy_smoke import require_xvfb

STAGES = ('external_partial', 'external_prompt', 'external_dismissed', 'external_ended')


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    if args.screenshots:
        require_xvfb()

    root = args.root.resolve()
    root.mkdir(parents=True, mode=0o700)
    output = root / 'run'
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(output), CUMULUS_EXTERNAL_SELECTION_PROOF='1')
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR',
                'APPDATA', 'LOCALAPPDATA'):
        directory = root / key
        directory.mkdir(mode=0o700)
        env[key] = str(directory)
    if args.runtime:
        env['PATH'] = os.pathsep.join([str(args.runtime.resolve(strict=True)), env.get('PATH', '')])
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(strict=True), env, log, output, args.screenshots,
                      capture_glob='external_*.json')

    failure = output / 'failed.json'
    assert not failure.exists(), read(failure) if failure.exists() else ''
    assert code == 0 and read(output / 'complete.json')['ok'] is True
    host = read(output / 'host-final.json')
    assert all(host[field] is True for field in (
        'ui_ok', 'local_restored', 'fixture_api', 'fixture_im', 'fixture_audio'))
    assert host['end_requests'] == 1 and host['im_closed'] > 0 and not host['real_peer_verified']
    partial, prompt, dismissed, ended = [read(output / f'{stage}.json') for stage in STAGES]
    assert partial['kept'] == ['714', '715'] and partial['playId'] and partial['position'] == 12
    assert prompt['focused'] == '取消'
    assert dismissed['roomOpen'] and not ended['roomOpen']
    assert dismissed['state'] == ended['state'] == 'paused'
    if args.screenshots:
        assert {path.stem for path in output.glob('external_*.png')} == set(STAGES)
        paint = read(output / 'paint-external_prompt.json')
        assert len(paint['samples']) == 1 and paint['samples'][0]['ok']
    report = dict(ok=True, actualWebView=True, stages=len(STAGES),
                  screenshots=len(STAGES) if args.screenshots else 0,
                  partialQueue=partial['kept'], confirmedEndRequests=host['end_requests'],
                  blockedSelectionNotReplayed=True, realPeerVerified=False,
                  fixtureApiImAudio=True)
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
