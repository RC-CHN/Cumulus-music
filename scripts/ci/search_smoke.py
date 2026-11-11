"""Search discovery/history/suggestion UI via production IPC and an isolated in-memory backend."""
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
    mode.add_argument('--exact-fallback', action='store_true')
    mode.add_argument('--suggestion-matches', action='store_true')
    mode.add_argument('--history-last', action='store_true')
    mode.add_argument('--podcast-top-classic', action='store_true')
    mode.add_argument('--podcast-browse-around', action='store_true')
    mode.add_argument('--default-recovery', action='store_true')
    mode.add_argument('--focus-activation', action='store_true')
    mode.add_argument('--guest-search', action='store_true')
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
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/search_ui_probe.js'
    script = root / 'search.js'
    prefix = 'window.__cumulusSearchExactFallbackProof = true;\n' if args.exact_fallback else ''
    if args.suggestion_matches:
        prefix = 'window.__cumulusSearchSuggestionMatchesProof = true;\n'
    if args.history_last:
        prefix = 'window.__cumulusSearchHistoryLastProof = true;\n'
    if args.podcast_browse_around:
        prefix = 'window.__cumulusPodcastBrowseAroundProof = true;\n'
        env['CUMULUS_SEARCH_PODCAST_BROWSE_AROUND_PROOF'] = '1'
    if args.podcast_top_classic:
        prefix = 'window.__cumulusPodcastTopClassicProof = true;\n'
    if args.focus_activation:
        prefix = 'window.__cumulusSearchFocusActivationProof = true;\n'
    if args.default_recovery:
        prefix = 'window.__cumulusSearchDefaultRecoveryProof = true;\n'
        env['CUMULUS_SEARCH_DEFAULT_RECOVERY_PROOF'] = '1'
    if args.guest_search:
        prefix = 'window.__cumulusGuestSearchProof = true;\n'
        env['CUMULUS_GUEST_SEARCH_PROOF'] = '1'
    script.write_text(prefix + source.read_text(encoding='utf-8'), encoding='utf-8')
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    env['CUMULUS_UX_SCRIPT'] = str(script)
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, root / 'first', args.screenshots)
    failed = root / 'first/failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'probe exited {code}'
    if args.focus_activation:
        verify_focus_activation(root, args.screenshots)
        return
    if args.default_recovery:
        verify_default_recovery(root, args.screenshots)
        return
    if args.guest_search:
        verify_guest_search(root, args.screenshots)
        return
    if args.podcast_browse_around:
        verify_podcast_browse_around(root, args.screenshots)
        return
    if args.exact_fallback or args.suggestion_matches or args.history_last or args.podcast_top_classic:
        complete = json.loads((root / 'first/complete.json').read_text(encoding='utf-8'))
        flag = ('podcastTopClassicVerified' if args.podcast_top_classic else
                'historyLastVerified' if args.history_last else
                'suggestionMatchesVerified' if args.suggestion_matches else 'exactFallbackVerified')
        count = 7 if args.suggestion_matches else 3
        assert complete['ok'] and complete[flag] and complete['stageCount'] == count
        assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
        assert len(list((root / 'first').glob('ux_*.png'))) == (count if args.screenshots else 0)
        if args.suggestion_matches:
            verify_suggestion_matches(root, complete, args.screenshots)
        print(json.dumps(dict(ok=True, actualWebView=True, **{flag: True})))
        return
    verify(root, args.screenshots)


