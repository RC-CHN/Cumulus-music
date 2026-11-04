"""Run community surfaces against a closed synthetic desktop API."""
import argparse
import json
import os
from pathlib import Path

from window_manager import launch


def verify_profile_notes(stages, native):
    assert set(stages) == {'first_error', 'first_page', 'detail', 'back_focus', 'next_error',
                           'next_page', 'blocked', 'blocked_back'}
    assert [stages[name]['visibleNotes'] for name in
            ('first_error', 'first_page', 'back_focus', 'next_error', 'next_page', 'blocked')] == [
                0, 12, 12, 12, 13, 0]
    assert stages['back_focus']['openerFocused']
    detail = lambda uid: [f'/api/w/v1/user/detail/{uid}', {'all': True, 'userId': str(uid)}]
    notes = lambda cursor: ['/api/event/get/73',
                            {'offset': 0, 'limit': 20, 'getcounts': True, 'time': cursor}]
    assert native['profileNotesCalls'] == [
        detail(73), detail(73), notes(-1), detail(73), notes(-1),
        ['/api/v1/event/73/31', {}],
        ['/api/v1/resource/comments/A_EV_2_31_73',
         {'limit': 60, 'offset': 0, 'beforeTime': 0, 'compareUserLocation': False,
          'commentId': '0', 'composeConcert': False, 'markReplied': False,
          'forceFlatComment': False, 'showInner': False}],
        detail(73), notes(1700000010321), detail(73), notes(1700000010321), detail(74), detail(74),
    ], native['profileNotesCalls']
    assert native['feed'] == [] and native['details'] == native['releaseReads'] == 0


def verify_profile_podcasts(stages, native, root):
    assert set(stages) == {'guest_error', 'guest_list', 'created_error', 'created_next', 'detail',
                           'back_focus', 'collected_next', 'source_back', 'own_manage', 'blocked'}
    assert [(stages[name]['created'], stages[name]['collected']) for name in
            ('guest_error', 'guest_list', 'created_error', 'created_next', 'back_focus',
             'collected_next', 'own_manage', 'blocked')] == [
                (19, 0), (19, 20), (19, 20), (1, 20), (1, 20), (1, 1), (1, 1), (0, 0)]
    assert stages['back_focus']['focused'] == '821'
    calls = native['profilePodcastCalls']

    def requests(path):
        return [body for target, body in calls if target == path]

    assert requests('/api/djradio/get/create/sub/count') == [
        {'userId': '73'}, {'userId': '2'}, {'userId': '74'}]
    assert requests('/api/djradio/get/byuser/v1') == [
        {'userId': uid, 'offset': offset, 'limit': 20}
        for uid, offset in [('73', 0), ('73', 20), ('73', 20), ('2', 0)]]
    assert requests('/api/djradio/get/subed') == [
        {'targetUserId': uid, 'limit': 1000} for uid in ('73', '73', '2')]
    for uid, count in (('73', 6), ('2', 1), ('74', 3)):
        assert requests(f'/api/w/v1/user/detail/{uid}') == [
            {'all': True, 'userId': uid}] * count, calls
    assert requests('/api/djradio/v3/get') == [{'id': '821'}]
    assert requests('/api/v6/dj/program/byradio') == [
        {'radioId': '821', 'offset': 0, 'limit': 200, 'asc': True}]
    assert requests('/api/sso/login/token') == [{}, {}]
    assert (root / 'browser-bin/xdg-open.receipt').read_text(encoding='utf-8').splitlines() == [
        '1', 'https://music.163.com/sso/login?token=SYNTHETIC%2BPROFILE%26SSO&uid=2'
        '&url=%2Fradio%2Fmy%2F%23%2Fplist&forceReplaceLogin=true',
        '1', 'https://music.163.com/radio/my/#/plist']
    assert native['feed'] == [] and native['details'] == native['releaseReads'] == 0


