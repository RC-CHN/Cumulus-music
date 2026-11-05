"""Prove classic podcast Scene FM and Settings in the real WebView and null libmpv."""
import argparse
import json
import os
import sys
from pathlib import Path

from window_manager import launch

DIRECTORY = '/api/voice/homepage/block/content'
EXPERIMENT = '/api/rtrs/abt/front/expinfo/list'
CHECK = '/api/dj/play/url/check'
MEDIA = '/api/song/enhance/player/url/v1'
DETAIL = '/api/dj/program/detail'
RESUME = '/api/dj/user/setting/resumeplay/get'
STAGES = {
    'automatic': ('settings', 'first_automatic', 'paused', 'remount_no_replay', 'title_resumes_detail'),
    'manual': ('settings', 'classic_opted_out', 'manual_play'),
    'updated': ('settings', 'updated_no_scene'),
    'retry': ('settings', 'directory_error', 'retry_automatic'),
}


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def requests(calls, endpoint):
    return [body for path, body in calls if path == endpoint]


def check_player(stage):
    p = stage['state']
    assert (p['state'], p['voice'], p['song'], p['mode']) == ('playing', '601', '1601', 'full'), stage
    assert p['playId'] and p['position'] > .2, stage
    q = stage['queue']
    assert [entry[1] for entry in q['entries']] == ['601', '602'], q
    assert q['current'] == q['entries'][0][0], q
    assert stage['hasCard'] and stage['layout'] == 'classic' and not stage['hasStandalone'], stage
    assert stage['scenes'] == ['OP_1', 'OP_7'], stage


def check_calls(calls, directory_count, playback):
    expected_directory = {'blockCode': 'VOICE_HOMEPAGE_BLOCK_SCENE_FM'}
    assert requests(calls, EXPERIMENT) == [{'expNames': 'podcast_tab_better'}], calls
    assert requests(calls, DIRECTORY) == [expected_directory] * directory_count, calls
    if not playback:
        assert [path for path, _ in calls] == [EXPERIMENT] + [DIRECTORY] * directory_count, calls
        return
    assert [path for path, _ in calls[:1 + directory_count]] == [EXPERIMENT] + [DIRECTORY] * directory_count, calls
    following = calls[1 + directory_count:]
    assert all(path in (CHECK, MEDIA, DETAIL, RESUME) for path, _ in following), calls
    permission = [str(p['voiceId']) for p in requests(following, CHECK)]
    media = [(str(json.loads(p['ids'])[0]), p['level']) for p in requests(following, MEDIA)]
    # The actor may prefetch the second row. Only one authorization for each
    # voice is allowed; the primary selected voice must authorize before media.
    assert permission in (['601'], ['601', '602']), calls
    assert media in ([('1601', 'standard')], [('1601', 'standard'), ('1602', 'standard')]), calls
    check_at = next(i for i, (path, p) in enumerate(following) if path == CHECK and str(p['voiceId']) == '601')
    media_at = next(i for i, (path, p) in enumerate(following) if path == MEDIA and str(json.loads(p['ids'])[0]) == '1601')
    assert check_at < media_at, calls


def run(case, root, binary, dist, runtime, screenshots):
    root.mkdir()
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root / 'run'), CUMULUS_PODCAST_SCENE_HOME_PROOF=case)
    if sys.platform == 'linux':
        env.update(LC_ALL='en_US.UTF-8', GDK_BACKEND='x11')
    if runtime:
        env['PATH'] = str(runtime) + os.pathsep + env.get('PATH', '')
    env['CUMULUS_UX_SCRIPT'] = str(Path(__file__).resolve().parents[2]
                                   / 'desktop/examples/podcast_scene_home_ui_probe.js')
    if dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(dist)
    for key, name in (('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')):
        directory = root / 'os-profile' / name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    output = root / 'run' / 'first'
    log_path = root / 'host.log'
    with log_path.open('x', encoding='utf-8') as log:
        status = launch(binary, env, log, output, screenshots)
    failure = output / 'failed.json'
    assert not failure.exists(), failure.read_text(encoding='utf-8') if failure.exists() else ''
    host = log_path.read_text(encoding='utf-8')
    assert status == 0 and 'panicked at' not in host, host
    complete = read(output / 'complete.json')
    stages = {path.stem.split('_', 2)[2]: read(path) for path in sorted(output.glob('ux_*.json'))}
    assert complete['ok'] and complete['case'] == case and complete['stageCount'] == len(stages)
    assert tuple(stages) == STAGES[case] and all(s['actual'] and s['case'] == case for s in stages.values())
    settings = stages['settings']
    assert settings['checked'] == (case != 'manual') and not settings['hasCard'], settings
    assert settings['calls'] == [], settings
    assert settings['state']['state'] == 'idle' and settings['queue']['entries'] == [], settings
    for name in ('updated_no_scene', 'classic_opted_out', 'directory_error'):
        if name in stages:
            stage = stages[name]
            assert stage['state']['state'] == 'idle' and stage['queue']['entries'] == [], stage
            assert stage['hasCard'] == (name != 'updated_no_scene'), stage
            check_calls(stage['calls'], int(name != 'updated_no_scene'), False)
    directory_count = 0 if case == 'updated' else 2 if case == 'retry' else 1
    check_calls(complete['calls'], directory_count, case != 'updated')
    for name in ('first_automatic', 'manual_play', 'retry_automatic'):
        if name in stages:
            check_player(stages[name])
            check_calls(stages[name]['calls'], directory_count, True)
    if case == 'automatic':
        paused, returned = stages['paused'], stages['remount_no_replay']
        assert paused['state']['state'] == returned['state']['state'] == 'paused'
        assert paused['state']['playId'] == returned['state']['playId'] == stages['first_automatic']['state']['playId']
        assert paused['queue'] == returned['queue'] == stages['first_automatic']['queue']
        assert abs(paused['state']['position'] - returned['state']['position']) < .1
        opened = stages['title_resumes_detail']
        assert opened['lyricsOpen'] and opened['state']['state'] == 'playing'
        assert opened['state']['playId'] == returned['state']['playId']
        assert opened['queue'] == returned['queue']
        assert opened['state']['position'] > returned['state']['position']
    if sys.platform == 'linux':
        locale = read(output / 'locale.json')
        assert locale['numeric'] == 'C' and all(locale[k].casefold().replace('-', '') == 'en_us.utf8'
                                                for k in ('messages', 'characters')), locale
    captures = len(list(output.glob('ux_*.png')))
    assert not screenshots or captures == len(stages), (captures, stages)
    return {'stages': list(stages), 'screenshots': captures, 'calls': complete['calls']}


def main():
    assert sys.platform in ('linux', 'win32'), 'this proof runs on Linux X11 or Windows'
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--root', required=True, type=Path)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--case', action='append', choices=STAGES)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    if args.screenshots and sys.platform != 'linux':
        parser.error('--screenshots requires Linux X11')
    binary = args.binary.resolve(strict=True)
    dist = args.frontend_dist.resolve(strict=True) if args.frontend_dist else None
    runtime = args.runtime.resolve(strict=True) if args.runtime else None
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    results = {case: run(case, root / case, binary, dist, runtime, args.screenshots)
               for case in (args.case or STAGES)}
    result = {'actualWebView': True, 'decoder': 'libmpv', 'audioOutput': 'null',
              'transport': 'synthetic only', 'cases': results}
    (root / 'report.json').write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n',
                                    encoding='utf-8')
    print(json.dumps(result, ensure_ascii=False))


if __name__ == '__main__':
    main()
