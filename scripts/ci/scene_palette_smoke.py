"""Prove source-bound Scene FM palette fetch, local canvas, Rust color and voice detail."""
import argparse
import json
import os
from pathlib import Path
import sys
from urllib.parse import urlsplit
from window_manager import launch

PENDING_CASES = ('pending_header', 'pending_image', 'pending_canvas')
CASES = ('header', 'only_tb', 'sample', 'late') + PENDING_CASES
BLUE = 'linear-gradient(245deg,rgb(71,125,179)0%,rgb(71,143,179)48%,rgb(71,125,179)100%)'
BROWN = 'linear-gradient(245deg,rgb(179,107,71)0%,rgb(179,89,71)48%,rgb(179,107,71)100%)'
RED = 'linear-gradient(245deg,rgb(179,71,71)0%,rgb(179,71,89)48%,rgb(179,71,71)100%)'


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def verify_pending(output, case, screenshots, done, stages):
    names = ('pending_first', 'pending_second', 'palette_complete', 'playing_stable')
    assert done['ok'] and done['case'] == case and tuple(stages) == names, done
    assert done['stageCount'] == 4 and all(s['actual'] for s in stages.values()), done
    assert done['oldVoiceRejected'] is True, done
    assert done['realMusicRequests'] == done['realAccountWrites'] == 0, done
    samples = int(case != 'pending_header')
    ipc = {'reads': 1, 'samples': samples, 'cancels': 0}
    assert done['ipc'] == ipc, done
    first, second, complete, stable = (stages[name] for name in names)
    assert first['state']['state'] == 'idle' and first['queue']['voices'] == [], first
    assert first['scene']['id'] == '601' and second['scene']['id'] == '602', stages
    cover = 'https://p1.music.126.net/obj/cumulus-palette/cover601.png'
    assert all(s['scene']['cover'] == cover for s in stages.values()), stages
    assert first['background']['pending'] == second['background']['pending'] == 'true', stages
    assert second['oldVoiceRejected'] is True, second
    expected = BLUE if case == 'pending_header' else BROWN
    for stage in (complete, stable):
        bg = stage['background']
        assert bg['pending'] == 'false' and bg['kind'] == 'gradient', stage
        assert ''.join(bg['image'].split()) == expected and stage['ipc'] == ipc, stage
    for stage in (second, complete, stable):
        state, queue = stage['state'], stage['queue']
        assert state['state'] == 'playing' and state['voice'] == '602' and state['playId'], stage
        assert state['playId'] == second['state']['playId'] and queue == second['queue'], stage
        assert [item[1] for item in queue['voices']] == ['601', '602'], stage
    assert stable['state']['position'] > complete['state']['position'] >= second['state']['position']
    stats = done['paletteRequests']
    assert len(stats['urls']) == 1 + samples and stats['bodyBytes'] == 76 * samples, stats
    assert all(urlsplit(u).netloc == 'p1.music.126.net' and
               urlsplit(u).path == '/obj/cumulus-palette/cover601.png' for u in stats['urls']), stats
    assert urlsplit(stats['urls'][0]).query == 'xnos=1&imageView=&thumbnail=300y300', stats
    if samples:
        assert urlsplit(stats['urls'][1]).query == 'imageView=&thumbnail=300y300&type=webp&rotate=0&tostatic=0'
        assert done['sampledSize'] == {'width': 16, 'height': 8, 'bytes': 512}, done
    if sys.platform == 'linux':
        locale = read(output / 'locale.json')
        assert locale['numeric'] == 'C' and all(locale[k].casefold().replace('-', '') == 'en_us.utf8'
            for k in ('messages', 'characters')), locale
    pictures = len(list(output.glob('ux_*.png')))
    assert not screenshots or pictures == 4, pictures
    return {'stages': names, 'screenshots': pictures, 'paletteRequests': stats,
            'sameArtworkPendingReuse': True, 'oldVoiceAdmissionRejected': True, 'ipc': ipc}


