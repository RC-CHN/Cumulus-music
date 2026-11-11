"""Production lyric/player layout with long metadata and synthetic libmpv/null audio."""
import argparse
import json
import os
import sys
from pathlib import Path
from history_row_next_smoke import share_qr_payload
from motion_smoke import launch


def verify_chorus_queue(root, complete, screenshots):
    stages = sorted((root / 'first').glob('ux_*.json'))
    assert len(stages) == complete['stageCount'] == 3 and complete['ok'] and complete['scrubClockVerified']
    assert complete['realMusicRequests'] == complete['accountWrites'] == 0
    entries, plays = [], []
    for stage, track, seconds, theme in zip(stages, ('1', '2', '1'), (4, 13, 4), ('light', 'dark', 'dark'), strict=True):
        row = json.loads(stage.read_text(encoding='utf-8'))
        player, queue = row['nativePlayer'], row['nativeQueue']
        assert row['chorusQueueFixture'] == {'batches': [['1', '2']], 'singles': 0}
        assert player['track']['id'] == row['trackId'] == track
        assert player['playId'] == row['playId'] and player['state'] == 'playing' and player['position'] > .12
        assert player['timeline']['total'] == 30 and row['markerVisible']
        assert row['theme'] == theme
        assert row['markerColor'] == ('rgba(255, 255, 255, 0.3)' if theme == 'dark' else 'rgba(40, 50, 72, 0.3)')
        assert abs(row['markerPercent'] - 100 * seconds / 30) < .01
        assert [item['track']['id'] for item in queue['tracks']] == ['1', '2']
        assert player['entryId'] == queue['current'] and player['queueRevision'] == queue['revision']
        entries.append(player['entryId'])
        plays.append(player['playId'])
    assert entries[0] == entries[2] != entries[1] and len(set(plays)) == 3
    assert complete['chorusQueueFixture'] == {'batches': [['1', '2']], 'singles': 0}
    if screenshots:
        for stage in stages:
            assert stage.with_suffix('.png').is_file()
            receipt = json.loads(stage.with_name(f'paint-{stage.name}').read_text(encoding='utf-8'))
            sample, = receipt['samples']
            assert sample['ok']
    return dict(complete, actualWebView=True, nativeQueueCache=True, screenshots=len(stages) if screenshots else 0,
                physicalDisplayVerified=False, physicalAudioOutputVerified=False)


def sample_capture(path, row, point):
    import gi
    gi.require_version('GdkPixbuf', '2.0')
    from gi.repository import GdkPixbuf
    image = GdkPixbuf.Pixbuf.new_from_file(str(path))
    x = int((row['screenX'] + point['x']) * row['ratio'])
    y = int((row['screenY'] + point['y']) * row['ratio'])
    assert 0 <= x < image.get_width() and 0 <= y < image.get_height()
    offset = y * image.get_rowstride() + x * image.get_n_channels()
    return list(image.get_pixels()[offset:offset + 3])


