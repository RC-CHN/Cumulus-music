"""Real Linux search-group download, native receipt, ordered task and lost-ack readback."""
import argparse
from contextlib import ExitStack, closing
import json
import os
from pathlib import Path
import sqlite3

from search_group_next_smoke import TrustedClicks
from vinyl_lyric_copy_smoke import require_xvfb
from window_manager import launch

DIRECT = ('first', 'blocked', 'last', 'anchor', 'download', 'check', 'accepted')
PLAYLIST = ('first', 'blocked', 'last', 'collect_filtered', 'anchor', 'download', 'choose', 'reconcile', 'accepted')
CREATED = ('first', 'blocked', 'last', 'collect_filtered', 'anchor', 'download', 'open_create',
           'private', 'create', 'creation_readback', 'add_created', 'accepted')
PURCHASED = ('first', 'blocked', 'last', 'paid2', 'anchor', 'download', 'cancel', 'anchor_again',
             'download_again', 'lower_tier', 'confirm', 'check', 'accepted')
PURCHASED_RESUME = (*PURCHASED[:PURCHASED.index('lower_tier')], 'wrong_tier',
                    *PURCHASED[PURCHASED.index('lower_tier'):])


def load(path):
    return json.loads(path.read_text(encoding='utf-8'))


def flac_frames(path):
    data = path.read_bytes()
    assert data.startswith(b'fLaC')
    offset = 4
    while True:
        last = bool(data[offset] & 0x80)
        length = int.from_bytes(data[offset + 1:offset + 4], 'big')
        offset += 4 + length
        assert offset < len(data)
        if last:
            return data[offset:]


