"""Independent snapshot checks for catalog route history and live native queues."""


def verify(stages):
    assert len(stages) == 52
    for suffix in ['1120_light', '760_light', '760_dark', '1120_dark']:
        def state(name):
            return next(row for row in stages if row['stage'].endswith('_' + name + '_' + suffix))
        original = state('albums40')
        assert original['albumTotal'] == 40 and original['listScroll'] > 0
        for name in ['artist_return', 'history_albums']:
            returned = state(name)
            assert returned['artistTab'] == 'albums' and returned['albumTotal'] == 40
            assert abs(returned['listScroll'] - original['listScroll']) < 1
            assert returned['artistAlbumReads'] == original['artistAlbumReads']
            assert returned['paintSamples'], 'restored album grid is outside the visible viewport'
        assert state('artist_return')['focusedCard'] == 'album:46'
        assert state('artist_return')['updating'], 'held revalidation was never visible'
        assert state('artist_return')['followDisabled'], 'stale membership controls were writable'
        assert state('album_details')['albumTab'] == 'details'
        assert state('intro')['artistTab'] == 'intro'
        assert state('history_songs')['artistTab'] == 'songs'
        assert state('all55')['listScroll'] > 0
        assert abs(state('all55')['listScroll'] - state('all_return')['listScroll']) < 1
        assert state('all55')['artistSongReads'] == state('all_return')['artistSongReads']
        before, returned = state('comments_before'), state('comments_back')
        assert before['albumTab'] == returned['albumTab'] == 'comments'
        assert before['commentIds'] == returned['commentIds'] == [str(n) for n in range(21, 26)]
        assert before['outerScroll'] > 0 and abs(before['outerScroll'] - returned['outerScroll']) < 1
        assert before['draft'] == returned['draft'] == state('comments_visible')['draft'] == '导航草稿 ' + suffix
        assert before['locator'] == returned['locator'] == '101'
        assert returned['commentReads'] == before['commentReads'] + 1
        playing = [row for row in stages if row['stage'].endswith('_' + suffix) and row.get('playing')]
        assert len(playing) == 10
        assert len({row['nativePlayer']['playId'] for row in playing}) == 1
        for row in playing:
            player = row['nativePlayer']
            assert player['playId'] and player['state'] == 'playing' and player['position'] > .2
            assert player['queueLen'] == 50 and not player['error']
            assert player['origin'] == {'kind': 'artist', 'id': '12'}
            assert [item['track']['id'] for item in row['nativeQueue']['tracks']] == [str(n) for n in range(1, 51)]
    return {'ok': True, 'actualWebView': True, 'states': 52, 'nativePlayingStates': 40,
            'realMusicRequests': 0, 'realAccountWrites': 0, 'physicalAudioOutputVerified': False,
            'routeReturnVerified': True}