def verify_podcast_browse_around(root, capture):
    directory = root / 'first'
    complete = json.loads((directory / 'complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['podcastBrowseAroundVerified'] and complete['rankDoubleClickVerified']
    assert complete['stageCount'] == 7
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    assert complete['podcastAroundCalls'] == [
        {'programId': episode, 'roundNum': 250} for episode in ('407', '404', '407', '408', '407')
    ]
    assert len(list(directory.glob('ux_*.png'))) == (7 if capture else 0)
    locale = None
    if os.name != 'nt':
        locale = json.loads((directory / 'locale.json').read_text(encoding='utf-8'))
        assert locale['numeric'] == 'C'
        assert all(locale[key] not in ('C', 'POSIX') for key in ('messages', 'characters'))
    report = dict(ok=True, actualWebView=True, nativeAudio=True, captures=7 if capture else 0,
                  rankDoubleClickVerified=True,
                  around=complete['podcastAroundCalls'], locale=locale,
                  realMusicRequests=0, realAccountWrites=0)
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


def verify_guest_search(root, capture):
    directory = root / 'first'

    def stage(name):
        return json.loads((directory / f'{name}.json').read_text(encoding='utf-8'))

    suggestions = stage('ux_02_guest_search_suggestions')
    assert suggestions['guestTypingGesture'] == dict(
        inputFocused=True, inputHovered=True, pointedSuggestion=None)
    comprehensive = stage('ux_03_guest_search_results')
    menu = stage('ux_04_guest_search_songs')
    copied = stage('ux_05_guest_search_share_copy')
    login = stage('ux_06_guest_search_share_login')
    dismissed = stage('ux_07_guest_search_back')
    complete = json.loads((directory / 'complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['guestSearchVerified'] and complete['stageCount'] == 8
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    assert complete['nativePlayer']['state'] == 'idle', complete['nativePlayer']['state']
    assert len(list(directory.glob('ux_*.png'))) == (8 if capture else 0)
    no_share_surface = dict(login=False, qr=False, chooser=False)
    assert menu['guestSongShareMenu'] == dict(
        rowKey='song:1', items=[dict(key='share', label='分享', enabled=True),
                                dict(key='copy', label='复制链接', enabled=True)],
        focusKey='share', **no_share_surface)
    focus_restored = dict(rowKey='song:1', focusRestored=True, **no_share_surface)
    assert dismissed['guestSongShareDismissed'] == focus_restored
    assert dismissed['guestSongShareLoginCancelled'] == focus_restored
    assert copied['guestSongShareCopied'] == dict(
        clipboard='https://music.163.com/song?id=1', nativeClipboard=True, **focus_restored)
    assert login['guestSongShareLogin'] == dict(
        rowKey='song:1', loginQr=True, login=True, qr=False, chooser=False)
    assert all(snapshot['guestShareLoginCalls'] == [] for snapshot in (comprehensive, menu, copied))
    login_paths = complete['guestShareLoginCalls']
    assert login_paths[0] == '/api/login/qrcode/unikey' and len(login_paths) >= 2, login_paths
    assert set(login_paths[1:]) == {'/api/login/qrcode/client/login'}, login_paths
    assert dismissed['guestShareLoginCalls'] == login_paths
    assert login_paths[:len(login['guestShareLoginCalls'])] == login['guestShareLoginCalls']
    ipc = complete['guestLoginIpc']
    assert ipc[0] == 'begin_qr' and ipc[-1] == 'cancel_qr' and set(ipc[1:-1]) == {'poll_qr'}, ipc
    before_calls = comprehensive['guestSearchCalls']
    menu_calls = menu['guestSearchCalls']
    dismissed_calls = dismissed['guestSearchCalls']
    assert menu_calls[:-1] == before_calls and menu_calls[-1]['path'] == '/api/search/song/list/page'
    assert copied['guestSearchCalls'] == login['guestSearchCalls'] == menu_calls
    assert dismissed_calls[:-1] == menu_calls
    assert dismissed_calls[-1]['path'] == '/api/search/pc/complex/page'
    calls = complete['guestSearchCalls']
    assert calls[:len(dismissed_calls)] == dismissed_calls
    paths = [call['path'] for call in calls]
    assert '/api/song/enhance/privilege' not in paths and '/api/login/qrcode/unikey' not in paths
    assert paths[:2] == ['/api/w/nuser/account/get', '/api/register/anonimous'], paths
    assert calls[1]['args'] == {'usernameVerified': True}
    assert all(call['signedPcContextVerified'] for call in calls)
    assert all(call['anonymousCookieVerified'] == (index >= 2) for index, call in enumerate(calls))
    assert paths.count('/api/search/default/keyword/get') == 1, paths
    experiments = [call for call in calls if call['args'] == {'fullBatchVerified': True}]
    assert len(experiments) == 1, paths
    complex_searches = [call['args'] for call in calls if call['path'] == '/api/search/pc/complex/page']
    assert [(row['keyword'], row['channel'], row['bizQueryInfo']) for row in complex_searches] == [
        ('合成游客查询', 'typing', ''),
        ('合成游客查询', 'typing', ''),
        ('合成默认关键词', 'defaultquery', 'synthetic-default-context-native-only'),
    ], complex_searches
    songs = [call['args'] for call in calls if call['path'] == '/api/search/song/list/page']
    assert len(songs) == 1 and songs[0]['keyword'] == '合成游客查询', songs
    suggestions = [call['args'] for call in calls if call['path'] == '/api/search/pc/suggest/keyword/get']
    assert {'keyword': '合成游客查询'} in suggestions, suggestions
    report = dict(ok=True, actualWebView=True, guestSearchVerified=True,
                  guestSongShareMenuVerified=True, guestSongShareFocusRestored=True,
                  guestSongShareMenuRequests=0, independentGuestCopy=True,
                  isolatedGuestLoginQr=True, isolatedGuestLoginCancelled=True,
                  nativeGuestRequests=len(calls), nativeGuestShareLoginCalls=login_paths,
                  stages=8, screenshots=8 if capture else 0,
                  realMusicRequests=0, realAccountWrites=0)
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


def verify_focus_activation(root, capture):
    directory = root / 'first'
    complete = json.loads((directory / 'complete.json').read_text(encoding='utf-8'))
    labels = ('focus_only', 'edit_opens_suggestions', 'refocus_only', 'composing_closed',
              'composition_commit', 'empty_edit_opens_discovery', 'arrow_opens_discovery',
              'click_opens_discovery')
    assert complete['ok'] and complete['searchFocusActivationVerified']
    assert complete['stageCount'] == len(labels) and complete['activationSubmissions'] == 0
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    for index, (label, count) in enumerate(zip(labels, (0, 1, 1, 1, 2, 7, 12, 17)), 1):
        file = directory / f'ux_{index:02d}_{label}.json'
        stage = json.loads(file.read_text(encoding='utf-8'))
        assert stage['expanded'] == (label not in ('focus_only', 'refocus_only', 'composing_closed'))
        assert len(stage['activationReads']) == count and stage['focused'] == '搜索音乐'
        assert file.with_suffix('.png').is_file() == capture
    result = dict(ok=True, actualWebView=True, stages=len(labels), screenshots=len(labels) if capture else 0,
                  focusReads=0, suggestionReads=2, discoveryReads=6, chartDetailReads=9, searchSubmissions=0,
                  realMusicRequests=0, realAccountWrites=0)
    (root / 'report.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(result))


def verify_default_recovery(root, capture):
    directory = root / 'first'
    complete = json.loads((directory / 'complete.json').read_text(encoding='utf-8'))
    labels = ('initial', 'input_focused', 'window_blurred', 'populated', 'submitted', 'typed')
    assert complete['ok'] and complete['defaultRecoveryVerified']
    assert complete['stageCount'] == len(labels) and complete['windowFocusNotificationSynthetic']
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    calls = complete['defaultRecoveryCalls']
    defaults = {'path': '/api/search/default/keyword/get', 'args': {}}
    assert calls[:2] == [defaults, defaults] and len(calls) == 4, calls
    for call, (channel, context) in zip(calls[2:], [
        ('defaultquery', 'synthetic-recovered-second'), ('typing', ''),
    ]):
        assert call['path'] == '/api/search/pc/complex/page'
        args = call['args']
        assert (args['keyword'], args['channel'], args['bizQueryInfo']) == (
            '合成恢复第二词', channel, context), call
    for index, (label, count) in enumerate(zip(labels, [1, 1, 1, 2, 3, 4]), 1):
        file = directory / f'ux_{index:02d}_default_recovery_{label}.json'
        stage = json.loads(file.read_text(encoding='utf-8'))
        assert stage['defaultRecoveryCalls'] == calls[:count], stage['defaultRecoveryCalls']
        assert file.with_suffix('.png').is_file() == capture
    result = dict(ok=True, actualWebView=True, stages=len(labels), screenshots=len(labels) if capture else 0,
                  nativeDefaultReads=2, windowFocusNotificationSynthetic=True,
                  realMusicRequests=0, realAccountWrites=0)
    (root / 'report.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(result))


def verify_suggestion_matches(root, complete, capture):
    for name in ('suggestionPointerVerified', 'suggestionGlyphDirectionVerified',
                 'suggestionFirstSpaceVerified', 'suggestionRecommendationKeyboardVerified'):
        assert complete[name] is True
    stages = [
        'suggestion_literal_matches', 'suggestion_pointer_active', 'suggestion_pointer_selection',
        'suggestion_traditional_query', 'suggestion_simplified_query_not_reversed',
        'suggestion_first_space', 'suggestion_recommendation_selection',
    ]
    assert sorted(path.stem for path in (root / 'first').glob('ux_*.json')) == [
        f'ux_{index:02d}_{name}' for index, name in enumerate(stages, 1)
    ]
    for index, popup, keyword, input_text in [
        (3, 'İ A+B 和 a+b <plain>', '合成精确词', '合成输入栏文案'),
        (7, 'B / C / A / b c', '合成分支推荐', '合成分支推荐'),
    ]:
        stage = json.loads((root / 'first' / f'ux_{index:02d}_{stages[index - 1]}.json').read_text(encoding='utf-8'))
        assert stage['selection'] == {'popup': popup, 'result': keyword, 'input': input_text}, stage['selection']
    assert complete['suggestionHistory'] == ['更改后的原词', '退出悬停', '合成分支推荐', '合成精确词']
    # These requests are recorded by the native synthetic backend, not by the
    # injected JS. The last edit and immediate Enter must not send a suggestion.
    expected = [
        ('suggestion', 'A+B', None),
        ('result', '合成精确词', 'suggest'),
        ('suggestion', '音樂', None),
        ('suggestion', '音乐', None),
        ('suggestion', 'a b c', None),
        ('result', '合成分支推荐', 'rec_query'),
        ('suggestion', '退出悬停', None),
        ('result', '退出悬停', 'typing'),
        ('suggestion', '第一词', None),
        ('result', '更改后的原词', 'typing'),
    ]
    calls = complete['suggestionMatchCalls']
    assert len(calls) == len(expected), calls
    for call, (kind, keyword, channel) in zip(calls, expected):
        if kind == 'suggestion':
            assert call == {'path': '/api/search/pc/suggest/keyword/get',
                            'args': {'keyword': keyword}}, call
        else:
            assert call['path'] == '/api/search/pc/complex/page', call
            assert (call['args']['keyword'], call['args']['channel']) == (keyword, channel), call
    result = dict(complete, actualWebView=True, screenshots=len(stages) if capture else 0,
                  physicalDisplayVerified=False, windowsVisualVerified=False)
    (root / 'audit.json').write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n',
                                    encoding='utf-8')


def verify(root, capture):
    complete = json.loads((root / 'first/complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['responseTimingInjected'] and complete['conditionalChartPlayVerified']
    assert complete['hotqueryChannelVerified'] and complete['searchDisplayLabelVerified']
    assert complete['searchInputChannelVerified'] and complete['searchSelectionChannelsVerified']
    assert complete['cyclicSuggestionsVerified'] and complete['audioRankNavigationVerified']
    assert complete['immediateCompositionSubmitVerified']
    assert complete['audioRankPlaybackVerified']
    assert complete['audiobookMoreVerified'] and complete['podcastMoreVerified']
    assert complete['podcastCategoryVerified'] and complete['podcastCategoryRatingsVerified']
    assert complete['podcastRankingsVerified']
    assert complete['podcastTopUpdatedVerified']
    assert complete['audioRankDetails'] == [301, 301, 302, 304, 305]
    assert complete['podcastAroundCalls'] == [
        {'programId': episode, 'roundNum': 250} for episode in ('407', '404', '407')
    ]
    assert complete['stageCount'] == 76 and complete['syntheticSearchCalls'] > 30
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