def run(args):
    repo = Path(__file__).resolve().parents[2]
    root = args.root.resolve()
    root.mkdir(parents=True, mode=0o700)
    resume = getattr(args, 'resume', False)
    has_partial = args.mixed or resume
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1', CUMULUS_SEARCH_UI_PROOF='1',
               CUMULUS_SEARCH_GROUP_NEXT_PROOF='1', CUMULUS_SEARCH_GROUP_DOWNLOAD_PROOF='1',
               CUMULUS_UX_SCRIPT=str(repo / 'desktop/examples/search_group_download_ui_probe.js'),
               CUMULUS_PROOF_FRONTEND_DIST=str(args.frontend_dist.resolve(strict=True)))
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime')]:
        directory = root / 'os-profile' / name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    if args.playlist:
        env['CUMULUS_SEARCH_GROUP_DOWNLOAD_DESTINATION_PROOF'] = '1'
    if args.liked:
        env['CUMULUS_SEARCH_GROUP_DOWNLOAD_LIKED_PROOF'] = '1'
    if args.purchased:
        env['CUMULUS_SEARCH_GROUP_DOWNLOAD_PURCHASED_PROOF'] = '1'
        env['CUMULUS_UX_SCRIPT'] = str(repo / 'desktop/examples/search_group_purchased_download_ui_probe.js')
    if args.mixed:
        env['CUMULUS_SEARCH_GROUP_DOWNLOAD_MIXED_PROOF'] = '1'
    if resume:
        env['CUMULUS_SEARCH_GROUP_DOWNLOAD_PURCHASED_RESUME_PROOF'] = '1'
    if args.created:
        env['CUMULUS_SEARCH_GROUP_DOWNLOAD_CREATED_PROOF'] = '1'
    output = root / 'first'
    labels = (PURCHASED_RESUME if resume else PURCHASED) if args.purchased else (
        CREATED if args.created else (*PLAYLIST[:-1], 'check', 'accepted') if args.mixed else
        PLAYLIST if args.playlist else DIRECT
    )
    with ExitStack() as stack:
        driver = TrustedClicks()
        stack.callback(driver.close)
        log = stack.enter_context((root / 'host.log').open('x', encoding='utf-8'))
        code = launch(args.binary.resolve(strict=True), env, log, output, True,
                      before_ack=driver.at_capture)
    assert not (output / 'failed.json').exists(), (
        load(output / 'failed.json') if (output / 'failed.json').exists() else ''
    )
    assert code == 0
    report = load(output / 'complete.json')
    assert report['ok'] and report['stageCount'] == len(labels)
    assert report['realAccountWrites'] == report['realMusicRequests'] == 0
    rows = []
    for index, label in enumerate(labels, 1):
        path = output / f'ux_{index:02}_{label}.json'
        rows.append(load(path))
        assert path.with_suffix('.png').is_file()
        if label in ('choose', 'reconcile'):
            paint = load(path.with_name('paint-' + path.name))
            assert len(paint['samples']) == 2 and all(sample['ok'] for sample in paint['samples'])
        if args.purchased and label == 'confirm':
            paint = load(path.with_name('paint-' + path.name))
            selected, previous = paint['samples']
            assert selected['region'] == 'hires-tick' and selected['present'] and selected['ok']
            assert previous['region'] == 'sky-no-tick' and not previous['present'] and previous['ok']
    first, last = rows[0], rows[-1]
    prefix = 2 if has_partial else 0
    old = first.get('old')
    if args.mixed:
        assert old and old['bytes'] == 2048
        assert first['searchGroupDownload'] == [
            {'kind':'authorize','args':{'id':'104','level':'lossless','immerseType':'c51'},'cached':False},
            {'kind':'media','id':'104','offset':0},
        ]
    elif resume:
        assert old and old['bytes'] == 2048
        assert first['searchGroupDownload'] == [
            {'kind':'authorize','args':{'id':'103','level':'hires','immerseType':'c51'},'cached':False},
            {'kind':'media','id':'103','offset':0,'status':200,'contentRange':None},
        ]
    else:
        assert old is None and first['searchGroupDownload'] == []
    assert all(row['queue'] == first['queue'] for row in rows)
    assert all(row['player']['position'] == first['player']['position'] for row in rows)
    ordered = (['104', '101', '103'] if resume else ['103', '101', '104']) if args.purchased else (
        ['104', '101', '103'] if args.playlist else ['103', '101'])
    prepare = last['actions'][0]
    assert prepare['command']['anchor']['trackId'] == '102'
    assert prepare['reply']['eligibleCount'] == len(ordered) and prepare['reply']['omittedCount'] == 1
    receipt = prepare['reply']['receipt']
    if not args.purchased:
        assert all(step['command']['receipt'] == receipt for step in last['actions'][1:])
    trace = last['searchGroupDownload'][prefix:]
    if args.purchased:
        prepare, cancel, reopen, *rejected, start, check = last['actions']
        assert [step['command']['kind'] for step in last['actions']] == [
            'prepare', 'cancel', 'prepare', *(['confirm_quality'] if resume else []),
            'confirm_quality', 'status']
        assert prepare['reply']['decision'] == reopen['reply']['decision'] == 'purchased_quality_required'
        assert prepare['reply']['purchasedQualities'] == reopen['reply']['purchasedQualities'] == [
            'sky', 'hires', 'exhigh', 'standard']
        assert [row['trackId'] for row in reopen['command']['rows']] == [ordered[0], '102', *ordered[1:]]
        assert receipt == cancel['command']['receipt'] != reopen['reply']['receipt']
        assert reopen['reply']['receipt'] == start['command']['receipt'] == check['command']['receipt']
        if resume:
            mismatch, = rejected
            assert prepare['reply']['existingPurchasedQuality'] == reopen['reply']['existingPurchasedQuality'] == 'hires'
            assert mismatch['command']['quality'] == 'sky' and mismatch['reply']['kind'] == 'existing_quality_mismatch'
            assert mismatch['command']['receipt'] == mismatch['reply']['receipt'] == reopen['reply']['receipt']
            assert mismatch['reply']['requiredQuality'] == 'hires'
            assert start['reply']['result']['accepted'][-1] == old['id']
        assert start['command']['quality'] == 'hires'
        assert start['reply']['result']['accepted'] == check['reply']['result']['accepted']
        # No auth or transfer beyond the one setup transfer before the matching confirm.
        assert all(row['searchGroupDownload'] == first['searchGroupDownload']
                   for row in rows[:labels.index('confirm') + 1])
        assert all(row['args']['level'] == 'hires' and row['args']['immerseType'] == 'c51'
                   for row in trace if row['kind'] == 'authorize')
    elif args.created:
        prepare, listing, create, check, add, start = last['actions']
        assert [step['command']['kind'] for step in last['actions']] == [
            'prepare', 'destinations', 'create_playlist', 'creation_status', 'add_created',
            'start_from_playlist']
        assert [row['trackId'] for row in prepare['command']['rows']] == ['103', '102', '101', '104']
        assert prepare['reply']['decision'] == 'playlist_required'
        assert listing['reply']['canCreate'] and listing['reply']['systemLikedReceipt'] is None
        assert create['command']['name'] == '合成新下载歌单' and create['command']['privatePlaylist']
        assert create['reply']['result']['playlistId'] is None
        assert create['reply']['result']['add'] is None and not create['reply']['result']['canAdd']
        assert check['command']['reconcile'] and check['reply']['result']['canAdd']
        assert check['reply']['result']['playlistId'] == '91' and check['reply']['result']['add'] is None
        assert not add['reply']['result']['canAdd'] and add['reply']['result']['add']['phase'] == 'confirmed'
        assert add['reply']['result']['add']['continuation'] == start['command']['continuation']
        before = ['playlist_list', 'created_owned', 'playlist_create', 'created_owned', 'created_owned']
        assert [step['kind'] for step in rows[-4]['searchGroupDownload']] == before[:1]
        assert [step['kind'] for step in rows[-3]['searchGroupDownload']] == before[:4]
        assert [step['kind'] for step in rows[-2]['searchGroupDownload']] == before
        before_media = before + ['created_detail', 'playlist_add', 'created_detail']
        assert [step['kind'] for step in trace[:len(before_media)]] == before_media
        assert trace[2]['args'] == {'uid': '2', 'name': '合成新下载歌单', 'privacy': 10}
        assert not trace[3]['hasNew'] and trace[4]['hasNew']
        assert trace[6]['args'] == {'op': 'add', 'pid': '91', 'trackIds': '["103","101"]'}
        assert trace[5]['tracks'] == [] and trace[7]['tracks'] == [{'id': 103}, {'id': 101}]
        trace = trace[len(before_media):]
    elif args.playlist:
        prepare, listing, choose, check, start = last['actions'][:5]
        expected = ['prepare', 'destinations', 'choose_liked' if args.liked else 'choose',
                    'reconcile', 'start_from_playlist']
        if args.mixed:
            expected.append('status')
            assert last['actions'][5]['reply']['result']['accepted'] == start['reply']['result']['accepted']
        assert [step['command']['kind'] for step in last['actions']] == expected
        assert [row['trackId'] for row in prepare['command']['rows']] == ['103', '102', '101', '104']
        assert prepare['reply']['decision'] == 'playlist_required'
        assert [row['id'] for row in listing['reply']['page']['value']['playlists']] == ['81']
        grant = listing['reply']['systemLikedReceipt']
        if args.liked:
            assert grant and choose['command']['destinationReceipt'] == grant
        else:
            assert grant is None and choose['command']['playlistId'] == '81'
        assert choose['reply']['result']['phase'] == 'uncertain'
        assert choose['reply']['result']['continuation'] is None
        assert choose['reply']['result']['likedIds'] == []
        assert check['reply']['result']['phase'] == 'confirmed'
        assert check['reply']['result']['likedIds'] == (['103', '101'] if args.liked else [])
        assert check['reply']['result']['continuation'] == start['command']['continuation']
        if args.liked:
            opened = ['playlist_list', 'liked_list', 'liked_detail']
            observation = ['liked_list', 'liked_detail', 'liked_membership']
            after_choose = opened + observation + ['playlist_add'] + observation
            assert [step['kind'] for step in rows[labels.index('choose')]['searchGroupDownload'][prefix:]] == opened
            assert [step['kind'] for step in rows[labels.index('reconcile')]['searchGroupDownload'][prefix:]] == after_choose
            before_media = after_choose + observation
            assert [step['kind'] for step in trace[:len(before_media)]] == before_media
            assert trace[6]['args'] == {'op': 'add', 'pid': '83', 'trackIds': '["103","101"]'}
            assert trace[8]['tracks'] == trace[11]['tracks'] == [{'id': 103}, {'id': 101}]
            assert trace[9]['ids'] == [103] and trace[12]['ids'] == [103, 101]
            trace = trace[len(before_media):]
        else:
            assert [step['kind'] for step in rows[labels.index('choose')]['searchGroupDownload'][prefix:]] == ['playlist_list']
            assert [step['kind'] for step in rows[labels.index('reconcile')]['searchGroupDownload'][prefix:]] == [
                'playlist_list', 'playlist_detail', 'playlist_add', 'playlist_detail']
            assert [step['kind'] for step in trace[:5]] == [
                'playlist_list', 'playlist_detail', 'playlist_add', 'playlist_detail', 'playlist_detail']
            assert trace[2]['args'] == {'op': 'add', 'pid': '81', 'trackIds': '["103","101"]'}
            assert trace[1]['tracks'] == trace[3]['tracks'] == []
            assert trace[4]['tracks'] == [{'id': 103}, {'id': 101}]
            trace = trace[5:]
    elif not args.purchased:
        prepare, start, check = last['actions']
        assert [step['command']['kind'] for step in last['actions']] == ['prepare', 'start', 'status']
        assert [row['trackId'] for row in prepare['command']['rows']] == ['103', '102', '101']
        assert prepare['reply']['decision'] == 'ready'
        assert start['reply']['result']['accepted'] == check['reply']['result']['accepted']
    ids = start['reply']['result']['accepted']
    assert len(ids) == len(ordered)
    assert sorted(row['args']['id'] for row in trace if row['kind'] == 'authorize') == sorted(ordered)
    assert sorted(row['id'] for row in trace if row['kind'] == 'media') == sorted(ordered)
    assert len(trace) == 2 * len(ordered)
    if args.mixed:
        assert [row['offset'] for row in trace if row['kind'] == 'media' and row['id'] == '104'] == [2048]
        assert [row['args']['level'] for row in trace if row['kind'] == 'authorize'
                and row['args']['id'] == '104'] == ['lossless']
    source = repo / 'core/tests/fixtures/stereo-pcm-source.flac'
    if resume:
        size = source.stat().st_size
        assert [row for row in trace if row['kind'] == 'media' and row['id'] == '103'] == [
            {'kind':'media','id':'103','offset':2048,'status':206,'contentRange':f'bytes 2048-{size - 1}/{size}'},
        ]
        assert all(row['status'] == 200 and row['offset'] == 0 and row['contentRange'] is None
                   for row in trace if row['kind'] == 'media' and row['id'] != '103')
    assert load(output / 'locale.json') == {
        'numeric': 'C', 'messages': 'en_US.UTF-8', 'characters': 'en_US.UTF-8',
    }
    db = (root / 'state' / 'core.sqlite3').resolve(strict=True)
    with closing(sqlite3.connect(db.as_uri() + '?mode=ro', uri=True)) as store:
        tasks = [json.loads(value) for value, in store.execute(
            "SELECT value FROM state WHERE kind='download_tasks_v1' AND owner='2'"
        )]
        if has_partial:
            assert len(tasks) == len(ids)
            tasks.sort(key=lambda row: (row['dispatch']['sequence'], row['dispatch']['index']))
            assert len({row['dispatch']['sequence'] for row in tasks}) == 1
            assert [row['dispatch']['index'] for row in tasks] == [0, 1, 2]
            retained = tasks[-1] if resume else tasks[0]
            assert retained['id'] == old['id'] and retained['created_ms'] == old['createdMs']
            assert retained['options']['quality'] == ('hires' if resume else 'lossless')
            assert not retained['options']['tags'] and not retained['options']['cover']
            assert Path(retained['target']) == root / f"group-previous/original-{retained['song_id']}.flac"
            if resume:
                assert retained['song_id'] == '103' and retained['authorization_count'] == 2
                assert Path(retained['target']).read_bytes() == source.read_bytes()
        else:
            tasks.sort(key=lambda row: (row['created_ms'], row['id']))
        assert [row['id'] for row in tasks] == ids
        assert [row['song_id'] for row in tasks] == ordered
        assert all(row['status'] == 'complete' for row in tasks)
        quality = 'hires' if args.purchased else 'exhigh'
        assert all(row['options']['quality'] == quality for row in tasks[int(args.mixed):])
        journal = [json.loads(value) for value, in store.execute(
            "SELECT value FROM state WHERE kind='playlist-mutations' AND owner='2'"
        )]
        if args.created:
            assert len(journal) == 2 and all(row['phase'] == 'confirmed' for row in journal)
            assert all(row['playlist_id'] == '91' and not row.get('system_liked_group', False) for row in journal)
        elif args.playlist:
            assert len(journal) == 1 and journal[0]['phase'] == 'confirmed'
            assert journal[0].get('system_liked_group', False) == args.liked
        else:
            assert journal == []
    published = list((root / 'group-output').rglob('*.flac'))
    if has_partial:
        published.append(root / f"group-previous/original-{103 if resume else 104}.flac")
    assert len(published) == len(ordered)
    reference = flac_frames(source)
    assert all(flac_frames(path) == reference for path in published)
    summary = {'ok': True, 'screenshots': len(labels), 'actualWebView': True,
               'orderedTasks': ordered, 'anchor': '102', 'readOnlyLostReplyRecovery': not args.created,
               'readOnlyDelayedCreateRecovery': args.created,
               'playlistAddCount': int(args.playlist), 'playlistCreateCount': int(args.created),
               'playlistAddedInOrder': ['103', '101'] if args.playlist else [],
               'systemLikedDestination': args.liked, 'explicitPurchasedQuality': 'hires' if args.purchased else None,
               'exactFlacFrames': True, 'preservedPartialTask': has_partial,
               'purchasedMismatchKeptPaused': resume, 'purchasedResumeStatus': 206 if resume else None,
               'realAccountWrites': 0, 'realMusicRequests': 0}
    (root / 'report.json').write_text(json.dumps(summary, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(summary))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path, required=True)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--playlist', action='store_true')
    mode.add_argument('--liked', action='store_true')
    mode.add_argument('--purchased', action='store_true')
    mode.add_argument('--created', action='store_true')
    parser.add_argument('--mixed', action='store_true')
    parser.add_argument('--resume', action='store_true', help='seed and resume a real same-tier partial task')
    parser.add_argument('--root', type=Path, required=True)
    args = parser.parse_args()
    if args.mixed and not args.playlist:
        parser.error('--mixed requires --playlist')
    if args.resume and not args.purchased:
        parser.error('--resume requires --purchased')
    args.playlist = args.playlist or args.liked or args.created
    require_xvfb()
    run(args)