def verify(output, case, screenshots):
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    done = read(output / 'complete.json')
    stages = {p.stem.split('_', 2)[2]: read(p) for p in sorted(output.glob('ux_*.json'))}
    if case in PENDING_CASES:
        return verify_pending(output, case, screenshots, done, stages)
    names = ('new_source', 'old_source_discarded') if case == 'late' else (
        'palette_idle', 'playing_palette_stable', 'voice_detail')
    assert done['ok'] and done['case'] == case and tuple(stages) == names, done
    assert done['stageCount'] == len(stages) and all(p['actual'] for p in stages.values())
    for name, stage in stages.items():
        bg = stage['background']
        assert bg['pending'] == 'false', stage
        if case == 'only_tb':
            assert (bg['kind'], bg['image']) == ('theme_fallback', 'none'), stage
            assert bg['color'].replace(' ', '') == (
                'rgba(255,255,255,0.3)' if stage['theme'] == 'dark' else 'rgba(40,50,72,0.3)'), stage
        else:
            expected = RED if case == 'late' else BROWN if case == 'sample' else BLUE
            assert bg['kind'] == 'gradient' and ''.join(bg['image'].split()) == expected, stage
        player, queue = stage['state'], stage['queue']
        if name == 'palette_idle':
            assert player['state'] == 'idle' and queue['voices'] == [], stage
        else:
            voice = '602' if case == 'late' else '601'
            assert player['state'] == 'playing' and player['voice'] == voice and player['playId'], stage
            assert [v[1] for v in queue['voices']] == ['601', '602'], stage
        if name == 'voice_detail':
            assert stage['voiceSourceVisible'] is False, stage
    if case != 'late':
        before, after = stages['playing_palette_stable'], stages['voice_detail']
    else:
        before, after = stages['new_source'], stages['old_source_discarded']
    assert before['state']['playId'] == after['state']['playId'], stages
    assert before['queue'] == after['queue'], stages
    assert after['state']['position'] >= before['state']['position'], stages
    stats = done['paletteRequests']
    urls = stats['urls']
    assert len(urls) == (2 if case in ('sample', 'late') else 1), stats
    for url in urls:
        target = urlsplit(url)
        assert (target.scheme, target.netloc) == ('https', 'p1.music.126.net'), url
        assert target.path in ('/obj/cumulus-palette/cover601.png', '/obj/cumulus-palette/cover602.png'), url
        assert target.query in ('xnos=1&imageView=&thumbnail=300y300',
                                 'imageView=&thumbnail=300y300&type=webp&rotate=0&tostatic=0'), url
    assert (stats['bodyBytes'] > 0) == (case == 'sample'), stats
    if case == 'late':
        assert '601.png' in urls[0] and '602.png' in urls[1], urls
    if sys.platform == 'linux':
        locale = read(output / 'locale.json')
        assert locale['numeric'] == 'C'
        assert all(locale[k].casefold().replace('-', '') == 'en_us.utf8'
                   for k in ('messages', 'characters')), locale
    pictures = len(list(output.glob('ux_*.png')))
    assert not screenshots or pictures == len(stages), (pictures, stages)
    return {'stages': names, 'screenshots': pictures, 'paletteRequests': stats}


def run(case, binary, root, dist, runtime, screenshots):
    root.mkdir()
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root / 'run'), CUMULUS_SCENE_FM_PALETTE_PROOF=case,
               CUMULUS_UX_SCRIPT=str(Path(__file__).resolve().parents[2] /
                                    'desktop/examples/scene_palette_ui_probe.js'))
    if sys.platform == 'linux':
        env.update(LC_ALL='en_US.UTF-8', GDK_BACKEND='x11')
    if dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(dist)
    if runtime:
        env['PATH'] = str(runtime) + os.pathsep + env.get('PATH', '')
    for key, name in (('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')):
        p = root / 'os-profile' / name
        p.mkdir(parents=True, mode=0o700)
        env[key] = str(p)
    output = root / 'run' / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        status = launch(binary, env, log, output, screenshots)
    result = verify(output, case, screenshots)
    assert status == 0, (status, (root / 'host.log').read_text(encoding='utf-8'))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--root', required=True, type=Path)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--case', choices=CASES, action='append')
    args = parser.parse_args()
    if sys.platform not in ('linux', 'win32') or args.screenshots and sys.platform != 'linux':
        parser.error('use Linux X11 or Windows; screenshots require X11')
    binary = args.binary.resolve(strict=True)
    dist = args.frontend_dist.resolve(strict=True) if args.frontend_dist else None
    runtime = args.runtime.resolve(strict=True) if args.runtime else None
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    results = {case: run(case, binary, root / case, dist, runtime, args.screenshots)
               for case in args.case or CASES}
    report = dict(actualWebView=True, transport='closed synthetic MediaBackend', decoder='libmpv',
                  audioOutput='null', cases=results)
    (root / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n',
                                    encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    main()
