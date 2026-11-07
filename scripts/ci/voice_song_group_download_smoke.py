"""Isolated Linux WebView SongCard group download with trusted XTest input."""

import argparse
from hashlib import sha256
import json
import os
from pathlib import Path

from queue_collection_smoke import isolated_profile
from vinyl_lyric_copy_smoke import require_xvfb
from voice_song_download_smoke import AUTH, META, MP3, actions, persisted
from voice_song_multi_smoke import TrustedSongInput, suite_window_manager
from voice_songs_smoke import TRACKS, read
from window_manager import launch

PREFIX = ('baseline', 'select_second', 'select_first', 'select_anchor',
          'open_unliked', 'unliked_offered', 'open_liked', 'click_download')
PURCHASED = 'group_purchased'
DESTINATION = 'playerVoiceSongsDownloadDestination'
PICKER_PREFIX = (*PREFIX[:6], 'click_unliked_download')
PICKERS = {
    'group_playlist': (*PICKER_PREFIX, 'picker', 'sort_recently', 'select_destination', 'queued', 'transferred'),
    'group_playlist_liked': (*PICKER_PREFIX, 'picker', 'close_prepared_picker', 'reopen_unliked_menu',
                            'reopen_unliked_download', 'select_destination', 'pending', 'hide_pending',
                            'resume_pending', 'reconcile', 'queued', 'transferred'),
    'group_purchased_playlist': (*PREFIX[:4], 'select_purchased', *PICKER_PREFIX[4:],
                                'picker_choose_hires', 'picker_confirm_hires', 'picker',
                                'open_create', 'private_create', 'confirm_create', 'create_failed'),
}
PURCHASED_PREFIX = (*PREFIX[:4], 'select_purchased', *PREFIX[4:])
CASES = {
    'group_liked': PREFIX + ('queued', 'transferred'),
    'group_directory': PREFIX + ('missing_directory', 'open_settings', 'settings'),
    PURCHASED: PURCHASED_PREFIX + (
        'open_guide', 'close_chooser', 'cancelled', 'reopen_menu', 'reopen_chooser',
        'choose_hires', 'confirm_hires', 'queued', 'transferred',
    ),
    **PICKERS,
}
CARD = 'playerVoiceSongs'
PREPARE = 'playerVoiceSongsDownloadPrepare'
START = 'playerVoiceSongsDownloadStart'
CONFIRM = 'playerVoiceSongsDownloadConfirmQuality'
GUIDE = 'openVoiceSongGroupPurchasedQualityGuide'
CANCEL = 'playerVoiceSongsDownloadCancel'
TIERS = ['sky', 'hires', 'exhigh', 'standard']
GUIDE_URL = 'https://y.music.163.com/m/at/648fba3cbe7c73d87cbc2e81'