def verify_profile_playlists(stages, native):
    assert set(stages) == {'guest_created', 'guest_collected', 'own_created', 'own_private_detail',
                           'own_back_focus', 'own_collected', 'blocked'}
    assert stages['guest_created']['ids'] == ['20', '26', '25']
    assert stages['guest_collected']['ids'] == ['29']
    assert stages['own_created']['ids'] == stages['own_back_focus']['ids'] == ['41', '42', '46', '22']
    assert stages['own_back_focus']['focused'] == '42'
    assert stages['own_collected']['ids'] == ['45']
    assert stages['blocked']['ids'] == []

    def detail(id_):
        return ['/api/v6/playlist/detail', {'id': id_, 'n': 100000, 's': 0}]

    def profile(uid):
        return [f'/api/w/v1/user/detail/{uid}', {'all': True, 'userId': uid}]

    def groups(uid):
        return [['/api/djradio/get/create/sub/count', {'userId': uid}],
                ['/api/user/playlist', {'uid': uid, 'offset': 0, 'limit': 1000}],
                ['/api/user/playlist/getTop', {'userId': uid, 'offset': 0, 'limit': 1000}]]

    calls = native['profilePlaylistCalls']
    assert calls == [detail('20'), profile('73'), *groups('73'), detail('22'), profile('2'),
                     *groups('2'), detail('42'), detail('21'), profile('74')], calls


def verify_profile_follow(stages, native):
    assert set(stages) == {'guest_ready', 'followed', 'confirm', 'cancel', 'uncertain', 'readback',
                           'own', 'target_blocked', 'closed', 'viewer_blocked'}
    assert stages['confirm']['dialog'] and not stages['cancel']['dialog']
    assert stages['confirm']['active'] == '继续关注' and stages['cancel']['active'] == '已互关'

    def detail(uid):
        return [f'/api/w/v1/user/detail/{uid}', {'all': True, 'userId': str(uid)}]

    calls = native['profileFollowCalls']
    assert calls == [detail(73), detail(73), ['/api/user/follow/73', {}], detail(73),
                     detail(73), ['/api/user/delfollow/73', {}], detail(73), detail(73),
                     detail(2), detail(74), detail(75), detail(76)], calls


def verify_community(stages, native):
    assert set(stages) == {'first_error', 'feed', 'notice_return', 'detail_error', 'comment_error', 'comments',
                           'comment_next', 'back_to_feed', 'unavailable', 'next_error',
                           'next_recovered', 'empty', 'composer_draft', 'composer_search',
                           'composer_restored'}
    assert len(stages) == 15
    assert stages['feed']['theme'] == 'light' and stages['empty']['theme'] == 'dark'
    assert stages['next_error']['feedCount'] == 5 and stages['next_recovered']['feedCount'] == 6
    assert native['feed'] == [{'cursor': '-1'}, {'cursor': '-1'},
                              {'cursor': '90071992547409933331'}] + [
                                  {'cursor': '90071992547409933331'}, {'cursor': -1}]
    assert native['details'] == 2 and native['unavailable'] == 1
    assert native['commentOffsets'] == [0, 0, 2]
    assert native['notice'][:2] == [{}, {}] and len(native['notice']) == 3
    assert list(native['notice'][2]) == ['time'] and native['notice'][2]['time'] > 1700000000000
    assert native['releaseReads'] == 2
    assert [path for path, _ in native['composer']] == [
        '/api/search/song/list/page', '/api/search/song/list/page', '/api/search/song/list/page',
        '/api/v1/search/artist/get', '/api/v1/search/playlist/get', '/api/search/voicelist/get',
        '/api/v1/search/album/get',
    ]
    assert [body['offset'] for _, body in native['composer']] == [0, 8, 8, 0, 0, 0, 0]


