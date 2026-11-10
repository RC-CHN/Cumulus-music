"""Search result categories and interactions via production IPC and an isolated in-memory backend."""
import argparse
from concurrent.futures import ThreadPoolExecutor
from contextlib import closing
import json
import os
from pathlib import Path
import sqlite3
import stat
import sys
from threading import Event
import time
from motion_smoke import launch
from search_signed_trace import assignment_arguments, comprehensive_reads


def signed_result_reads(complete, result_reads, expected_total):
    repository = Path(__file__).resolve().parents[2]
    return comprehensive_reads(complete, assignment_arguments(repository), result_reads, expected_total)


def drag_playlist_column(output, cancelled, capture):
    """Optional X11 pointer injection; the portable probe always checks keyboard input."""
    import ctypes
    from ctypes import c_char_p, c_int, c_uint, c_ulong, c_void_p

    stage = output / 'ux_04_playlist_table_columns.json'
    deadline = time.monotonic() + 60
    while not cancelled.is_set() and time.monotonic() < deadline:
        try:
            if not capture or stage.with_suffix('.captured').exists():
                point = json.loads(stage.read_text(encoding='utf-8'))['pointer']
                break
        except (FileNotFoundError, json.JSONDecodeError):
            pass
        cancelled.wait(.1)
    else:
        raise AssertionError('missing playlist pointer target')

    x11, xtest = ctypes.CDLL('libX11.so.6'), ctypes.CDLL('libXtst.so.6')
    x11.XOpenDisplay.argtypes, x11.XOpenDisplay.restype = [c_char_p], c_void_p
    x11.XSync.argtypes = [c_void_p, c_int]
    x11.XCloseDisplay.argtypes = [c_void_p]
    xtest.XTestFakeMotionEvent.argtypes = [c_void_p, c_int, c_int, c_int, c_ulong]
    xtest.XTestFakeButtonEvent.argtypes = [c_void_p, c_uint, c_int, c_ulong]
    display = x11.XOpenDisplay(None)
    assert display, 'trusted pointer proof requires an isolated X11 display'
    try:
        x, y, ratio = point['x'], point['y'], point['ratio']
        assert xtest.XTestFakeMotionEvent(display, -1, x, y, 0)
        assert xtest.XTestFakeButtonEvent(display, 1, 1, 0)
        x11.XSync(display, 0)
        time.sleep(.1)
        assert xtest.XTestFakeMotionEvent(display, -1, x + round(24 * ratio), y + round(80 * ratio), 0)
        x11.XSync(display, 0)
        time.sleep(.1)
        assert xtest.XTestFakeButtonEvent(display, 1, 0, 0)
        x11.XSync(display, 0)
    finally:
        x11.XCloseDisplay(display)