def verify(case, root, output):
    purchased = case == PURCHASED
    stages = {path.stem.split('_', 2)[2]: read(path) for path in sorted(output.glob('ux_*.json'))}
    assert tuple(stages) == CASES[case], list(stages)
    done = read(output / 'complete.json')
    assert done['ok'] and done['mode'] == case and done['count'] == len(stages)
    assert done['realMusicRequests'] == done['realAccountWrites'] == 0 and not done['physicalAudioVerified']
    locale = read(output / 'locale.json')
    assert locale['numeric'] == 'C' and all(locale[key] != 'C' for key in ('messages', 'characters')), locale
    first, last = stages['baseline'], list(stages.values())[-1]
    assert first['player']['kind'] == 'voice' and first['player']['state'] == 'paused'
    assert first['player']['voiceId'] == '601' and first['player']['position'] > .2
    assert not first['player']['seeking'] and not first['player']['error']
    assert all(stage['actual'] and stage['player'] == first['player'] and stage['queue'] == first['queue']
               for stage in stages.values())
    assert all(not stage['dom']['singlePicker'] for stage in stages.values())
    assert stages['unliked_offered']['dom']['collect'] and stages['unliked_offered']['dom']['download']
    if case in PICKERS:
        return verify_picker(case, root, output, stages, done)
    assert stages['click_download']['dom']['collect'] and stages['click_download']['dom']['download']
    selected = [0, 1, 2, 3] if purchased else [0, 1, 2]
    assert stages['unliked_offered']['selected'] == stages['click_download']['selected'] == selected
    for name in PURCHASED_PREFIX if purchased else PREFIX:
        stage = stages[name]
        assert not stage['tasks']['active'] and not stage['tasks']['complete']
        assert not actions(stage, PREPARE, START, CONFIRM)
    [card] = actions(last, CARD)
    epoch, play_id = card['args']
    assert play_id == first['player']['playId'] == card['result']['playId']
    assert card['result']['voiceId'] == '601'
    hints = ['ordinary', 'purchased', 'omitted', 'purchased'] if purchased else [
        'ordinary', 'ordinary', 'omitted', 'unsupported',
    ]
    assert [item['groupDownload'] for item in card['result']['availability']] == hints
    scope = {'epoch': epoch, 'playId': play_id, 'cardReceipt': card['result']['receipt']}
    prepares = actions(last, PREPARE)
    assert len(prepares) == (2 if purchased else 1)
    for prepare in prepares:
        assert prepare['args'] == [{**scope, 'anchorIndex': 2,
                                    'indices': [1, 0, 2, 3] if purchased else [1, 0, 2]}]
        plan = prepare['result']
        assert plan['eligibleIndices'] == ([1, 0, 3] if purchased else [1, 0])
        assert plan['omittedIndices'] == [2]
        assert plan['epoch'] == epoch and plan['playId'] == play_id and plan['anchorIndex'] == 2
    plan = prepares[-1]['result']
    expected_clicks = [
        ('row:1', 'click', False), ('row:0', 'click', True), ('row:2', 'click', True),
        ('row:0', 'contextmenu', False), ('row:2', 'contextmenu', False),
        ('menu:download', 'click', False),
    ]
    if case == 'group_directory':
        expected_clicks.append(('directory:settings', 'click', False))
    if purchased:
        expected_clicks.insert(3, ('row:3', 'click', True))
        expected_clicks.extend([
            ('quality:guide', 'click', False), ('quality:close', 'click', False),
            ('row:2', 'contextmenu', False), ('menu:download', 'click', False),
            ('quality:hires', 'click', False), ('quality:confirm', 'click', False),
        ])
    assert all(event['trusted'] and not event['shift'] for event in last['events'])
    assert [(event['target'], event['type'], event['ctrl']) for event in last['events']] == expected_clicks
    assert len(list(output.glob('input-ux_*.json'))) == len(expected_clicks)
    assert len(list(output.glob('ux_*.png'))) == len(stages)
    native = done['voiceSongDownloadTrace']
    assert native['likedReads'] > 0
    assert done['voiceSongsTrace']['songReads'] == [[TRACKS, {'id': '601'}]]
    assert not done['voiceSongsTrace']['likes']
    records = persisted(root)
    assert all(kind == 'download_tasks_v1' for kind, _ in records), records.keys()

    if case != 'group_directory':
        if purchased:
            first_plan = prepares[0]['result']
            assert all(p['result']['decision'] == 'purchased_quality_required' and
                       p['result']['purchasedQualities'] == TIERS and p['result']['selectionReceipt']
                       for p in prepares)
            assert plan['selectionReceipt'] != first_plan['selectionReceipt']
            assert [item['name'] for item in last['commands']] == [
                CARD, PREPARE, GUIDE, CANCEL, PREPARE, CONFIRM,
            ]
            first_target = {**scope, 'selectionReceipt': first_plan['selectionReceipt']}
            [guide] = actions(last, GUIDE)
            [cancel] = actions(last, CANCEL)
            assert guide['args'] == cancel['args'] == [first_target]
            assert guide['result'] is cancel['result'] is None
            assert (root / 'browser-arguments').read_text(encoding='utf-8').splitlines() == ['1', GUIDE_URL]
            for stage in list(stages.values())[:-2]:
                assert not stage['tasks']['active'] and not stage['tasks']['complete']
                assert not actions(stage, START, CONFIRM)
                assert not stage['voiceSongDownloadTrace']['calls']
            assert stages['cancelled']['dom']['quality'] is None
            assert stages['cancelled']['dom']['focusRow'] == '2'
            for stage in ('open_guide', 'close_chooser', 'choose_hires', 'confirm_hires'):
                assert stages[stage]['dom']['quality'] == {
                    'heading': '选择下载音质', 'selected': 'hires' if stage == 'confirm_hires' else 'sky',
                    'tiers': TIERS, 'confirm': True,
                }
            [confirmed] = actions(last, CONFIRM)
            assert confirmed['args'] == [{**scope, 'selectionReceipt': plan['selectionReceipt']}, 'hires']
            assert confirmed['result']['kind'] == 'started'
            result = confirmed['result']['result']
            assert last['savedQuality'] == 'exhigh'
        else:
            assert plan['decision'] == 'ready' and plan['selectionReceipt']
            assert [item['name'] for item in last['commands']] == [CARD, PREPARE, START]
            [start] = actions(last, START)
            assert start['args'] == [{**scope, 'selectionReceipt': plan['selectionReceipt']}]
            result = start['result']
        accepted = result['accepted']
        assert result['operationId'] == plan['selectionReceipt']
        ids = ['1712', '1710', '1791'] if purchased else ['1712', '1710']
        tier = 'hires' if purchased else 'exhigh'
        assert len(accepted) == len(records) == len(ids) and not result['failed']
        tasks = [records['download_tasks_v1', ident] for ident in accepted]
        assert [task['song_id'] for task in tasks] == ids
        for task in tasks:
            assert task['status'] == 'complete' and task['origin'] == {'kind': 'song'}
            assert task['options']['quality'] == tier and task['authorization_count'] == 1
            target = Path(task['target']).resolve(strict=True)
            assert target.parent == (root / 'run/download-output').resolve()
            assert target.read_bytes() == MP3.read_bytes()
        calls = native['calls']
        assert len(calls) == 3 * len(ids) and {path for path, _ in calls} == {AUTH, META, 'media_get'}
        assert sorted(body['id'] for path, body in calls if path == AUTH) == sorted(ids)
        assert all(body['level'] == tier and body['immerseType'] == 'c51'
                   for path, body in calls if path == AUTH)
        assert [body for path, body in calls if path == 'media_get'] == [
            {'offset': 0, 'bytes': MP3.stat().st_size},
        ] * len(ids)
        assert stages['queued']['dom']['notice'] == f'已添加到下载列表（{len(ids)} 首，跳过 1 首）'
        assert stages['queued']['dom']['noticeKind'] == 'queued'
        assert all(stage['dom']['vinyl'] and not stage['dom']['downloads'] and not stage['dom']['settings']
                   for stage in stages.values())
    else:
        assert plan['decision'] == 'directory_required' and plan['selectionReceipt'] is None
        assert [item['name'] for item in last['commands']] == [CARD, PREPARE]
        assert not native['calls'] and not records
        assert not list((root / 'run/download-output').iterdir())
        notice = stages['missing_directory']['dom']
        assert notice['noticeKind'] == 'directory' and notice['action'] == '下载设置'
        assert notice['vinyl'] and not notice['settings'] and not notice['downloads']
        assert not last['dom']['vinyl'] and last['dom']['downloads']
        assert last['dom']['settings'] == '音质与下载' and last['dom']['settingsDirectory']

    return {'case': case, 'stages': len(stages), 'screenshots': len(stages), 'tasks': len(records),
            'nativeDownloadCalls': len(native['calls']),
            **({'trustedGroupGuideHandoff': True, 'requestedQuality': 'hires',
                'savedQuality': last['savedQuality'], 'explicitConfirmations': 1} if purchased else {}),
            'syntheticMp3Sha256': sha256(MP3.read_bytes()).hexdigest() if records else None}


