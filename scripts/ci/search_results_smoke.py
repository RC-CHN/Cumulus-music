"""Search result categories and interactions via production IPC and an isolated in-memory backend."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import json
import os
from pathlib import Path
from threading import Event
import time
from motion_smoke import launch


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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--v3', action='store_true')
    mode.add_argument('--carousel', action='store_true')
    mode.add_argument('--hot-card', action='store_true')
    mode.add_argument('--voice-direct', action='store_true')
    mode.add_argument('--card-matches', action='store_true')
    mode.add_argument('--correction-original', action='store_true')
    mode.add_argument('--artist-gallery', action='store_true')
    mode.add_argument('--people-categories', action='store_true')
    mode.add_argument('--search-next', action='store_true')
    mode.add_argument('--song-waterfall', action='store_true')
    mode.add_argument('--voice-queue', action='store_true')
    mode.add_argument('--podcast-follow', action='store_true')
    mode.add_argument('--mv-play', action='store_true')
    mode.add_argument('--playlist-creator', action='store_true')
    mode.add_argument('--collection-next', action='store_true')
    mode.add_argument('--album-artist', action='store_true')
    mode.add_argument('--suggestion-focus', action='store_true')
    mode.add_argument('--filtered-pagination', action='store_true')
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--trusted-playlist-pointer', action='store_true',
                        help='also drag a resize handle using XTest under isolated Xvfb')
    args = parser.parse_args()
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
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/search_results_ui_probe.js'
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
    if args.voice_queue:
        prefix = 'window.__cumulusSearchVoiceQueueProof = true;\n'
    if args.podcast_follow:
        prefix = 'window.__cumulusSearchPodcastFollowProof = true;\n'
    if args.mv_play:
        prefix = 'window.__cumulusSearchMvProof = true;\n'
        env['CUMULUS_SEARCH_MV_PROOF'] = '1'
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
    if args.artist_gallery:
        prefix = 'window.__cumulusArtistGalleryProof = true;\n'
    if args.card_matches:
        prefix = 'window.__cumulusSearchCardMatchesProof = true;\n'
    if args.voice_direct:
        prefix = 'window.__cumulusSearchVoiceDirectProof = true;\n'
    if args.v3:
        env['CUMULUS_SEARCH_V3_PROOF'] = '1'
        prefix = 'window.__cumulusSearchV3Proof = true;\n'
    if args.carousel:
        env['CUMULUS_SEARCH_CAROUSEL_PROOF'] = '1'
        prefix = 'window.__cumulusSearchCarouselProof = true;\n'
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
    if args.hot_card:
        first = root / 'first'
        complete = json.loads((first / 'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['hotCardVerified'] and complete['stageCount'] == 5
        assert complete['syntheticV3Calls'] == 7 and complete['realAccountWrites'] == 0
        assert len(list(first.glob('ux_*.png'))) == (5 if args.screenshots else 0)
        print(json.dumps(dict(ok=True, actualWebView=True, hotCardVerified=True,
                              stageCount=5, v3Reads=7, realAccountWrites=0)))
        return
    if args.carousel:
        first = root / 'first'
        complete = json.loads((first / 'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['searchCarouselVerified'] and complete['searchBestThirdVerified'] and complete['stageCount'] == 6
        calls = complete['searchCarouselCalls']
        assert [call['path'] for call in calls] == ['/api/rtrs/abt/front/expinfo/list', '/api/search/pc/complex/page']
        assert calls[0]['args']['expNames'] == 'PH-PC-searchHot,PH-PC-searchMore2,PH-PC-searchPageEfficiency'
        assert complete['realAccountWrites'] == 0
        assert len(list(first.glob('ux_*.png'))) == (6 if args.screenshots else 0)
        print(json.dumps(dict(ok=True, actualWebView=True, searchCarouselVerified=True,
                              searchBestThirdVerified=True, stageCount=6,
                              nativeAssignmentAndResultReads=2, realAccountWrites=0)))
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
        assert complete['ok'] and complete['searchSuggestionFocusVerified'] and complete['stageCount'] == 2
        assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
        assert len(list(first.glob('ux_*.png'))) == (2 if args.screenshots else 0)
        print(json.dumps(dict(ok=True, actualWebView=True, searchSuggestionFocusVerified=True, stageCount=2)))
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
        assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
        assert len(list(first.glob('ux_*.png'))) == (7 if args.screenshots else 0)
        print(json.dumps(dict(ok=True, actualWebView=True, actualNativeVideo=True, searchMvArtistVerified=True, stageCount=7)))
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
    if args.voice_queue:
        complete = json.loads((root / 'first/complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['voiceQueueVerified'] and complete['audioWaterfallVerified']
        assert complete['voiceTableVerified'] and complete['podcastTableVerified'] and complete['stageCount'] == 8
        assert complete['audioReads'] == [['voices', 0, 10], ['voices', 10, 10],
                                          ['podcasts', 0, 10], ['podcasts', 10, 10]]
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
        assert len(list((root / 'first').glob('ux_*.png'))) == (8 if args.screenshots else 0)
        print(json.dumps(dict(ok=True, actualWebView=True, voiceQueueVerified=True,
                              voiceTableVerified=True, podcastTableVerified=True,
                              audioWaterfallVerified=True, stageCount=8)))
        return
    if args.song_waterfall:
        complete = json.loads((root / 'first/complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['songWaterfallVerified'] and complete['stageCount'] == 5
        assert complete['songReads'] == [[0, 10], [10, 10], [0, 30], [10, 10], [20, 10]]
        assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
        assert len(list((root / 'first').glob('ux_*.png'))) == (5 if args.screenshots else 0)
        print(json.dumps(dict(ok=True, actualWebView=True, songWaterfallVerified=True,
                              stageCount=5, songReads=complete['songReads'])))
        return
    if args.artist_gallery:
        first = root / 'first'
        complete = json.loads((first / 'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['artistGalleryVerified'] and complete['stageCount'] == 3
        assert complete['artistGalleryCalls'] == [
            dict(area=area, type=kind, initial=initial, offset=offset, limit=20)
            for area, kind, initial, offset in [
                (-1, -1, -1, 0), (7, -1, -1, 0), (7, 2, -1, 0), (7, 2, 65, 0),
                (7, 2, 65, 20), (7, 2, 65, 20), (7, 2, 0, 0),
            ]
        ]
        assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
        assert len(list(first.glob('ux_*.png'))) == (3 if args.screenshots else 0)
        print(json.dumps(dict(ok=True, actualWebView=True, artistGalleryVerified=True,
                              stageCount=3, nativeArtistGalleryReads=7, realAccountWrites=0)))
        return
    if args.voice_direct or args.card_matches or args.correction_original or args.people_categories or args.search_next:
        complete = json.loads((root / 'first/complete.json').read_text(encoding='utf-8'))
        flag = ('searchNextVerified' if args.search_next else
                'peopleCategoriesVerified' if args.people_categories else
                'originalCorrectionVerified' if args.correction_original else
                'cardTitleMatchesVerified' if args.card_matches else 'voiceDirectVerified')
        stages = 5 if args.voice_direct else 4 if args.people_categories else 3 if args.correction_original else 2
        assert complete['ok'] and complete[flag] and complete['stageCount'] == stages
        if args.people_categories:
            assert complete['searchArtistMenuVerified']
        if args.card_matches:
            assert complete['mvArtistMatchesVerified']
        if args.correction_original:
            assert complete['correctionPartsVerified']
            assert complete['categoryCorrectionPartsVerified'] and complete['staleCorrectionPartsSuppressed']
        if args.voice_direct:
            assert complete['preparedTicketFenceVerified'] and complete['searchSongNamespaceVerified']
            assert complete['voiceDownloadVerified']
        assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
        assert len(list((root / 'first').glob('ux_*.png'))) == (stages if args.screenshots else 0)
        print(json.dumps(dict(complete, actualWebView=True)))
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
        assert complete['syntheticV3Calls'] == 3 and complete['syntheticSearchCalls'] >= 4
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