def verify_voice_download_file(path, directory):
    """Check the real descendant even if Windows spells the same drive two ways."""
    # The intended scratch directory must itself be real; never permit a
    # symlink or Windows junction to make an outside directory the fixture root.
    root = directory.lstat()
    assert (stat.S_ISDIR(root.st_mode)
            and not getattr(root, 'st_file_attributes', 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT), (
                'voice download destination is not an ordinary directory')
    target = Path(path).resolve(strict=True)
    expected = directory.resolve(strict=True)
    # Rust persists extended-length Windows paths. CPython retains that prefix
    # if the stored input has it, so lexical is_relative_to can reject the same
    # physical directory. Compare filesystem identity after following links.
    assert any(parent.samefile(expected) for parent in target.parents), 'voice download escaped its destination'
    assert target.suffix == '.mp3', 'voice download has the wrong format'
    state = target.stat()
    assert stat.S_ISREG(state.st_mode) and state.st_size > 0, 'voice download is not a nonempty file'


def verify_artist_top50(root, group, screenshots):
    first = root / 'first'
    read = lambda path: json.loads(path.read_text(encoding='utf-8'))
    complete = read(first / 'complete.json')
    assert complete['ok'] and complete['artistTop50Verified'] and complete['stageCount'] == 7
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    states = {p.stem.split('_', 2)[2]: read(p) for p in sorted(first.glob('ux_*.json'))}
    expected_states = (
        ('artist_best_play', 'all', '101', '92', 'playing'),
        ('artist_best_next', 'all', '101', '91', 'playing'),
        ('artist_regular_play', 'artists', '102', '96', 'playing'),
        ('artist_regular_pause', 'artists', '102', '96', 'paused'),
        ('artist_regular_resume', 'artists', '102', '96', 'playing'),
        ('artist_failure_keeps_queue', 'artists', '102', '96', 'playing'),
        ('artist_back_to_search', 'all', '102', '96', 'playing'),
    )
    assert tuple(states) == tuple(name for name, *_ in expected_states)
    ids = {'101': ['92', '91', '93'], '102': ['96', '94', '95'], '103': ['98', '97', '99']}
    url = '/api/song/enhance/player/url/v1'
    reads = [call for call in complete['artistTop50Calls'] if 'path' in call and call['path'] != url]
    assert [call['assignment'] for call in complete['artistTop50Calls'] if 'assignment' in call] == [group]
    oracle = []
    for artist, tracks in ids.items():
        if group == 't0':
            oracle.append(dict(path='/api/v2/artist/songs', args=dict(id=artist, limit=50, offset=0), cache=False))
        else:
            oracle.extend((
                dict(path='/api/artist/v3/detail', args=dict(id=artist, top=50, e_r=False), cache=True),
                dict(path='/api/song/enhance/privilege',
                     args=dict(ids=json.dumps(tracks, separators=(',', ':'))), cache=False),
            ))
    assert reads == oracle, reads
    for i, (name, category, artist, track, transport) in enumerate(expected_states):
        stage = states[name]
        p, queue = stage['nativePlayer'], stage['nativeQueue']
        assert stage['query'] == '合成歌手热歌' and stage['category'] == category
        assert p['track']['id'] == stage['trackId'] == track and p['resourceKind'] == 'catalog'
        assert p['playId'] == stage['playId'] and p['state'] == transport and p['position'] > .14
        assert p['origin']['kind'] == 'artist' and p['origin']['id'] == artist
        assert queue['revision'] == p['queueRevision'] == stage['queueRevision']
        assert queue['current'] == p['entryId']
        assert [row['track']['id'] for row in queue['tracks']] == ids[artist]
        assert [row['entryId'] for row in queue['tracks']] == stage['entryIds']
        observed = [call for call in stage['artistTop50Calls'] if 'path' in call and call['path'] != url]
        requests = 1 if i < 2 else 2 if i < 5 else 3
        assert observed == oracle[:requests * (2 if group == 't1' else 1)], (name, observed)
    first_play, next_play, regular = (states[name] for name, *_ in expected_states[:3])
    assert len({first_play['playId'], next_play['playId'], regular['playId']}) == 3
    assert first_play['entryIds'] == next_play['entryIds']
    assert all(stage['playId'] == regular['playId'] and stage['entryIds'] == regular['entryIds']
               for stage in list(states.values())[2:])
    assert states['artist_failure_keeps_queue']['error']
    media = [call for call in complete['artistTop50Calls'] if call.get('path') == url]
    authorized = [str(id_) for call in media for id_ in json.loads(call['args']['ids'])]
    assert authorized[0] == '92' and {'92', '91', '96'} <= set(authorized) <= set(ids['101'] + ids['102'])
    assert all(call['args']['trialMode'] == -1 and not call['cache'] for call in media)
    if os.name != 'nt':
        locale = read(first / 'locale.json')
        assert locale['numeric'] == 'C'
        assert all(locale[key].casefold().replace('-', '') == 'en_us.utf8'
                   for key in ('messages', 'characters'))
    pictures = len(list(first.glob('ux_*.png')))
    assert pictures == (7 if screenshots else 0)
    report = dict(ok=True, actualWebView=True, nativeNullPlaybackVerified=True, group=group,
                  states=list(states), artistRequests=reads, screenshots=pictures,
                  realMusicRequests=0, realAccountWrites=0)
    (root / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n',
                                      encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


def verify_offline_route(root, screenshots):
    first = root / 'first'
    read = lambda path: json.loads(path.read_text(encoding='utf-8'))
    stages = (
        ('cold_offline', 0, None), ('local_diagnostics', 0, None),
        ('first_online', 1, 'all'), ('classified', 2, 'songs'),
        ('known_offline', 2, None), ('restored_category', 2, 'songs'),
        ('interrupted_offline', 3, None), ('new_online', 4, 'all'),
        ('reachable_error', 5, 'all'), ('explicit_retry', 6, 'all'),
    )
    expected = [
        ['all', '合成断网前搜索'], ['songs', '合成断网前搜索'],
        ['all', '合成迟到搜索'], ['all', '合成恢复搜索'],
        ['all', '合成读取失败'], ['all', '合成读取失败'],
    ]
    classified_scroll = None
    for index, (label, count, category) in enumerate(stages, 1):
        path = first / f'ux_{index:02}_{label}.json'
        row = read(path)
        assert row['label'] == label and row['category'] == category
        assert row['hasOffline'] == (category is None) and row['browserOnline'] == (category is not None)
        assert row['diagnostics'] == (label == 'local_diagnostics')
        assert row['offlineResultCalls'] == row['requests'] == expected[:count], (label, row)
        if label == 'classified':
            classified_scroll = row['trackScroll']
            assert classified_scroll > 0
        if label == 'restored_category':
            assert abs(row['trackScroll'] - classified_scroll) <= 1
        if screenshots:
            assert path.with_suffix('.png').is_file()
    complete = read(first / 'complete.json')
    assert complete['ok'] and complete['stageCount'] == len(stages)
    assert complete['offlineResultCalls'] == complete['requests'] == expected
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    if os.name != 'nt':
        locale = read(first / 'locale.json')
        assert locale['numeric'] == 'C'
        assert all(locale[key].casefold().replace('-', '') == 'en_us.utf8'
                   for key in ('messages', 'characters')), locale
    report = dict(ok=True, actualWebView=True, nativeIpc=True, browserConnectivitySynthetic=True,
                  localDiagnosticsVerified=True, restoredTrackScroll=classified_scroll,
                  networkNamespaceWithoutRoutes=sys.platform == 'linux',
                  stages=[label for label, _, _ in stages],
                  searchReads=expected, screenshots=len(stages) if screenshots else 0,
                  realMusicRequests=0, realAccountWrites=0)
    (root / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n',
                                      encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--v3', action='store_true')
    mode.add_argument('--carousel', action='store_true')
    mode.add_argument('--hot-card', action='store_true')
    mode.add_argument('--hot-playback', action='store_true')
    mode.add_argument('--voice-direct', action='store_true')
    mode.add_argument('--voice-author', action='store_true')
    mode.add_argument('--voice-download-submit', action='store_true')
    mode.add_argument('--card-matches', action='store_true')
    mode.add_argument('--correction-original', action='store_true')
    mode.add_argument('--artist-gallery', action='store_true')
    mode.add_argument('--artist-top50', choices=('t0', 't1'))
    mode.add_argument('--people-categories', action='store_true')
    mode.add_argument('--search-next', action='store_true')
    mode.add_argument('--song-waterfall', action='store_true')
    mode.add_argument('--voice-queue', action='store_true')
    mode.add_argument('--voice-next', action='store_true')
    mode.add_argument('--voice-links', action='store_true')
    mode.add_argument('--podcast-follow', action='store_true')
    mode.add_argument('--mv-play', action='store_true')
    mode.add_argument('--mv-fallback', action='store_true')
    mode.add_argument('--playlist-creator', action='store_true')
    mode.add_argument('--collection-next', action='store_true')
    mode.add_argument('--album-artist', action='store_true')
    mode.add_argument('--suggestion-focus', action='store_true')
    mode.add_argument('--filtered-pagination', action='store_true')
    mode.add_argument('--offline-route', action='store_true')
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--trusted-playlist-pointer', action='store_true',
                        help='also drag a resize handle using XTest under isolated Xvfb')
    args = parser.parse_args()
    if (args.offline_route and sys.platform == 'linux' and
            len(Path('/proc/net/route').read_text().splitlines()) != 1):
        parser.error('--offline-route on Linux requires a sealed no-route network namespace')
    if args.trusted_playlist_pointer and (not args.playlist_creator or not os.environ.get('DISPLAY')):
        parser.error('--trusted-playlist-pointer requires --playlist-creator under isolated Xvfb')
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1',
               CUMULUS_SEARCH_UI_PROOF='1')
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        directory = root / 'os-profile' / name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    probe = 'search_offline_ui_probe.js' if args.offline_route else 'search_results_ui_probe.js'
    source = Path(__file__).resolve().parents[2] / 'desktop/examples' / probe
    if args.offline_route:
        env['CUMULUS_SEARCH_OFFLINE_PROOF'] = '1'
        if os.name != 'nt':
            env['LC_ALL'] = 'en_US.UTF-8'
    script = root / 'search-results.js'
    prefix = ''
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if args.people_categories:
        prefix = 'window.__cumulusSearchPeopleCategoriesProof = true;\n'
    if args.search_next:
        prefix = 'window.__cumulusSearchNextProof = true;\n'
    if args.song_waterfall:
        prefix = 'window.__cumulusSearchSongWaterfallProof = true;\n'
    if args.voice_author:
        prefix = 'window.__cumulusSearchVoiceAuthorProof = true;\n'
        env['CUMULUS_SEARCH_VOICE_AUTHOR_PROOF'] = '1'
    if args.voice_queue:
        prefix = 'window.__cumulusSearchVoiceQueueProof = true;\n'
    if args.voice_next:
        prefix = 'window.__cumulusSearchVoiceNextProof = true;\n'
    if args.voice_links:
        env['CUMULUS_SEARCH_VOICE_LINKS_PROOF'] = '1'
        prefix = ('window.__cumulusSearchVoiceLinksProof = true;\n'
                  + source.with_name('search_voice_links_ui_probe.js').read_text(encoding='utf-8') + '\n')
    if args.podcast_follow:
        prefix = 'window.__cumulusSearchPodcastFollowProof = true;\n'
    if args.mv_play:
        prefix = 'window.__cumulusSearchMvProof = true;\n'
        env['CUMULUS_SEARCH_MV_PROOF'] = '1'
    if args.mv_fallback:
        prefix = 'window.__cumulusSearchMvFallbackProof = true;\n'
        env.update(CUMULUS_SEARCH_MV_PROOF='1', CUMULUS_SEARCH_MV_FALLBACK_PROOF='1',
                   CUMULUS_SEARCH_V3_PROOF='1')
    if args.playlist_creator:
        prefix = 'window.__cumulusSearchPlaylistCreatorProof = true;\n'
        if args.trusted_playlist_pointer:
            prefix += 'window.__cumulusSearchTrustedPlaylistPointer = true;\n'
    if args.collection_next:
        prefix = 'window.__cumulusSearchCollectionNextProof = true;\n'
    if args.filtered_pagination:
        prefix = 'window.__cumulusSearchFilteredPaginationProof = true;\n'
    if args.suggestion_focus:
        prefix = 'window.__cumulusSearchSuggestionFocusProof = true;\n'
    if args.album_artist:
        prefix = 'window.__cumulusSearchAlbumArtistProof = true;\n'
    if args.correction_original:
        prefix = 'window.__cumulusSearchOriginalCorrectionProof = true;\n'
        env['CUMULUS_SEARCH_CORRECTION_PROOF'] = '1'
    if args.artist_gallery:
        prefix = 'window.__cumulusArtistGalleryProof = true;\n'
    if args.artist_top50:
        env['CUMULUS_SEARCH_ARTIST_TOP50_PROOF'] = args.artist_top50
        prefix = ('window.__cumulusSearchArtistTop50Proof = true;\n'
                  + source.with_name('search_artist_top50_ui_probe.js').read_text(encoding='utf-8') + '\n')
    if args.card_matches:
        prefix = 'window.__cumulusSearchCardMatchesProof = true;\n'
    if args.voice_direct or args.voice_download_submit:
        prefix = 'window.__cumulusSearchVoiceDirectProof = true;\n'
        if args.voice_direct:
            env['CUMULUS_SEARCH_VOICE_PENDING_PROOF'] = '1'
            prefix += 'window.__cumulusSearchVoicePendingProof = true;\n'
        if args.voice_download_submit:
            env['CUMULUS_VOICE_DOWNLOAD_UI_PROOF'] = '1'
            prefix += 'window.__cumulusVoiceDownloadDirectory = ' + json.dumps(str(root / 'voice-media')) + ';\n'
    if args.v3:
        env['CUMULUS_SEARCH_V3_PROOF'] = '1'
        prefix = 'window.__cumulusSearchV3Proof = true;\n'
    if args.carousel:
        env['CUMULUS_SEARCH_CAROUSEL_PROOF'] = '1'
        prefix = 'window.__cumulusSearchCarouselProof = true;\n'
    if args.hot_playback:
        env['CUMULUS_SEARCH_V3_PROOF'] = '1'
        env['CUMULUS_SEARCH_HOT_PLAYBACK_PROOF'] = '1'
        env['CUMULUS_SEARCH_VOICE_PENDING_PROOF'] = '1'
        prefix = 'window.__cumulusSearchHotPlaybackProof = true;\n'
    if args.hot_card:
        env['CUMULUS_SEARCH_V3_PROOF'] = '1'
        env['CUMULUS_SEARCH_HOT_CARD_PROOF'] = '1'
        prefix = 'window.__cumulusSearchHotCardProof = true;\n'
    script.write_text(prefix + source.read_text(encoding='utf-8'), encoding='utf-8')
    env['CUMULUS_UX_SCRIPT'] = str(script)
    env['CUMULUS_SEARCH_USER_PROFILE_PROOF'] = '1'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        if args.trusted_playlist_pointer:
            cancelled = Event()
            with ThreadPoolExecutor(max_workers=1) as workers:
                drag = workers.submit(drag_playlist_column, root / 'first', cancelled, args.screenshots)
                try:
                    code = launch(args.binary.resolve(), env, log, root / 'first', args.screenshots)
                finally:
                    cancelled.set()
                if code == 0 and not (root / 'first/failed.json').exists():
                    drag.result()
        else:
            code = launch(args.binary.resolve(), env, log, root / 'first', args.screenshots)
    failed = root / 'first/failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'probe exited {code}'
    if args.offline_route:
        verify_offline_route(root, args.screenshots)
        return
    if args.artist_top50:
        verify_artist_top50(root, args.artist_top50, args.screenshots)
        return
    if args.voice_links:
        first = root / 'first'
        read = lambda p: json.loads(p.read_text(encoding='utf-8'))
        complete = read(first / 'complete.json')
        states = [read(p) for p in sorted(first.glob('ux_*.json'))]
        assert [state['label'] for state in states] == [
            'voice_link_best_match', 'voice_link_comprehensive_menu', 'voice_link_comprehensive_copy',
            'voice_link_classified_share', 'voice_link_classified_copy', 'voice_link_retired',
        ]
        assert complete['ok'] and complete['voiceLinksVerified'] and complete['stageCount'] == len(states)
        assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
        assert all(state['voiceNextCalls'] == [] for state in [*states, complete])
        reads = [state['syntheticSearchCalls'] for state in states]
        assert reads[0] == reads[1] == reads[2] < reads[3] == reads[4] < reads[5], reads
        playback = states[0]['playback']
        assert playback['playId'] and playback['entryId'] and playback['position'] > .15
        assert len(playback['entryIds']) == 2
        assert all(state['playback'] == playback for state in states)
        for state in [*states, complete]:
            native = state['nativePlayer']
            assert native['state'] == 'paused' and native['track']['id'] == '1'
            assert native['queueLen'] == len(playback['entryIds'])
            assert all(native[key] == playback[key]
                       for key in ('playId', 'entryId', 'queueRevision', 'position'))
        links = [state['clipboard'] for state in states[2:]]
        assert links == ['https://music.163.com/dj?id=401'] * 2 + ['https://music.163.com/dj?id=402'] * 2
        assert states[3]['link'] == links[-1]
        pictures = len(list(first.glob('ux_*.png')))
        assert pictures == (len(states) if args.screenshots else 0)
        if args.screenshots:
            painted = [read(p) for p in sorted(first.glob('paint-ux_*.json'))]
            assert len(painted) == 2 and all(item['samples'] and
                                             all(sample['ok'] for sample in item['samples'])
                                             for item in painted)
        if os.name != 'nt':
            locale = read(first / 'locale.json')
            assert locale['numeric'] == 'C'
            assert all(locale[key].casefold().replace('-', '') == 'en_us.utf8'
                       for key in ('messages', 'characters'))
        report = dict(ok=True, actualWebView=True, nativeNullPlaybackVerified=True,
                      nativeClipboardVerified=True, states=[state['label'] for state in states],
                      screenshots=pictures, clipboard=links, syntheticSearchReads=reads,
                      realMusicRequests=0, realAccountWrites=0)
        (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
        print(json.dumps(report))
        return
    if args.voice_author:
        first = root / 'first'
        complete = json.loads((first / 'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['voiceAuthorVerified'] and complete['stageCount'] == 3
        assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
        assert complete['searchProfileCalls'] == [
            {'all': True, 'userId': '501'}, {'uid': '501', 'offset': 0, 'limit': 1000},
        ]
        states = {p.stem.split('_', 2)[2]: json.loads(p.read_text(encoding='utf-8'))
                  for p in first.glob('ux_*.json')}
        assert set(states) == {'voice_author_link', 'voice_author_profile', 'voice_author_return'}
        assert states['voice_author_link']['position'] > 0
        assert abs(states['voice_author_return']['position'] - states['voice_author_link']['position']) <= 1
        assert states['voice_author_return']['focus']
        assert len(list(first.glob('ux_*.png'))) == (3 if args.screenshots else 0)
        print(json.dumps(dict(ok=True, actualWebView=True, voiceAuthorVerified=True, stageCount=3)))
        return
    if args.hot_playback:
        first = root / 'first'
        complete = json.loads((first / 'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['hotPlaybackVerified'] and complete['stageCount'] == 8
        assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
        assert len(list(first.glob('ux_*.png'))) == (8 if args.screenshots else 0)
        states = {p.stem.split('_', 2)[2]: json.loads(p.read_text(encoding='utf-8'))
                  for p in first.glob('ux_*.json')}
        cover, details, paused, resumed = [states[name]['nativePlayer'] for name in
                                          ('hot_cover_song', 'hot_details_song', 'hot_cover_pause', 'hot_details_resume')]
        assert cover['track']['id'] == '1' and cover['position'] > .2 and cover['queueLen'] == 1
        pending = states['hot_cover_pending']['nativePlayer']
        assert pending['state'] == 'loading' and pending['pendingPaused'] is True
        assert pending['playId'] == cover['playId']
        assert cover['resourceKind'] == 'catalog'
        assert details['state'] == resumed['state'] == 'playing' and paused['state'] == 'paused'
        assert len({entry['playId'] for entry in (cover, details, paused, resumed)}) == 1
        assert len({entry['queueRevision'] for entry in (cover, details, paused, resumed)}) == 1
        if (first / 'locale.json').is_file():
            actual = json.loads((first / 'locale.json').read_text(encoding='utf-8'))
            assert actual['numeric'] == 'C'
            if os.environ.get('LC_ALL') == 'en_US.UTF-8':
                assert actual['messages'] == actual['characters'] == 'en_US.UTF-8'
        voice = states['hot_details_voice']['nativePlayer']
        assert voice['state'] == 'playing' and voice['resourceKind'] == 'voice'
        assert voice['track']['id'] == '1' and voice['track']['voiceId'] == '401'
        assert voice['queueLen'] == 2 and voice['position'] > .2 and voice['playId'] != cover['playId']
        calls = complete['hotPlaybackCalls']
        assert not any(call['path'] in ('/api/v6/dj/program/byradio', '/api/djradio/v3/get') for call in calls)
        details = [str(call['args']['id']) for call in calls if call['path'] == '/api/dj/program/detail']
        assert details == ['401', '401'], details  # Native queue metadata, then resolver revalidation.
        assert [str(call['args']['voiceId']) for call in calls if call['path'] == '/api/dj/play/url/check'] == ['401']
        urls = [call['args'] for call in calls if call['path'] == '/api/song/enhance/player/url/v1']
        assert len(urls) == 3 and all(call['trialMode'] == -1 for call in urls), urls
        report = dict(ok=True, actualWebView=True, nativeNullPlaybackVerified=True,
                      stageCount=8, screenshots=len(list(first.glob('ux_*.png'))),
                      realMusicRequests=0, realAccountWrites=0)
        (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
        print(json.dumps(report))
        return
    if args.hot_card:
        first = root / 'first'
        complete = json.loads((first / 'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['hotCardVerified'] and complete['stageCount'] == 5
        reads = signed_result_reads(complete, complete['syntheticV3Calls'], 7)
        assert complete['realAccountWrites'] == 0
        assert len(list(first.glob('ux_*.png'))) == (5 if args.screenshots else 0)
        print(json.dumps(dict(ok=True, actualWebView=True, hotCardVerified=True,
                              stageCount=5, nativeAssignmentAndResultReads=reads, realAccountWrites=0)))
        return
    if args.carousel:
        first = root / 'first'
        complete = json.loads((first / 'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['searchCarouselVerified'] and complete['searchBestThirdVerified'] and complete['stageCount'] == 7
        before, resized, category_back = (
            json.loads((first / name).read_text(encoding='utf-8')) for name in (
                'ux_04_carousel_detail_back_reset.json', 'ux_05_carousel_same_columns_reset.json',
                'ux_07_carousel_category_back.json'))
        old, new = resized['beforeResize'], resized['afterResize']
        assert old['columns'] == new['columns'] == 6 and old['start'] == 4 and new['start'] == 0
        assert old['reads'] == new['reads'] == ['all'] and old['player'] == new['player']
        assert before['start'] == 0 and before['rendererReads'] == ['all']
        assert before['syntheticSearchCalls'] == resized['syntheticSearchCalls'] == 1
        assert before['searchCarouselCalls'] == resized['searchCarouselCalls']
        assert before['signedSearch']['calls'] == resized['signedSearch']['calls']
        assert category_back['start'] == 0 and category_back['rendererReads'] == ['all', 'playlists']
        assert category_back['syntheticSearchCalls'] == 2
        calls = complete['searchCarouselCalls']
        assert [call['path'] for call in calls] == ['/api/search/pc/complex/page'], calls
        assert calls[0]['args'] == {
            'keyword': '合成轮播', 'scene': 'normal', 'needCorrect': True,
            'channel': 'typing', 'cursor': '{"page":0,"ignoreBlocks":[],"traceId":""}',
            'bizQueryInfo': '',
        }, calls
        reads = signed_result_reads(complete, len(calls), 2)
        assert complete['realAccountWrites'] == 0
        assert len(list(first.glob('ux_*.png'))) == (7 if args.screenshots else 0)
        print(json.dumps(dict(ok=True, actualWebView=True, searchCarouselVerified=True,
                              searchBestThirdVerified=True, mountedResizeReset=True,
                              detailBackReset=True, categoryBackReset=True, stageCount=7,
                              nativeAssignmentAndResultReads=reads, realAccountWrites=0)))
        return
    if args.filtered_pagination:
        first = root / 'first'
        complete = json.loads((first / 'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['searchFilteredPaginationVerified'] and complete['stageCount'] == 6
        assert complete['searchCategoryEmptyVerified']
        assert complete['filteredMvReads'] == [[0, 13], [13, 13], [26, 13]]
        assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
        assert len(list(first.glob('ux_*.png'))) == (6 if args.screenshots else 0)
        print(json.dumps(dict(complete, actualWebView=True)))
        return
    if args.suggestion_focus:
        first = root / 'first'
        complete = json.loads((first / 'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['searchSuggestionFocusVerified'] and complete['stageCount'] == 4
        assert complete['searchSuggestionOwnerVerified']
        assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
        assert len(list(first.glob('ux_*.png'))) == (4 if args.screenshots else 0)
        print(json.dumps(dict(ok=True, actualWebView=True, searchSuggestionFocusVerified=True,
                              searchSuggestionOwnerVerified=True, stageCount=4)))
        return
    if args.album_artist:
        first = root / 'first'
        complete = json.loads((first / 'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['searchAlbumArtistVerified']
        assert complete['searchAlbumLinksVerified'] and complete['searchAlbumCollectVerified']
        assert complete['searchAlbumMetadataVerified']
        assert complete['searchAlbumWaterfallVerified'] and complete['searchAlbumTableVerified']
        assert complete['stageCount'] == 11
        assert complete['albumReads'] == [[0, 10], [10, 10]]
        assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
        assert len(list(first.glob('ux_*.png'))) == (11 if args.screenshots else 0)
        print(json.dumps(dict(ok=True, actualWebView=True, searchAlbumArtistVerified=True,
                              searchAlbumLinksVerified=True, searchAlbumCollectVerified=True,
                              searchAlbumMetadataVerified=True, searchAlbumWaterfallVerified=True,
                              searchAlbumTableVerified=True, stageCount=11)))
        return
    if args.collection_next:
        first = root / 'first'
        complete = json.loads((first / 'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['searchCollectionNextVerified']
        assert complete['searchPlaylistDownloadVerified'] and complete['searchPlaylistPlayabilityVerified']
        assert complete['stageCount'] == 6
        assert complete['playlistReads'] == [['3', 0, 100], ['3', 0, 100]]
        assert complete['downloadCommands'] == 0
        assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
        assert len(list(first.glob('ux_*.png'))) == (6 if args.screenshots else 0)
        print(json.dumps(dict(ok=True, actualWebView=True, searchCollectionNextVerified=True,
                              searchPlaylistDownloadVerified=True, searchPlaylistPlayabilityVerified=True,
                              stageCount=6)))
        return
    if args.playlist_creator:
        first = root / 'first'
        complete = json.loads((first / 'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['searchPlaylistCreatorVerified']
        assert complete['searchPlaylistLinksVerified'] and complete['searchPlaylistSubscribeVerified']
        assert complete['searchPlaylistWaterfallVerified'] and complete['searchPlaylistTableVerified']
        assert complete['searchPlaylistResizeVerified'] and complete['stageCount'] == 11
        assert complete['searchPlaylistTrustedPointer'] == args.trusted_playlist_pointer
        assert complete['playlistWaterfallReads'] == [[0, 10], [18, 10], [29, 10]]
        calls = complete['searchProfileCalls']
        profile_reads = [{'all': True, 'userId': '501'}, {'uid': '501', 'offset': 0, 'limit': 1000}]
        assert calls in (profile_reads, profile_reads * 2)
        assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
        assert len(list(first.glob('ux_*.png'))) == (11 if args.screenshots else 0)
        widths = complete['playlistWidths']
        assert len(widths) == 5 and widths[1] == 200 and widths[2] > 79
        reopen = root / 'reopen'
        script = root / 'search-results-reopen.js'
        script.write_text(f'window.__cumulusSearchPlaylistReopenWidths = {json.dumps(widths)};\n'
                          + source.read_text(encoding='utf-8'), encoding='utf-8')
        with (root / 'reopen.log').open('x', encoding='utf-8') as log:
            code = launch(args.binary.resolve(), {
                **env, 'CUMULUS_UX_SCRIPT': str(script), 'CUMULUS_PROOF_REOPEN': '1',
            }, log, reopen, args.screenshots)
        failed = reopen / 'failed.json'
        assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
        assert code == 0, f'reopened probe exited {code}'
        persisted = json.loads((reopen / 'complete.json').read_text(encoding='utf-8'))
        assert persisted['ok'] and persisted['searchPlaylistResizeReopened'] and persisted['stageCount'] == 1
        assert persisted['realMusicRequests'] == persisted['realAccountWrites'] == 0
        assert len(list(reopen.glob('ux_*.png'))) == (1 if args.screenshots else 0)
        print(json.dumps(dict(ok=True, actualWebView=True, searchPlaylistCreatorVerified=True,
                              searchPlaylistLinksVerified=True, searchPlaylistSubscribeVerified=True,
                              searchPlaylistWaterfallVerified=True, searchPlaylistTableVerified=True,
                              searchPlaylistResizeVerified=True, searchPlaylistResizeReopened=True,
                              trustedPointer=args.trusted_playlist_pointer, stageCount=12)))
        return
    if args.mv_fallback:
        first = root / 'first'
        complete = json.loads((first / 'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['searchMvFallbackVerified'] and complete['stageCount'] == 3
        assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
        before = json.loads((first / 'ux_01_search_mv_fallback_badge.json').read_text(encoding='utf-8'))
        assert not before['searchMvCalls']
        calls = complete['searchMvCalls']
        detail = {'path': '/api/mlog/detail/v1', 'args': {'id': '601', 'type': '2'}}
        options = {'path': '/api/mlog/detail/v1', 'args': {'id': '601', 'type': '2', 'videoType': 'mp4'}}
        authorize = {'path': '/api/song/enhance/play/mv/url', 'args': {'id': '601', 'r': 480}}
        # The MV detail panel and native video resolver read independently.
        assert len(calls) == 3
        assert calls.count(detail) == calls.count(options) == calls.count(authorize) == 1
        assert calls.index(detail) < calls.index(authorize)
        assert len(list(first.glob('ux_*.png'))) == (3 if args.screenshots else 0)
        print(json.dumps(dict(ok=True, actualWebView=True, actualNativeVideo=True,
                              searchMvFallbackVerified=True, stageCount=3)))
        return
    if args.mv_play:
        first = root / 'first'
        complete = json.loads((first / 'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['searchMvVerified'] and complete['searchMvArtistVerified']
        assert complete['stageCount'] == 7
        for prefix, name in enumerate(('artist_all', 'artist_mvs', 'play_target'), start=1):
            assert not json.loads((first / f'ux_{prefix:02}_search_mv_{name}.json').read_text(encoding='utf-8'))['searchMvCalls']
        played = json.loads((first / 'ux_06_search_mv_native_frame.json').read_text(encoding='utf-8'))['searchMvCalls']
        category = json.loads((first / 'ux_04_search_mv_category_native_frame.json').read_text(encoding='utf-8'))['searchMvCalls']
        assert played[:len(category)] == category
        details = [call for call in played if call['path'] == '/api/mlog/detail/v1']
        assert details and all(call['args'] == {'id': '601', 'type': '2'} for call in details)
        expected = {'path': '/api/song/enhance/play/mv/url', 'args': {'id': '601', 'r': 480}}
        assert [call for call in category if call['path'] == expected['path']] == [expected]
        assert [call for call in played if call not in details] == [expected, expected]
        assert complete['searchMvCalls'] == played
        ids = ['1', '2', '3', '4', '5', '6']
        privilege = {'path': '/api/song/enhance/privilege',
                     'args': {'ids': json.dumps(ids, separators=(',', ':'))}, 'cached': False,
                     'rows': [{'id': id_, 'st': -200 if id_ == '2' else 0,
                               'pl': 0 if id_ == '2' else 320000} for id_ in ids]}
        assert complete['searchMvPrivilegeCalls'] == [privilege] * 3, complete['searchMvPrivilegeCalls']
        typed = {'sameReceipt': True, 'sameEpoch': True, 'noObservation': True, 'ids': ids,
                 'rows': [{'id': id_, 'mvId': '601' if id_ == '1' else None, 'replacement': False}
                          for id_ in ids]}
        assert complete['privilegeRefreshes'] == [typed] * 3, complete['privilegeRefreshes']
        assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
        assert len(list(first.glob('ux_*.png'))) == (7 if args.screenshots else 0)
        print(json.dumps(dict(ok=True, actualWebView=True, actualNativeVideo=True,
                              searchMvArtistVerified=True, mvPrivilegeRefreshes=3, stageCount=7)))
        return
    if args.podcast_follow:
        first = root / 'first'
        complete = json.loads((first / 'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['podcastSearchFollowVerified']
        assert complete['stageCount'] == 5 and complete['snapshots'] == 7
        calls = complete['podcastFollowCalls']
        read = ['/api/djradio/get/subed', {'targetUserId': '2', 'limit': 1000}]
        detail = ['/api/djradio/v3/get', {'id': '301'}]
        assert calls == [
            read, detail, ['/api/djradio/sub', {'id': '301'}], detail, detail, read,
            read, read, read, detail, ['/api/djradio/unsub', {'id': '301'}], read,
            detail, detail, read,
        ]
        assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
        assert len(list(first.glob('ux_*.png'))) == (5 if args.screenshots else 0)
        print(json.dumps(dict(ok=True, actualWebView=True, podcastSearchFollowVerified=True,
                              membershipReads=7, syntheticWrites=2, stageCount=5)))
        return
    if args.voice_next:
        first = root / 'first'
        complete = json.loads((first / 'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['voiceNextVerified'] and complete['stageCount'] == 6
        assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
        states = {p.stem.split('_', 2)[2]: json.loads(p.read_text(encoding='utf-8'))
                  for p in first.glob('ux_*.json')}
        detail, permit = '/api/dj/program/detail', '/api/dj/play/url/check'

        def voice_calls(state):
            calls = state['voiceNextCalls']
            # Whole-podcast reads and authorization for an unselected episode
            # must never occur, regardless of when decoder prefetch settles.
            assert all(
                (path == detail and payload in ({'id': '401'}, {'id': '402'}, {'id': '403'})) or
                (path == permit and payload in ({'voiceId': '401'}, {'voiceId': '402'}))
                for path, payload in calls
            ), calls
            return [(path, payload.get('id') or payload.get('voiceId')) for path, payload in calls]

        assert not voice_calls(states['voice_next_comprehensive_menu'])
        playing = states['voice_next_preserves_playing']
        paused = states['voice_next_preserves_pause']
        playing_calls = voice_calls(playing)
        paused_calls = voice_calls(paused)
        failed_calls = voice_calls(states['voice_next_read_failure'])
        advanced_calls = voice_calls(states['voice_next_native_episode'])
        calls = voice_calls(complete)
        assert calls[:len(advanced_calls)] == advanced_calls
        assert playing_calls and playing_calls[0] == (detail, '401')
        assert all(episode == '401' for _, episode in playing_calls)
        assert (detail, '402') in paused_calls and all(episode != '403' for _, episode in paused_calls)
        assert failed_calls.count((detail, '403')) == calls.count((detail, '403')) == 1
        assert (permit, '402') in advanced_calls
        assert (detail, '401') in calls[len(advanced_calls):] and (permit, '401') in calls
        assert all(1 <= calls.count((permit, episode)) <= 2 for episode in ('401', '402')), calls
        assert playing['order'] == ['1', '401', '2', '3']
        assert paused['order'] == ['1', '402', '401', '2', '3']
        assert playing['playId'] == paused['playId']
        assert states['voice_next_native_episode']['position'] > .15
        assert states['voice_next_empty_starts_one']['order'] == ['401']
        assert states['voice_next_empty_starts_one']['position'] > .15
        assert len(list(first.glob('ux_*.png'))) == (6 if args.screenshots else 0)
        report = dict(ok=True, actualWebView=True, voiceNextVerified=True, stageCount=6,
                      nativeNullPlaybackVerified=True, screenshots=len(list(first.glob('ux_*.png'))),
                      realMusicRequests=0, realAccountWrites=0)
        (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
        print(json.dumps(report))
        return
    if args.voice_queue:
        complete = json.loads((root / 'first/complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['voiceQueueVerified'] and complete['audioWaterfallVerified']
        assert complete['voiceTableVerified'] and complete['podcastTableVerified'] and complete['stageCount'] == 11
        assert complete['audioReads'] == [['voices', 0, 10], ['voices', 10, 10],
                                          ['podcasts', 0, 10], ['podcasts', 10, 10],
                                          ['podcasts', 10, 10]]
        assert complete['emptyAudioReads'] == [['合成播客空值', 0, 10], ['合成播客空列表', 0, 10]]
        first = root / 'first'
        played = json.loads((first / 'ux_01_search_episode_joins_ordinary_queue.json').read_text(encoding='utf-8'))
        paused = json.loads((first / 'ux_02_search_episode_pauses_in_place.json').read_text(encoding='utf-8'))
        assert set(played['voiceQueueCalls']) == {'/api/dj/program/detail'}
        assert paused['voiceQueueCalls'] == played['voiceQueueCalls']
        assert '/api/v6/dj/program/byradio' in complete['voiceQueueCalls']
        assert complete['searchProfileCalls'] == [
            {'all': True, 'userId': '501'}, {'uid': '501', 'offset': 0, 'limit': 1000},
        ]
        assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
        assert len(list(first.glob('ux_*.png'))) == (11 if args.screenshots else 0)
        print(json.dumps(dict(ok=True, actualWebView=True, voiceQueueVerified=True,
                              voiceTableVerified=True, podcastTableVerified=True,
                              audioWaterfallVerified=True, stageCount=11)))
        return
    if args.song_waterfall:
        complete = json.loads((root / 'first/complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['songWaterfallVerified'] and complete['stageCount'] == 5
        assert complete['songReads'] == [[0, 10], [10, 10], [0, 30], [10, 10], [20, 10]]
        assert complete['songTitleHints'] == [
            dict(alias='前缀合成连续单曲别名', lyric=f'🎵合成连续单曲歌词 {song_id}',
                 aliasMark='合成连续单曲', lyricMark='合成连续单曲')
            for song_id in [101, 130]
        ]
        assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
        assert len(list((root / 'first').glob('ux_*.png'))) == (5 if args.screenshots else 0)
        print(json.dumps(dict(ok=True, actualWebView=True, songWaterfallVerified=True,
                              songTitleHintsVerified=True, stageCount=5,
                              songReads=complete['songReads'])))
        return
    if args.artist_gallery:
        first = root / 'first'
        complete = json.loads((first / 'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['artistGalleryVerified'] and complete['stageCount'] == 5
        assert complete['artistGalleryBatchesVerified']
        assert complete['artistGalleryCalls'] == [
            dict(area=area, type=kind, initial=initial, offset=offset, limit=20)
            for area, kind, initial, offset in [
                (-1, -1, -1, 0), (7, -1, -1, 0), (7, 2, -1, 0), (7, 2, 65, 0),
                (7, 2, 65, 20), (7, 2, 65, 20), (7, 2, 0, 0),
                *((7, 2, 66, offset) for offset in range(0, 200, 20)),
                (7, 2, 66, 200), (7, 2, 66, 200), (7, 2, 66, 0),
            ]
        ]
        assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
        assert len(list(first.glob('ux_*.png'))) == (5 if args.screenshots else 0)
        print(json.dumps(dict(ok=True, actualWebView=True, artistGalleryVerified=True,
                              artistGalleryBatchesVerified=True, stageCount=5,
                              nativeArtistGalleryReads=20, realAccountWrites=0)))
        return
    if args.voice_download_submit:
        complete = json.loads((root / 'first/complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['voiceDownloadSubmitVerified'] and complete['stageCount'] == 6
        assert complete['voiceDownloadService'] == {'rights': 4, 'options': 3, 'urls': 1, 'gets': 1}
        assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
        assert len(list((root / 'first').glob('ux_*.png'))) == (6 if args.screenshots else 0)
        with closing(sqlite3.connect((root / 'state/core.sqlite3').as_uri() + '?mode=ro', uri=True)) as db:
            task, = (json.loads(value) for value, in db.execute(
                "SELECT value FROM state WHERE owner='2' AND kind='download_tasks_v1'"))
        assert task['origin'] == {'kind': 'voice', 'voice_id': '401'} and task['song_id'] == '1'
        assert task['status'] == 'complete' and task['authorization_count'] == 1
        assert task['options']['quality'] == 'higher' and task['options']['container'] == 'audio'
        assert not task['options']['lyrics']
        verify_voice_download_file(task['target'], root / 'voice-media')
        summary = dict(ok=True, actualWebView=True, voiceId='401', mediaId='1', stageCount=6,
                       nativePublished=True, privateSyntheticMedia=True, requestCounts=complete['voiceDownloadService'])
        (root / 'report.json').write_text(json.dumps(summary, indent=2), encoding='utf-8')
        print(json.dumps(summary))
        return
    if args.voice_direct or args.card_matches or args.correction_original or args.people_categories or args.search_next:
        complete = json.loads((root / 'first/complete.json').read_text(encoding='utf-8'))
        flag = ('searchNextVerified' if args.search_next else
                'peopleCategoriesVerified' if args.people_categories else
                'originalCorrectionVerified' if args.correction_original else
                'cardTitleMatchesVerified' if args.card_matches else 'voiceDirectVerified')
        stages = 8 if args.voice_direct else 4 if args.people_categories or args.correction_original or args.search_next else 3 if args.card_matches else 2
        assert complete['ok'] and complete[flag] and complete['stageCount'] == stages
        if args.search_next:
            assert complete['searchNextNoopVerified'] and complete['searchMenuPlayVerified']
        if args.people_categories:
            assert complete['searchArtistMenuVerified']
        if args.card_matches:
            assert complete['mvArtistMatchesVerified'] and complete['playlistCategoryRecommendationVerified']
        if args.correction_original:
            assert complete['correctionPartsVerified']
            assert complete['categoryCorrectionPartsVerified'] and complete['staleCorrectionPartsSuppressed']
            path = '/api/search/pc/complex/page'
            originals = [payload for name, payload in complete['correctionResultCalls']
                         if name == path and payload['needCorrect'] is False]
            assert [payload['channel'] for payload in originals] == ['synthetic_qc_reminder', 'suggest']
            assert all(payload['keyword'] == '合成错词' and payload['bizQueryInfo'] == '' for payload in originals)
            signed = [call for call in complete['signedSearch']['calls'] if call['path'] == path]
            assert len(signed) == 1 and signed[0]['args'] == dict(originals[0], needCorrect='false')
            assert signed[0]['signedPcContextVerified'] and signed[0]['sameAccountCredential']
        if args.voice_direct:
            assert complete['preparedTicketFenceVerified'] and complete['searchSongNamespaceVerified']
            assert complete['voiceDownloadVerified'] and complete['voicePendingTransportVerified']
            stages_by_label = {
                name: json.loads(next((root / 'first').glob(f'ux_*_{name}.json')).read_text(encoding='utf-8'))
                for name in ('voice_row_paused_while_loading', 'voice_cover_resumed_while_loading',
                             'voice_first_decode_paused')
            }
            row, cover, decoded = (stage['nativePlayer'] for stage in stages_by_label.values())
            assert row['state'] == cover['state'] == 'loading' and decoded['state'] == 'paused'
            assert row['pendingPaused'] is True and cover['pendingPaused'] is False
            assert all(row[key] == cover[key] == decoded[key]
                       for key in ('epoch', 'playId', 'entryId', 'queueRevision'))
            assert all(stage['sourceAudio']['loads'] == 1 for stage in stages_by_label.values())
        assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
        assert len(list((root / 'first').glob('ux_*.png'))) == (stages if args.screenshots else 0)
        report = dict(complete, actualWebView=True)
        if args.voice_direct:
            (root / 'report.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
        print(json.dumps(report))
        return
    verify(root, args.screenshots, args.v3)

def verify(root, capture, v3=False):
    complete = json.loads((root / 'first/complete.json').read_text(encoding='utf-8'))
    if v3:
        assert all(complete[key] for key in (
            'ok', 'v3Verified', 'v3TrackHintsVerified', 'v3ExplicitVerified',
            'v3ReasonRoutesVerified', 'v3ReasonPlaybackVerified', 'v3ReasonBatchVerified',
            'v3OriginalRecordingVerified', 'v3ReasonCurrentRowVerified',
        ))
        assert complete['stageCount'] == 9
        signed_result_reads(complete, complete['syntheticV3Calls'], 3)
        assert complete['syntheticSearchCalls'] >= 4
        assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
        assert len(list((root / 'first').glob('ux_*.json'))) == 9
        assert len(list((root / 'first').glob('ux_*.png'))) == (9 if capture else 0)
        path, = (root / 'first').glob('ux_*_v3_city_row_paused.json')
        player = json.loads(path.read_text(encoding='utf-8'))['nativePlayer']
        assert player['state'] == 'paused' and player['track']['id'] == '19' and player['position'] > .25
        (root / 'audit.json').write_text(json.dumps(dict(complete, actualWebView=True), indent=2), encoding='utf-8')
        print(json.dumps(complete))
        return
    assert complete['ok'] and complete['responseTimingInjected'] and complete['shortPageOffsetsVerified']
    assert complete['searchResultPlaybackVerified'] and complete['searchMatchHighlightVerified']
    assert complete['searchCurrentRowsVerified']
    assert complete['searchCategoryWithoutFramesVerified'] and complete['searchServiceEmptyVerified']
    assert complete['searchKeywordRaceVerified']
    before_profile = json.loads((root / 'first/ux_01_all_1120_light.json').read_text(encoding='utf-8'))
    assert before_profile['searchProfileCalls'] == []
    assert complete['searchProfileWithoutFramesVerified']
    assert complete['searchProfileCalls'] == [
        {'all': True, 'userId': '501'},
        {'uid': '501', 'offset': 0, 'limit': 1000},
    ]
    assert complete['stageCount'] == 71 and complete['syntheticSearchCalls'] > 40
    for suffix, song in (('_current_lyric_paused', '3'), ('_current_song_paused', '2')):
        path, = [p for p in (root / 'first').glob('ux_*.json') if p.stem.endswith(suffix)]
        player = json.loads(path.read_text(encoding='utf-8'))['nativePlayer']
        assert player['state'] == 'paused' and player['track']['id'] == song and player['position'] > .15
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    assert len(list((root / 'first').glob('ux_*.json'))) == complete['stageCount']
    if capture:
        assert len(list((root / 'first').glob('ux_*.png'))) == complete['stageCount']
    result = dict(complete, actualWebView=True,
                  screenshots=complete['stageCount'] if capture else 0,
                  physicalDisplayVerified=False, windowsVisualVerified=False)
    (root / 'audit.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({k: result[k] for k in ['ok', 'stageCount', 'screenshots', 'syntheticSearchCalls']}))


if __name__ == '__main__':
    main()