def verify_picker(case, root, output, stages, done):
    star = case == 'group_playlist_liked'
    purchased = case == 'group_purchased_playlist'
    last = list(stages.values())[-1]
    playlist = '/api/v6/playlist/detail'
    liked = '/api/song/like/get'
    owned = '/api/user/playlist'
    add = '/api/v1/playlist/manipulate/tracks'
    create = '/api/playlist/create'
    listing = '/api/user/playlist/v2'
    records = persisted(root)
    wire = done['voiceSongDownloadTrace']['calls']
    [card] = actions(last, CARD)
    epoch, play_id = card['args']
    scope = {'epoch': epoch, 'playId': play_id, 'cardReceipt': card['result']['receipt']}
    prepares = actions(last, PREPARE)
    assert len(prepares) == (2 if star else 1)
    for item in prepares:
        assert item['args'] == [{**scope, 'anchorIndex': 0, 'indices': [1, 0, 2, 3] if purchased else [1, 0, 2]}]
        assert item['result']['eligibleIndices'] == ([1, 0, 3] if purchased else [1, 0])
        assert item['result']['omittedIndices'] == [2]
        assert item['result']['decision'] == ('purchased_quality_required' if purchased else 'playlist_required')
    selection = prepares[-1]['result']['selectionReceipt']
    assert selection
    target = {**scope, 'selectionReceipt': selection}
    assert not actions(last, START)
    assert not done['voiceSongsTrace']['likes']
    assert done['voiceSongsTrace']['songReads'] == [[TRACKS, {'id': '601'}]]
    picker = stages['picker']['dom']['destination']
    assert picker['heading'] == '下载到歌单' and picker['stage'] == 'pick'
    assert picker['star'] is star
    assert picker['rows'] == ['创建新歌单', *(['我喜欢的音乐'] if star else []),
                              '下载到：合成已有下载歌单'], picker
    assert picker['focus'] == '选择下载歌单'
    assert not any(stage['tasks']['active'] or stage['tasks']['complete']
                   for name, stage in stages.items() if name not in ('queued', 'transferred'))
    assert all(stage['dom']['vinyl'] and not stage['dom']['downloads'] and not stage['dom']['settings']
               for stage in stages.values())
    assert not any(row[0] in (add, create, AUTH) for row in stages['picker']['voiceSongDownloadTrace']['calls'])
    for path, stage in zip(sorted(output.glob('ux_*.json')), stages.values(), strict=True):
        assert path.with_suffix('.png').is_file()
        if 'input' not in stage:
            continue
        receipt = read(path.with_name('input-' + path.name))
        assert receipt['request'] == stage['input']
        expected = stage['input']
        assert any(event['trusted'] and event['target'] == expected['target'] and
                   event['type'] == ('contextmenu' if expected['button'] == 3 else 'click') and
                   event['ctrl'] == expected['ctrl'] for event in last['events'])
    for action in actions(last, DESTINATION):
        args, command = action['args']
        assert args in [{**scope, 'selectionReceipt': item['result']['selectionReceipt']} for item in prepares]
        if command['kind'] not in ('page', 'liked_destination'):
            assert args == target
    calls = {row['args'][1]['kind']: row for row in actions(last, DESTINATION)}
    [sent] = [row for row in wire if row[0] == add]
    assert sent[1] == {'pid': '82' if star else '84' if purchased else '81',
                       'op': 'add', 'trackIds': '["1710"]'}
    if purchased:
        [confirmation] = actions(last, CONFIRM)
        assert confirmation['args'] == [target, 'hires']
        assert confirmation['result'] == {'kind': 'playlist_required'}
        assert not actions(stages['picker_confirm_hires'], DESTINATION)
        assert 'start' not in calls and not any(row[0] in (AUTH, META, 'media_get') for row in wire)
        [made] = [row for row in wire if row[0] == create]
        assert made[1] == {'uid': '2', 'name': '合成私密下载歌单', 'privacy': 10}
        result = calls['create']['result']['creation']
        assert result['createPhase'] == 'confirmed' and result['playlistId'] == '84'
        attempt = result['add']
        assert attempt['phase'] == 'rejected' and not attempt['continuation']
        assert set(records) == {('playlist-mutations', result['createOperationId']),
                                ('playlist-mutations', attempt['operationId'])}
        assert last['dom']['destination']['stage'] == 'failed'
        return {'case': case, 'stages': len(stages), 'screenshots': len(stages), 'tasks': 0,
                'playlistCreates': 1, 'playlistAdds': 1, 'requestedQuality': 'hires'}

    chosen = calls['choose_liked' if star else 'choose']['result']['playlist']
    if star:
        assert len(actions(last, CANCEL)) == 1
        assert prepares[0]['result']['selectionReceipt'] != selection
        assert stages['pending']['dom']['destination']['focusedReconcile']
        assert stages['resume_pending']['dom']['noticeKind'] == 'playlist_pending'
        assert stages['resume_pending']['dom']['focusRow'] == '0'
        assert chosen['phase'] == 'uncertain' and not chosen['continuation'] and 'likedIds' not in chosen
        assert calls['status']['args'] == [target, {'kind': 'status', 'reconcile': True}]
        assert len([x for x in actions(last, DESTINATION) if x['args'][1]['kind'] == 'status']) == 1
        reconciled = calls['status']['result']['playlist']
        assert reconciled['operationId'] == chosen['operationId'] and reconciled['phase'] == 'confirmed'
        assert reconciled['continuation'] and sent[2] == 'lost_ack'
        assert reconciled['likedIds'] == ['1710']
        assert stages['pending']['dom']['heartPressed'] == ['false', 'false']
        assert stages['queued']['dom']['heartPressed'] == ['true', 'false']
        assert done['voiceSongGroupLikedTrace']['transport'] == 'MusicHttpClient'
        signed_paths = [row[0] for row in wire if row[0] in (owned, playlist, liked, add)]
        assert signed_paths == [owned, playlist, owned, playlist, owned, playlist, liked, add,
                                owned, playlist, liked, owned, playlist, liked], signed_paths
        assert [row[2] for row in wire if row[0] == liked] == [False, False, True]
        assert len([row for row in stages['pending']['voiceSongDownloadTrace']['calls'] if row[0] == liked]) == 2
        allowed = reconciled
    else:
        assert [row[1]['sortType'] for row in wire if row[0] == listing] == ['default', 'recently']
        assert [row[0] for row in wire if row[0] not in (AUTH, META, 'media_get')] == [
            listing, listing, playlist, add, playlist]
        assert chosen['phase'] == 'rejected' and chosen['addFailed'] and chosen['continuation']
        assert 'likedIds' not in chosen and stages['queued']['dom']['heartPressed'] == ['false', 'false']
        assert stages['queued']['dom']['notice'].startswith('未能收藏到歌单，')
        allowed = chosen
    journal = records.pop(('playlist-mutations', chosen['operationId']))
    assert journal['phase'] == ('confirmed' if star else 'rejected')
    assert journal.get('system_liked_group', False) is star
    assert calls['start']['args'] == [target, {'kind': 'start', 'continuation': allowed['continuation']}]
    [started] = [row for row in actions(last, DESTINATION) if row['args'][1]['kind'] == 'start']
    result = started['result']['result']
    assert result['operationId'] == selection and not result['failed']
    assert len(result['accepted']) == len(records) == 2
    tasks = [records['download_tasks_v1', ident] for ident in result['accepted']]
    assert [row['song_id'] for row in tasks] == ['1710', '1712']
    for task in tasks:
        assert task['status'] == 'complete' and task['origin'] == {'kind': 'song'}
        assert task['options']['quality'] == 'exhigh' and task['authorization_count'] == 1
        path = Path(task['target']).resolve(strict=True)
        assert path.parent == (root / 'run/download-output').resolve()
        assert path.read_bytes() == MP3.read_bytes()
    assert sorted(row[1]['id'] for row in wire if row[0] == AUTH) == ['1710', '1712']
    return {'case': case, 'stages': len(stages), 'screenshots': len(stages), 'tasks': len(tasks),
            'playlistAdds': 1, 'explicitReconciles': int(star),
            'syntheticMp3Sha256': sha256(MP3.read_bytes()).hexdigest()}

