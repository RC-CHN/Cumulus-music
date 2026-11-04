"""Search result categories and interactions via production IPC and an isolated in-memory backend."""
import argparse
import json
import os
from pathlib import Path
from motion_smoke import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--v3', action='store_true')
    mode.add_argument('--voice-direct', action='store_true')
    mode.add_argument('--card-matches', action='store_true')
    mode.add_argument('--correction-original', action='store_true')
    mode.add_argument('--people-categories', action='store_true')
    mode.add_argument('--search-next', action='store_true')
    mode.add_argument('--song-waterfall', action='store_true')
    mode.add_argument('--voice-queue', action='store_true')
    mode.add_argument('--mv-play', action='store_true')
    mode.add_argument('--playlist-creator', action='store_true')
    mode.add_argument('--collection-next', action='store_true')
    mode.add_argument('--album-artist', action='store_true')
    mode.add_argument('--suggestion-focus', action='store_true')
    mode.add_argument('--filtered-pagination', action='store_true')
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
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
    if args.mv_play:
        prefix = 'window.__cumulusSearchMvProof = true;\n'
        env['CUMULUS_SEARCH_MV_PROOF'] = '1'
    if args.playlist_creator:
        prefix = 'window.__cumulusSearchPlaylistCreatorProof = true;\n'
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
    if args.card_matches:
        prefix = 'window.__cumulusSearchCardMatchesProof = true;\n'
    if args.voice_direct:
        prefix = 'window.__cumulusSearchVoiceDirectProof = true;\n'
    if args.v3:
        env['CUMULUS_SEARCH_V3_PROOF'] = '1'
        prefix = 'window.__cumulusSearchV3Proof = true;\n'
    script.write_text(prefix + source.read_text(encoding='utf-8'), encoding='utf-8')
    env['CUMULUS_UX_SCRIPT'] = str(script)
    env['CUMULUS_SEARCH_USER_PROFILE_PROOF'] = '1'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, root / 'first', args.screenshots)
    failed = root / 'first/failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'probe exited {code}'
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
        assert complete['ok'] and complete['searchAlbumArtistVerified'] and complete['stageCount'] == 2
        assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
        assert len(list(first.glob('ux_*.png'))) == (2 if args.screenshots else 0)
        print(json.dumps(dict(ok=True, actualWebView=True, searchAlbumArtistVerified=True, stageCount=2)))
        return
    if args.collection_next:
        first = root / 'first'
        complete = json.loads((first / 'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['searchCollectionNextVerified']
        assert complete['searchPlaylistDownloadVerified'] and complete['stageCount'] == 4
        assert complete['playlistReads'] == [['3', 0, 100], ['3', 0, 100]]
        assert complete['downloadCommands'] == 0
        assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
        assert len(list(first.glob('ux_*.png'))) == (4 if args.screenshots else 0)
        print(json.dumps(dict(ok=True, actualWebView=True, searchCollectionNextVerified=True,
                              searchPlaylistDownloadVerified=True, stageCount=4)))
        return
    if args.playlist_creator:
        first = root / 'first'
        complete = json.loads((first / 'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['searchPlaylistCreatorVerified'] and complete['stageCount'] == 2
        calls = complete['searchProfileCalls']
        profile_reads = [{'all': True, 'userId': '501'}, {'uid': '501', 'offset': 0, 'limit': 1000}]
        assert calls in (profile_reads, profile_reads * 2)
        assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
        assert len(list(first.glob('ux_*.png'))) == (2 if args.screenshots else 0)
        print(json.dumps(dict(ok=True, actualWebView=True, searchPlaylistCreatorVerified=True, stageCount=2)))
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
    if args.voice_queue:
        complete = json.loads((root / 'first/complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['voiceQueueVerified'] and complete['stageCount'] == 3
        first = root / 'first'
        played = json.loads((first / 'ux_01_search_episode_joins_ordinary_queue.json').read_text(encoding='utf-8'))
        paused = json.loads((first / 'ux_02_search_episode_pauses_in_place.json').read_text(encoding='utf-8'))
        assert set(played['voiceQueueCalls']) == {'/api/dj/program/detail'}
        assert paused['voiceQueueCalls'] == played['voiceQueueCalls']
        assert '/api/v6/dj/program/byradio' in complete['voiceQueueCalls']
        assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
        assert len(list((root / 'first').glob('ux_*.png'))) == (3 if args.screenshots else 0)
        print(json.dumps(dict(ok=True, actualWebView=True, voiceQueueVerified=True, stageCount=3)))
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
    if args.voice_direct or args.card_matches or args.correction_original or args.people_categories or args.search_next:
        complete = json.loads((root / 'first/complete.json').read_text(encoding='utf-8'))
        flag = ('searchNextVerified' if args.search_next else
                'peopleCategoriesVerified' if args.people_categories else
                'originalCorrectionVerified' if args.correction_original else
                'cardTitleMatchesVerified' if args.card_matches else 'voiceDirectVerified')
        stages = 4 if args.people_categories else 3 if args.voice_direct else 2
        assert complete['ok'] and complete[flag] and complete['stageCount'] == stages
        if args.people_categories:
            assert complete['searchArtistMenuVerified']
        if args.correction_original:
            assert complete['correctionPartsVerified']
        if args.voice_direct:
            assert complete['preparedTicketFenceVerified'] and complete['searchSongNamespaceVerified']
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
            'v3OriginalRecordingVerified',
        ))
        assert complete['stageCount'] == 8
        assert complete['syntheticV3Calls'] == 3 and complete['syntheticSearchCalls'] >= 4
        assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
        assert len(list((root / 'first').glob('ux_*.json'))) == 8
        assert len(list((root / 'first').glob('ux_*.png'))) == (8 if capture else 0)
        (root / 'audit.json').write_text(json.dumps(dict(complete, actualWebView=True), indent=2), encoding='utf-8')
        print(json.dumps(complete))
        return
    assert complete['ok'] and complete['responseTimingInjected'] and complete['shortPageOffsetsVerified']
    assert complete['searchResultPlaybackVerified'] and complete['searchMatchHighlightVerified']
    assert complete['searchCategoryWithoutFramesVerified'] and complete['searchServiceEmptyVerified']
    assert complete['searchKeywordRaceVerified']
    before_profile = json.loads((root / 'first/ux_01_all_1120_light.json').read_text(encoding='utf-8'))
    assert before_profile['searchProfileCalls'] == []
    assert complete['searchProfileWithoutFramesVerified']
    assert complete['searchProfileCalls'] == [
        {'all': True, 'userId': '501'},
        {'uid': '501', 'offset': 0, 'limit': 1000},
    ]
    assert complete['stageCount'] == 69 and complete['syntheticSearchCalls'] > 40
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