def verify_fm_chorus(root, complete, screenshots):
    first = root / 'first'
    names = ('ux_01_fm_personal', 'ux_02_fm_familiar_held',
             'ux_03_fm_familiar', 'ux_04_fm_personal_again', 'ux_05_fm_thumb_priority')
    assert [path.stem for path in sorted(first.glob('ux_*.json'))] == list(names)
    assert complete['ok'] and complete['stageCount'] == len(names)
    assert complete['accountWrites'] == complete['realMusicRequests'] == 0
    locale = None
    # The probe records GTK startup locale only on Linux.
    if sys.platform == 'linux':
        locale = json.loads((first / 'locale.json').read_text(encoding='utf-8'))
        assert locale['numeric'] == 'C' and locale['messages'] != 'C' and locale['characters'] != 'C'
    plays, entries, markers, paint = [], [], [], {}
    for index, (name, seconds) in enumerate(zip(names, (4, None, 13, 4, 4), strict=True)):
        row = json.loads((first / f'{name}.json').read_text(encoding='utf-8'))
        native, queue, fixture = row['nativePlayer'], row['nativeQueue'], row['fmChorusFixture']
        familiar = index in (1, 2)
        assert row['nativeFmSource'] == (
            {'kind': 'familiar', 'source_id': '["41","42"]'} if familiar else {'kind': 'personal'})
        assert native['fm'] and native['state'] == ('paused' if index == 4 else 'playing') and native['error'] is None
        assert native['track']['id'] == row['trackId'] == '1' and native['playId'] == row['playId']
        assert native['position'] > .12 and row['position'] > .12 and native['timeline']['total'] == 30
        assert native['queueRevision'] == queue['revision'] and native['entryId'] == queue['current']
        assert [item['track']['id'] for item in queue['tracks']] == (
            ['1', '21', '22'] if familiar else ['1', '11', '12'])
        assert row['audioFactoryCount'] == 1 and fixture['singles'] == 0
        assert fixture['recommendations'] == ['personal', 'familiar'][:1 + int(index > 0)]
        assert fixture['batches'] == [[1, 11, 12], [1, 21, 22]][:1 + int(index > 0)]
        assert fixture['familiarReleased'] == fixture['familiarReturned'] == (index >= 2)
        assert row['markerVisible'] == (seconds is not None)
        if index == 4:
            assert native['position'] == row['position'] == 4
        if seconds is None:
            assert row['position'] < 4 and row['markerPercent'] is None and row['missingSample'] is not None
        else:
            assert abs(row['markerPercent'] - 100 * seconds / 30) < .01
        if screenshots:
            png = first / f'{name}.png'
            assert png.is_file()
            if seconds is None:
                pixel = sample_capture(png, row, row['missingSample'])
                assert pixel != paint[names[0]], 'old Personal marker pixel must be gone'
                paint[name] = pixel
            elif index == 4:
                sample = row['thumbSample']
                # GTK draws the native thumb with its own accent. Compare it
                # against the same thumb outside the four-pixel painted rail.
                assert sample['markerColor'] == 'rgb(255, 255, 255)'
                center = sample_capture(png, row, sample['center'])
                outside = sample_capture(png, row, sample['outsideRail'])
                assert center == outside and center[0] > max(center[1:]), 'thumb stays above the white marker'
                paint[name] = center
            else:
                proof = json.loads((first / f'paint-{name}.json').read_text(encoding='utf-8'))
                sample, = proof['samples']
                assert sample['ok']
                paint[name] = sample['actual']
        plays.append(native['playId'])
        entries.append(native['entryId'])
        markers.append(row['markerPercent'])
    assert plays[1] == plays[2] and plays[3] == plays[4] and len(set(plays)) == 3
    assert entries[0] == entries[3] == entries[4] and entries[1] == entries[2] and entries[0] != entries[1]
    assert complete['fmChorusFixture'] == {
        'recommendations': ['personal', 'familiar'], 'batches': [[1, 11, 12], [1, 21, 22]],
        'singles': 0, 'familiarReleased': True, 'familiarReturned': True,
    }
    return dict(complete, actualWebView=True, nativeFmChorusCache=True, markerPercents=markers,
                sampledPixels=paint, screenshots=len(names) if screenshots else 0, locale=locale,
                physicalDisplayVerified=False, physicalAudioOutputVerified=False)