def run(args, case, root):
    root.mkdir()
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root / 'run'), CUMULUS_VOICE_SONG_DOWNLOAD_PROOF=case,
               CUMULUS_PODCAST_SCENE_HOME_PROOF='automatic', LC_ALL='en_US.UTF-8', GDK_BACKEND='x11',
               CUMULUS_PROOF_FRONTEND_DIST=str(args.frontend_dist.resolve(strict=True)))
    isolated_profile(env, root / 'application')
    if case == PURCHASED:
        launcher = root / 'bin/xdg-open'
        launcher.parent.mkdir()
        launcher.write_text('#!/bin/sh\nprintf "%s\\n" "$#" "$1" >> "$CUMULUS_PROOF_BROWSER_ARGUMENTS"\n',
                            encoding='utf-8')
        launcher.chmod(0o700)
        env['PATH'] = str(launcher.parent) + os.pathsep + env.get('PATH', '')
        env['CUMULUS_PROOF_BROWSER_ARGUMENTS'] = str(root / 'browser-arguments')
    output = root / 'run/first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        driver = TrustedSongInput()
        try:
            status = launch(args.binary.resolve(strict=True), env, log, output, True,
                            before_ack=driver.at_capture, capture_glob='ux_[0-9][0-9]_*.json')
        finally:
            driver.close()
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert status == 0, (root / 'host.log').read_text(encoding='utf-8')
    return verify(case, root, output)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path, required=True)
    parser.add_argument('--case', choices=('all', *CASES), default='all')
    args = parser.parse_args()
    require_xvfb()
    if os.environ.get('CUMULUS_TEST_WINDOW_MANAGER') != 'openbox':
        parser.error('set CUMULUS_TEST_WINDOW_MANAGER=openbox on the dedicated Xvfb display')
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    with suite_window_manager(root):
        report = [run(args, case, root / case) for case in (CASES if args.case == 'all' else [args.case])]
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
