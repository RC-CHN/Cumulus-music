"""Real native voice header, square cover/count, creator profile and failed-cover UI."""
import argparse
import json
import os
from pathlib import Path
import sys
from window_manager import launch

STAGES = ('card_placeholder', 'bar_placeholder', 'voice_metadata', 'creator_profile', 'detail_reopened')


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    if sys.platform not in ('linux', 'win32') or args.screenshots and sys.platform != 'linux':
        parser.error('use Linux X11 or Windows; screenshots require X11')
    binary = args.binary.resolve(strict=True)
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root / 'run'), CUMULUS_VOICE_DETAIL_PROOF='1',
               CUMULUS_UX_SCRIPT=str(Path(__file__).resolve().parents[2] / 'desktop/examples/voice_detail_ui_probe.js'))
    if sys.platform == 'linux':
        env.update(LC_ALL='en_US.UTF-8', GDK_BACKEND='x11', TZ='Asia/Shanghai')
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve(strict=True)) + os.pathsep + env.get('PATH', '')
    for key, name in (('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')):
        p = root / 'os-profile' / name
        p.mkdir(parents=True, mode=0o700)
        env[key] = str(p)
    output = root / 'run' / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        status = launch(binary, env, log, output, args.screenshots)
    failure = output / 'failed.json'
    assert not failure.exists(), failure.read_text(encoding='utf-8') if failure.exists() else ''
    assert status == 0, (root / 'host.log').read_text(encoding='utf-8')
    done = read(output / 'complete.json')
    stages = {p.stem.split('_', 2)[2]: read(p) for p in sorted(output.glob('ux_*.json'))}
    assert done['ok'] and done['stageCount'] == len(stages) and tuple(stages) == STAGES, done
    assert done['realMusicRequests'] == done['realAccountWrites'] == 0
    assert all(stage['actual'] for stage in stages.values()), stages
    start = stages['card_placeholder']
    assert start['covers']['card'] == 'error' and start['state']['state'] == 'idle', start
    assert start['queue']['voices'] == [] and start['voiceDetailCalls'] == [], start
    played = stages['bar_placeholder']
    assert played['covers']['bar'] == 'error' and played['voiceDetailCalls'] == [], played
    expected_detail = {
        'id': '601', 'creator': {'id': '55', 'name': '合成主播'}, 'releaseTime': 1700049600000,
        'coverUrl': 'https://p1.music.126.net/obj/cumulus-palette/cover601.png', 'playCount': 123456,
    }
    for name in STAGES[1:]:
        stage = stages[name]
        player, queue = stage['state'], stage['queue']
        assert player['state'] == 'playing' and player['voice'] == '601', stage
        assert player['playId'] == played['state']['playId'] and player['voiceDetail'] == expected_detail, stage
        assert queue == played['queue'] and [v[1] for v in queue['voices']] == ['601', '602'], stage
        assert player['position'] >= played['state']['position'], stage
    for name in ('voice_metadata', 'detail_reopened'):
        stage = stages[name]
        assert stage['metadata'] == {'creator': '合成主播', 'release': '2023-11-15', 'songCredits': False}, stage
        assert stage['covers']['detail'] == 'error', stage
        art = stage['artwork']
        assert art['width'] > 0 and abs(art['width'] - art['height']) <= 1, art
        assert abs(art['width'] - art['fittedSize']) <= 1, art
        assert {key: art[key] for key in ('radius', 'count', 'top', 'right', 'fontSize')} == {
            'radius': '8px', 'count': '12.3万', 'top': '23px', 'right': '20px', 'fontSize': '22px',
        }, art
    assert stages['creator_profile']['profile'] == '合成主播主页'
    assert stages['voice_metadata']['voiceDetailCalls'] == [], stages
    requests = done['voiceDetailCalls']
    assert [path for path, _ in requests] == [
        '/api/w/v1/user/detail/55', '/api/djradio/get/create/sub/count', '/api/user/playlist'], requests
    assert done['paletteRequests']['bodyBytes'] == 0
    assert len(done['paletteRequests']['urls']) == 1, done['paletteRequests']
    if sys.platform == 'linux':
        locale = read(output / 'locale.json')
        assert locale['numeric'] == 'C' and all(
            locale[key].casefold().replace('-', '') == 'en_us.utf8' for key in ('characters', 'messages')), locale
    pictures = len(list(output.glob('ux_*.png')))
    assert not args.screenshots or pictures == len(stages), pictures
    report = dict(actualWebView=True, nativeVoiceMetadata=True, creatorTarget='user:55',
                  squareCover=stages['voice_metadata']['artwork'],
                  nativeDecoder='libmpv/null', screenshots=pictures, stages=STAGES,
                  voiceDetailCalls=requests, realAccountWrites=0)
    (root / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    main()