def verify_current_share(stages, complete, menu, screenshots):
    names = ('cancelled', 'denied', 'copied', 'wechat', 'stale', 'subscription')
    expected = [dict(path='/api/song/enhance/privilege', args={'ids': json.dumps([song])}, cached=False, sp=sp)
                for song, sp in zip(['2'] * 10 + ['1'] * 2,
                                    (1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1), strict=True)]
    assert menu['playerSongShareCalls'] == []
    assert complete['playerSongShareCalls'] == expected
    rows = []
    for name, count in zip(names, (1, 3, 5, 7, 10, 12), strict=True):
        match, = [file for file in stages if file.stem.endswith('_current_share_' + name)]
        row = json.loads(match.read_text(encoding='utf-8'))
        assert row['playerSongShareCalls'] == expected[:count], name
        rows.append(row)
    cancelled, denied, copied, wechat, stale, subscription = rows
    assert cancelled['forgedRejected'] is True
    assert all(row['focusRestored'] is True for row in (cancelled, denied, copied))
    assert (denied['notice'], denied['clipboardUntouched']) == ('当前资源暂不支持分享。', True)
    assert (copied['notice'], copied['nativeClipboard']) == ('复制成功', True)
    assert (wechat['title'], wechat['hint'], wechat['qrWidth']) == ('分享到微信', '打开微信, 扫一扫', 180)
    assert len(wechat['qrModules']) == 25
    assert share_qr_payload(wechat['qrModules']) == 'https://music.163.com/song?id=2'
    if screenshots:
        qr_path, = [file for file in stages if file.stem.endswith('_current_share_wechat')]
        paint = json.loads(qr_path.with_name('paint-' + qr_path.name).read_text(encoding='utf-8'))
        assert [sample['actual'] for sample in paint['samples']] == [[255, 255, 255], [0, 0, 0]]
        assert all(sample['ok'] for sample in paint['samples'])
    first = cancelled['nativePlayer']
    for row in rows[:4]:
        native, queue = row['nativePlayer'], row['nativeQueue']
        assert native['playId'] == row['playId'] == menu['nativePlayer']['playId']
        assert native['track']['id'] == row['trackId'] == '2' and native['state'] == 'paused'
        assert native['currentSongShareAvailable'] is True
        assert native['entryId'] == queue['current'] == first['entryId']
        assert native['queueRevision'] == queue['revision'] == first['queueRevision']
        assert queue == cancelled['nativeQueue']
        assert abs(native['position'] - row['position']) < .1
        assert abs(native['position'] - first['position']) < .1
    assert stale['oldChooserRetired'] is True and stale['shareHidden'] is True
    assert stale['clipboardUntouched'] is True and stale['stale'] == 'stale'
    assert stale['oldPlayId'] == first['playId'] != stale['nativePlayer']['playId']
    assert stale['nativePlayer']['track']['id'] == stale['trackId'] == '3'
    assert not stale['nativePlayer'].get('currentSongShareAvailable')
    native = subscription['nativePlayer']
    assert subscription['originalLeaseRejected'] is True and subscription['replacementReady'] is True
    assert subscription['clipboardUntouched'] is True
    assert native['state'] == 'paused' and native['track']['id'] == subscription['trackId'] == '1'
    assert native['playId'] == subscription['playId'] == complete['nativePlayer']['playId']
    assert native['entryId'] == subscription['entryId'] == subscription['nativeQueue']['current']
    assert native['queueRevision'] == subscription['queueRevision'] == subscription['nativeQueue']['revision']
    assert native['epoch'] == subscription['epoch'] == subscription['nativeQueue']['epoch']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument('--baseline', action='store_true')
    modes.add_argument('--chorus-queue', action='store_true')
    modes.add_argument('--fm-chorus', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1', CUMULUS_PLAYER_LAYOUT_PROOF='1')
    if args.chorus_queue:
        env['CUMULUS_PLAYER_CHORUS_QUEUE_PROOF'] = '1'
    elif args.fm_chorus:
        env['CUMULUS_FM_CHORUS_PROOF'] = '1'
    elif not args.baseline:
        env['CUMULUS_PLAYER_WAIT_PROOF'] = '1'
        env['CUMULUS_PLAYER_SONG_SHARE_PROOF'] = '1'
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        directory = root / 'os-profile' / name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve())
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    name = 'player_layout_ui_probe.js'
    if args.chorus_queue:
        name = 'player_chorus_queue_ui_probe.js'
    elif args.fm_chorus:
        name = 'player_fm_chorus_ui_probe.js'
    source = Path(__file__).resolve().parents[2] / 'desktop/examples' / name
    script = root / 'player-layout.js'
    script.write_text('window.__playerLayoutBaseline=' + json.dumps(args.baseline) + ';\n'
                      + source.read_text(encoding='utf-8'), encoding='utf-8')
    env['CUMULUS_UX_SCRIPT'] = str(script)
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, root / 'first', args.screenshots)
    failed = root / 'first/failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'probe exited {code}'
    complete = json.loads((root / 'first/complete.json').read_text(encoding='utf-8'))
    if args.fm_chorus:
        report = verify_fm_chorus(root, complete, args.screenshots)
        (root / 'audit.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
        print(json.dumps({k: report[k] for k in ['ok', 'nativeFmChorusCache', 'stageCount', 'screenshots']}))
        return
    if args.chorus_queue:
        report = verify_chorus_queue(root, complete, args.screenshots)
        (root / 'audit.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
        print(json.dumps({k: report[k] for k in ['ok', 'nativeQueueCache', 'stageCount', 'screenshots']}))
        return
    expected = 20 if args.baseline else 36
    assert complete['ok'] and complete['stageCount'] == expected and complete['focusRestored']
    assert complete['realMusicRequests'] == complete['accountWrites'] == 0
    assert complete['nativePlayer']['state'] == 'paused' and complete['nativePlayer']['position'] > 0
    assert args.baseline or all(complete[key] for key in (
        'layoutPassed', 'lyricFollowVerified', 'lyricTimelineVerified', 'lyricFontVerified',
        'lyricStopVerified',
        'lyricModesVerified', 'chorusMarkerVerified', 'seekTimePreviewVerified',
        'slowPlaybackWaitVerified', 'seekStepVerified', 'audioOutputRecoveryVerified',
        'recommendationToggleVerified', 'currentRowToggleVerified', 'playerCommentCountVerified',
        'currentTrackMenuVerified'))
    stages = list((root / 'first').glob('ux_*.json'))
    assert len(stages) == expected
    if not args.baseline:
        assert complete['playerLayoutCatalogueCalls'] == [
            ['/api/usertool/sound/mobile/promote', {}],
            ['/api/usertool/sound/mobile/all', {}],
        ]
        seek_path, = [stage for stage in stages if stage.stem.endswith('_slow_native_seek_wait')]
        seek = json.loads(seek_path.read_text(encoding='utf-8'))
        assert seek['nativePlayer']['nativeSeeking'] and seek['nativePlayer']['state'] == 'playing'
        assert seek['nativePlayer']['playId'] == seek['playId']
        stopped_path, = [stage for stage in stages if stage.stem.endswith('_lyrics_stopped_read')]
        stopped = json.loads(stopped_path.read_text(encoding='utf-8'))
        assert stopped['cleared'] and stopped['playId'] is None
        menu_path, = [stage for stage in stages if stage.stem.endswith('_current_track_menu')]
        menu = json.loads(menu_path.read_text(encoding='utf-8'))
        assert menu['menu'] and menu['nativePlayer']['state'] == 'paused'
        assert menu['nativePlayer']['playId'] == menu['playId']
        assert menu['nativePlayer']['track']['id'] == menu['trackId'] == '2'
        verify_current_share(stages, complete, menu, args.screenshots)
        for label, expected_badge in [('long_760_dark', '999+'), ('short_760_dark', '25')]:
            shot, = [stage for stage in stages if stage.stem.endswith('_' + label)]
            frame = json.loads(shot.read_text(encoding='utf-8'))
            assert frame['commentBadge'] == expected_badge and frame['checks']['commentBadgeContained']
        waiting, = [stage for stage in stages if stage.stem.endswith('_slow_playback_wait')]
        native_wait = json.loads(waiting.read_text(encoding='utf-8'))
        assert native_wait['nativePlayer']['state'] == 'buffering'
        assert native_wait['sourceAudio']['forcedBuffering']
        recovery, = [stage for stage in stages if stage.stem.endswith('_audio_output_recovery')]
        native_error = json.loads(recovery.read_text(encoding='utf-8'))
        assert native_error['nativePlayer']['state'] == 'error'
        assert native_error['nativePlayer']['errorCode'] == 'audio_output_failed'
        assert native_error['nativePlayer']['playId'] == native_error['failedId']
        assert native_error['nativePlayer']['track']['id'] == complete['nativePlayer']['track']['id']
        assert native_error['target'] == '音频输出'
        toggle, = [stage for stage in stages if stage.stem.endswith('_daily_play_toggle')]
        daily = json.loads(toggle.read_text(encoding='utf-8'))
        assert daily['nativePlayer']['state'] == 'paused' and daily['button'] == '继续播放'
        assert daily['nativePlayer']['playId'] == daily['playId']
        assert daily['nativePlayer']['track']['id'] == daily['trackId']
        assert daily['nativePlayer']['recommendationOrigin']['kind'] == 'daily'
        row_toggle, = [stage for stage in stages if stage.stem.endswith('_current_row_toggle')]
        row = json.loads(row_toggle.read_text(encoding='utf-8'))
        assert row['nativePlayer']['state'] == 'paused'
        assert row['nativePlayer']['playId'] == row['playId']
        assert row['nativePlayer']['track']['id'] == row['trackId']
    if args.screenshots:
        assert len(list((root / 'first').glob('ux_*.png'))) == expected
    report = dict(complete, actualWebView=True, screenshots=expected if args.screenshots else 0,
                  physicalDisplayVerified=False, physicalAudioOutputVerified=False)
    (root / 'audit.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({k: report[k] for k in ['ok', 'layoutPassed', 'stageCount', 'screenshots']}))


if __name__ == '__main__':
    main()