def verify_publication(stages, native):
    assert set(stages) == {'publication_confirm', 'publication_cancel', 'publication_phone',
                           'publication_ack', 'publication_detail', 'publication_unknown',
                           'publication_reopened', 'publication_readback_absent',
                           'publication_recovered', 'publication_recovered_detail'}
    calls = native['publication']
    assert [path for path, _ in calls] == [
        '/api/w/v1/user/bindings/2', '/api/w/v1/user/bindings/2',
        '/api/share/friends/resource', '/api/v1/event/2/9007199254740997',
        '/api/w/v1/user/bindings/2', '/api/share/friends/resource',
        '/api/event/get/2', '/api/event/get/2', '/api/v1/event/2/9007199254740998',
    ], calls
    assert calls[6][1] == calls[7][1] == {
        'offset': 0, 'limit': 20, 'getcounts': True, 'time': -1,
    }
    assert calls[2][1]['type'] == 'album' and calls[5][1]['type'] == 'djradio'
    assert calls[2][1]['uuid'] != calls[5][1]['uuid']
    assert len(native['feed']) == 2, native['feed']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--profile-notes', action='store_true')
    mode.add_argument('--profile-podcasts', action='store_true')
    mode.add_argument('--own-profile', action='store_true')
    mode.add_argument('--publication', action='store_true')
    mode.add_argument('--profile-playlists', action='store_true')
    mode.add_argument('--profile-follow', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    script = ('own_profile_navigation_ui_probe.js' if args.own_profile else
              'profile_notes_ui_probe.js' if args.profile_notes else
              'profile_podcasts_ui_probe.js' if args.profile_podcasts else
              'profile_playlists_ui_probe.js' if args.profile_playlists else
              'profile_follow_ui_probe.js' if args.profile_follow else
              'community_publication_ui_probe.js' if args.publication else 'community_ui_probe.js')
    env.update(CUMULUS_PROOF_DIR=str(root / 'run'),
               CUMULUS_UX_SCRIPT=str(Path(__file__).resolve().parents[2] / 'desktop/examples' / script))
    if args.profile_notes:
        env['CUMULUS_PROFILE_NOTES_PROOF'] = '1'
    if args.own_profile:
        env['CUMULUS_OWN_PROFILE_PROOF'] = '1'
    if args.publication:
        env['CUMULUS_COMMUNITY_PUBLICATION_PROOF'] = '1'
    if args.profile_playlists:
        env['CUMULUS_PROFILE_PLAYLISTS_PROOF'] = '1'
    if args.profile_follow:
        env['CUMULUS_PROFILE_FOLLOW_PROOF'] = '1'
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        path = root / 'os-profile' / name
        path.mkdir(parents=True, mode=0o700)
        env[key] = str(path)
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    if args.profile_podcasts:
        env['CUMULUS_PROFILE_PODCASTS_PROOF'] = '1'
        # Verify both fixed browser routes inside the isolated network namespace.
        browser = root / 'browser-bin/xdg-open'
        browser.parent.mkdir()
        browser.write_text('#!/bin/sh\nprintf "%s\\n" "$#" "$1" >> "$0.receipt"\nsleep 0.2\n',
                           encoding='utf-8')
        browser.chmod(0o700)
        env['PATH'] = str(browser.parent) + os.pathsep + env.get('PATH', '')
    output = root / 'run/first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        status = launch(args.binary.resolve(strict=True), env, log, output, args.screenshots)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert status == 0, f'community process exited {status}'
    complete = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['realMusicRequests'] == complete['remoteWrites'] == 0
    stages = {path.stem.split('_', 2)[2]: json.loads(path.read_text(encoding='utf-8'))
              for path in output.glob('ux_*.json')}
    assert len(stages) == complete['stageCount']
    assert all(stage['width'] == 760 for stage in stages.values())
    if args.own_profile:
        assert set(stages) == {'own_profile', 'account_controls', 'own_notes', 'own_following', 'own_followers'}
        calls = complete['native']['profileNotesCalls']
        assert [path for path, _ in calls if path != '/api/w/nuser/account/get'] == [
            '/api/w/v1/user/detail/2', '/api/event/get/2',
            '/api/user/follow/users/mixed/get/v2', '/api/user/getfolloweds/',
        ], calls
    elif args.publication:
        assert complete['publicationVerified']
        verify_publication(stages, complete['native'])
    elif args.profile_notes:
        verify_profile_notes(stages, complete['native'])
    elif args.profile_podcasts:
        verify_profile_podcasts(stages, complete['native'], root)
    elif args.profile_playlists:
        verify_profile_playlists(stages, complete['native'])
    elif args.profile_follow:
        verify_profile_follow(stages, complete['native'])
    else:
        assert complete['composerVerified']
        verify_community(stages, complete['native'])
    if args.screenshots:
        assert len(list(output.glob('ux_*.png'))) == len(stages)
    report = {'ok': True, 'actualWebView': True, 'states': len(stages),
              'screenshots': len(stages) if args.screenshots else 0,
              'realMusicRequests': 0, 'remoteAccountWrites': 0}
    if args.own_profile:
        report['ownProfileVerified'] = True
    elif args.publication:
        report['publicationVerified'] = True
    elif args.profile_notes:
        report['profileNotesCalls'] = complete['native']['profileNotesCalls']
    elif args.profile_podcasts:
        report['profilePodcastCalls'] = complete['native']['profilePodcastCalls']
        report['browserHandoff'] = {'officialSso': True, 'plainFallback': True}
    elif args.profile_playlists:
        report['profilePlaylistCalls'] = complete['native']['profilePlaylistCalls']
    elif args.profile_follow:
        report['profileFollowCalls'] = complete['native']['profileFollowCalls']
    else:
        report['composerVerified'] = True
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
