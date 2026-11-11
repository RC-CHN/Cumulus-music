"""Verify production Tauri homepage in a real WebView with synthetic responses."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--recall', action='store_true', help='verify native Home Recall prefill and guide')
    parser.add_argument('--screenshots', action='store_true', help='capture Linux GTK pixels')
    parser.add_argument('--frontend-dist', type=Path, help='local production snapshot for comparison')
    parser.add_argument('--probe-script', type=Path, help='local synthetic WebView script for comparison')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(mode=0o700)
    capture_root = None
    if args.screenshots:
        import gi
        gi.require_version('Gdk', '3.0')
        from gi.repository import Gdk
        capture_root = Gdk.get_default_root_window()
        assert capture_root is not None, 'no GTK display'

    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'APPDATA', 'LOCALAPPDATA'):
        directory = root / key
        directory.mkdir()
        env[key] = str(directory)
    output = root / 'run'
    env['CUMULUS_PROOF_DIR'] = str(output)
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve())
    if args.probe_script:
        env['CUMULUS_DISCOVERY_HOME_PROOF_SCRIPT'] = str(args.probe_script.resolve())
    captured = set()
    if args.recall:
        env['CUMULUS_DISCOVERY_RECALL_PROOF'] = '1'
        if args.screenshots:
            env['CUMULUS_PROOF_CAPTURE_ACK'] = '1'
        stages = ('ux_recall_ready', 'ux_recall_closed', 'ux_recall_playing')
    else:
        stages = ('home', 'dark', 'seeded_style', 'ranking', 'playlist_square', 'recent_style', 'retry', 'privacy')
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        process = subprocess.Popen([str(args.binary.resolve())], env=env, stdout=log, stderr=log)
        try:
            deadline = time.monotonic() + 75
            while process.poll() is None:
                if time.monotonic() >= deadline:
                    raise TimeoutError('discovery homepage WebView did not finish')
                if capture_root is not None:
                    for stage in stages:
                        if stage in captured or not (output / f'{stage}.json').is_file():
                            continue
                        picture = Gdk.pixbuf_get_from_window(
                            capture_root, 0, 0, capture_root.get_width(), capture_root.get_height())
                        assert picture is not None, 'no GTK frame'
                        png = output / f'{stage}.png'
                        assert picture.savev(str(png), 'png', [], []), 'GTK screenshot save failed'
                        if args.recall:
                            assert png.stat().st_size > 0, 'empty GTK screenshot'
                            (output / f'{stage}.captured').write_text('captured\n', encoding='utf-8')
                        captured.add(stage)
                time.sleep(.08)
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()

    failure = output / 'failed.json'
    assert not failure.exists(), failure.read_text(encoding='utf-8') if failure.exists() else ''
    assert process.returncode == 0, (root / 'host.log').read_text(encoding='utf-8')
    complete = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and not complete['realAccount']
    assert all((output / f'{stage}.json').is_file() for stage in stages)
    assert not args.screenshots or len(captured) == len(stages)
    if args.recall:
        report = verify_recall(output, complete, len(captured))
        (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
        print(json.dumps(report))
        return
    seeded = json.loads((output / 'seeded_style.json').read_text(encoding='utf-8'))
    assert seeded['styleCalls'] == [
        ['/api/homepage/category/daily/song/list',
         {'source': 'homepage', 'categoryId': '18', 'tagId': '21', 'songId': '81,82'}, False],
    ], seeded['styleCalls']
    home = json.loads((output / 'home.json').read_text(encoding='utf-8'))
    square = json.loads((output / 'playlist_square.json').read_text(encoding='utf-8'))
    assert not home['squareCalls'], 'recommendation square read before opening a heading'
    assert square['squareCalls'] == [
        ['/api/playlist/square/block/page', {'cursor': '', 'newStyle': True}, False],
    ], 'initial default feed read differs'
    assert complete['squareCalls'] == square['squareCalls'] + [
        ['/api/playlist/category/list', {'cat': '官方', 'limit': 24, 'newStyle': True}, False],
    ], 'warm destination or Back refetched an already loaded category'
    visits = complete['squareVisits']
    assert [(visit['source'], visit['openedTag']) for visit in visits] == [
        ('playlists', '推荐'), ('radar', '官方'),
    ], visits
    for visit in visits:
        assert visit['focus'] == visit['source'] + ':square', visit
        assert all(abs(before - after) <= 1 for before, after in zip(
            visit['before'], visit['after'], strict=True)), visit
        assert visit['before'][1] == 80, visit
    assert visits[0]['before'][0] < visits[1]['before'][0], visits
    retry = json.loads((output / 'retry.json').read_text(encoding='utf-8'))
    privacy = json.loads((output / 'privacy.json').read_text(encoding='utf-8'))
    paths = [path for path in privacy['paths'][len(retry['paths']):]
             if path in ('/api/user/setting', '/api/batch', '/api/pc/recent/listen/list',
                         '/api/pc/daily/rcmd/block')]
    assert paths.count('/api/batch') == 1 and paths.count('/api/user/setting') == 2, paths
    assert paths[-1] == '/api/user/setting', paths
    style = json.loads((output / 'recent_style.json').read_text(encoding='utf-8'))
    assert style['styleCalls'] == seeded['styleCalls'] + [
        ['/api/homepage/category/daily/song/list',
         {'source': '', 'categoryId': '', 'tagId': '', 'songId': ''}, False],
        ['/api/homepage/daily/song/config/get', {'limit': 20}, False],
    ], style['styleCalls']
    assert complete['styleCalls'] == style['styleCalls'], 'return to home triggered style reread'
    report = dict(ok=True, actualWebView=True, screenshots=len(captured), realAccount=False)
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


def verify_recall(output, complete, screenshots):
    def read(stage):
        return json.loads((output / f'{stage}.json').read_text(encoding='utf-8'))

    ready, closed, playing = (read(stage) for stage in ('ux_recall_ready', 'ux_recall_closed', 'ux_recall_playing'))
    assert ready['rows'] == [str(row) for row in range(911, 923)]
    assert ready['prefillSaved'] is True and ready['guideShown'] is False
    assert ready['ready']['state'] == closed['closed']['state'] == 'idle'
    assert ready['ready']['playId'] is None and closed['closed']['playId'] is None
    assert ready['mediaCalls'] == closed['mediaCalls'] == []
    for key in ('state', 'entryId', 'playId', 'queueRevision', 'position'):
        assert playing['beforePlay'][key] == closed['closed'][key], key
    assert closed['visibleMs'] > 2700 and complete['guideShown'] is True
    clock = closed['timing']
    assert (clock['insertedAt'] <= clock['firstPollAt'] <= clock['captureRequestedAt']
            <= clock['captureReturnedAt'] <= clock['removedAt']), clock
    assert clock['insertedAt'] <= clock['closingAt'] <= clock['removedAt'], clock
    timer = clock['timer']
    assert timer['requestedMs'] == 3000 and timer['firedAt'] - timer['scheduledAt'] >= 2900, clock
    assert clock['insertedAt'] <= timer['scheduledAt'] <= timer['firedAt'] <= clock['closingAt'], clock
    assert closed['visibleMs'] == clock['removedAt'] - clock['insertedAt'], clock
    assert playing['playing']['state'] == 'playing' and playing['playing']['position'] > .25
    assert playing['playing']['entryId'] == ready['ready']['entryId']
    assert str(playing['mediaCalls'][0][0]) == '911'
    assert len(complete['homepageBatches']) == 1
    blocks = json.loads(complete['homepageBatches'][0]['cursor'])['blockCodeOrderList']
    assert blocks == [
        'HOMPAGE_BLOCK_VIP_RCMD', 'HOMEPAGE_BLOCK_RED_SIMILAR_SONG',
        'HOMEPAGE_BLOCK_STYLE_RCMD', 'HOMEPAGE_MUSIC_PODCAST_RCMD_BLOCK',
        'HOMEPAGE_BLOCK_PLAYLIST_RCMD', 'HOMEPAGE_BLOCK_ALL_LISTEN', 'HOMEPAGE_BLOCK_HOT_RESOURCE',
    ]
    markers = sorted(path.stem for path in output.glob('ux_recall_*.captured'))
    assert markers == (['ux_recall_closed', 'ux_recall_playing', 'ux_recall_ready'] if screenshots else [])
    assert all(stage.get('captureHandshake', False) is bool(screenshots)
               for stage in (ready, closed, playing))
    return dict(ok=True, actualWebView=True, screenshots=screenshots, captureAcknowledgements=len(markers),
                states=3, realAccount=False,
                nativeNullPlaybackVerified=True, physicalAudioOutputVerified=False,
                prefillRows=ready['rows'], guideVisibleMs=closed['visibleMs'], mediaBeforePlay=0)


if __name__ == '__main__':
    main()
