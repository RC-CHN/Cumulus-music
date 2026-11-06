"""Real native voice header, square cover/count, creator profile and failed-cover UI."""
import argparse
import json
import os
from pathlib import Path
import sys
from window_manager import launch

STAGES = ('card_placeholder', 'bar_placeholder', 'voice_metadata', 'creator_profile', 'detail_reopened')
TRIAD_STAGES = ('held_no_partial', 'failed_no_partial', 'reopened_fresh', 'mismatch_read_only')
LANES = ('detail', 'recommendations', 'songs')


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def verify_triad(stages, done):
    assert done['ok'] and done['stageCount'] == len(stages) and tuple(stages) == TRIAD_STAGES, done
    assert done['realMusicRequests'] == done['realAccountWrites'] == 0, done
    assert done['physicalAudioVerified'] is False and done['voiceDetailCalls'] == [], done
    baseline = stages[TRIAD_STAGES[0]]['playback']
    assert baseline['state'] == 'paused' and baseline['voice'] == '601' and baseline['position'] > .2, baseline
    assert baseline['playId'] and baseline['entryId'] and not baseline['seeking'] and not baseline.get('error'), baseline
    assert baseline['snapshotCount'] == 123456 and baseline['title'] != '合成新读节目标题', baseline
    assert baseline['snapshotCover'] == 'https://p1.music.126.net/obj/cumulus-palette/cover601.png', baseline
    cover_boxes = [stage['dom']['coverBox'] for stage in stages.values()]
    anchor = cover_boxes[0]
    assert len(anchor) == 4 and anchor[2] > 0 and abs(anchor[2] - anchor[3]) < 0.5, anchor
    assert all(all(abs(a - b) < 0.5 for a, b in zip(anchor, box, strict=True))
               for box in cover_boxes), cover_boxes
    for name, state in zip(TRIAD_STAGES[:2], ('loading', 'failed'), strict=True):
        stage = stages[name]
        assert stage['actual'] and stage['playback'] == baseline, stage
        assert stage['dom'] == {
            'state': state, 'notice': True, 'title': baseline['title'], 'cover': True,
            'coverBox': cover_boxes[TRIAD_STAGES.index(name)],
            'count': None, 'header': False, 'description': None,
            'plainCreator': None, 'creatorLink': False, 'radio': False, 'radioName': None,
            'radioOpenEnabled': False, 'radioSubscribeEnabled': False,
            'recommendations': None, 'songs': None,
        }, stage
        reads = stage['reads']
        assert all(len(reads[lane]) == 1 for lane in LANES), reads
        expected_detail = 'pending' if state == 'loading' else 'failure'
        assert reads['detail'][0]['outcome'] == expected_detail, reads
        for lane, expected_rows in (('recommendations', 4), ('songs', 1)):
            assert reads[lane][0]['outcome'] == 'success' and reads[lane][0]['rows'] == expected_rows, reads
            assert reads[lane][0]['receipt'], reads
        assert sorted((row['lane'], row['voiceId'], row['state']) for row in stage['voicePageReads']) == [
            ('detail', '601', expected_detail), ('recommendations', '601', 'success'), ('songs', '601', 'success'),
        ], stage

    ready = stages['reopened_fresh']
    assert ready['actual'] and ready['playback'] == baseline, ready
    dom = ready['dom']
    assert dom['state'] == 'ready' and dom['notice'] is False and dom['cover'] and dom['header'] and dom['radio'], dom
    assert dom['title'] == '合成新读节目标题' and dom['count'] == '0', dom
    assert dom['description'] == '本段只来自重新读取的节目详情。', dom
    assert dom['creatorLink'] and dom['radioOpenEnabled'], dom
    assert '合成三路推荐 91' in dom['recommendations'] and '合成三路包含歌曲' in dom['songs'], dom
    reads = ready['reads']
    for lane in LANES:
        assert len(reads[lane]) == 2 and reads[lane][1]['outcome'] == 'success', reads
        assert reads[lane][0]['args'] == reads[lane][1]['args'], reads
        assert reads[lane][1]['args'][1] == baseline['playId'], reads
    assert reads['detail'][1]['cover'] == baseline['snapshotCover'], reads
    for lane in ('recommendations', 'songs'):
        assert reads[lane][1]['receipt'] and reads[lane][1]['receipt'] != reads[lane][0]['receipt'], reads
    mismatch = stages['mismatch_read_only']
    assert mismatch['actual'] and mismatch['playback'] == baseline, mismatch
    dom = mismatch['dom']
    assert dom['state'] == 'ready' and dom['plainCreator'] == '合成仅展示主播', dom
    assert dom['radioName'] == '合成仅展示播单' and dom['radio'], dom
    assert not any(dom[key] for key in ('creatorLink', 'radioOpenEnabled', 'radioSubscribeEnabled')), dom
    for lane in LANES:
        assert mismatch['reads'][lane][:2] == reads[lane], mismatch
        assert len(mismatch['reads'][lane]) == 3, mismatch
        assert mismatch['reads'][lane][2]['args'] == reads[lane][1]['args'], mismatch
        assert mismatch['reads'][lane][2]['outcome'] == 'success', mismatch
    third = mismatch['reads']['detail'][2]
    assert third['creatorId'] == '56' and third['radioId'] == '81', third
    assert done['voicePageReads'] == mismatch['voicePageReads'], done
    assert sorted((row['lane'], row['voiceId'], row['state']) for row in done['voicePageReads']) == [
        ('detail', '601', 'failure'), ('detail', '601', 'success'), ('detail', '601', 'success'),
        *[('recommendations', '601', 'success')] * 3, *[('songs', '601', 'success')] * 3,
    ], done
    return dict(actualWebView=True, heldSiblingResults=True, failedWithoutPartial=True,
                reopenedFresh=True, mismatchedTargetsReadOnly=True, unchangedPausedDecoder=True,
                nativeDecoder='libmpv/null', coverAnchored=True, coverBox=anchor,
                stages=TRIAD_STAGES, voicePageReads=done['voicePageReads'], realAccountWrites=0)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--triad', action='store_true', help='hold and fail the initial fresh detail, then reopen')
    args = parser.parse_args()
    if sys.platform not in ('linux', 'win32') or args.screenshots and sys.platform != 'linux':
        parser.error('use Linux X11 or Windows; screenshots require X11')
    binary = args.binary.resolve(strict=True)
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    script = 'voice_page_ui_probe.js' if args.triad else 'voice_detail_ui_probe.js'
    env.update(CUMULUS_PROOF_DIR=str(root / 'run'), CUMULUS_VOICE_DETAIL_PROOF='1',
               CUMULUS_UX_SCRIPT=str(Path(__file__).resolve().parents[2] / 'desktop/examples' / script))
    if args.triad:
        env['CUMULUS_VOICE_PAGE_PROOF'] = '1'
    if sys.platform == 'linux':
        env.update(LC_ALL='en_US.UTF-8', GDK_BACKEND='x11', TZ='Asia/Shanghai')
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve(strict=True)) + os.pathsep + env.get('PATH', '')
    for key, name in (('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')):
        p = root / 'os-profile' / name
        p.mkdir(parents=True, mode=0o700)
        env[key] = str(p)
    output = root / 'run' / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        status = launch(binary, env, log, output, args.screenshots)
    failure = output / 'failed.json'
    assert not failure.exists(), failure.read_text(encoding='utf-8') if failure.exists() else ''
    assert status == 0, (root / 'host.log').read_text(encoding='utf-8')
    done = read(output / 'complete.json')
    stages = {p.stem.split('_', 2)[2]: read(p) for p in sorted(output.glob('ux_*.json'))}
    if args.triad:
        report = verify_triad(stages, done)
        finish(args, root, output, stages, report)
        return
    assert done['ok'] and done['stageCount'] == len(stages) and tuple(stages) == STAGES, done
    assert done['realMusicRequests'] == done['realAccountWrites'] == 0
    assert all(stage['actual'] for stage in stages.values()), stages
    start = stages['card_placeholder']
    assert start['covers']['card'] == 'error' and start['state']['state'] == 'idle', start
    assert start['queue']['voices'] == [] and start['voiceDetailCalls'] == [], start
    played = stages['bar_placeholder']
    assert played['covers']['bar'] == 'error' and played['voiceDetailCalls'] == [], played
    expected_detail = {
        'id': '601', 'creator': {'id': '55', 'name': '合成主播'}, 'releaseTime': 1700049600000,
        'coverUrl': 'https://p1.music.126.net/obj/cumulus-palette/cover601.png', 'playCount': 123456,
        'radio': {'id': '80', 'name': '合成场景播客', 'subscriberCount': 0, 'ownedByViewer': False},
    }
    for name in STAGES[1:]:
        stage = stages[name]
        player, queue = stage['state'], stage['queue']
        assert player['state'] == 'playing' and player['voice'] == '601', stage
        assert player['playId'] == played['state']['playId'] and player['voiceDetail'] == expected_detail, stage
        assert queue == played['queue'] and [v[1] for v in queue['voices']] == ['601', '602'], stage
        assert player['position'] >= played['state']['position'], stage
    for name in ('voice_metadata', 'detail_reopened'):
        stage = stages[name]
        assert stage['metadata'] == {'creator': '合成主播', 'release': '2023-11-15', 'songCredits': False}, stage
        assert stage['covers']['detail'] == 'error', stage
        art = stage['artwork']
        assert art['width'] > 0 and abs(art['width'] - art['height']) <= 1, art
        assert abs(art['width'] - art['fittedSize']) <= 1, art
        assert {key: art[key] for key in ('radius', 'count', 'top', 'right', 'fontSize')} == {
            'radius': '8px', 'count': '12.3万', 'top': '23px', 'right': '20px', 'fontSize': '22px',
        }, art
    assert stages['creator_profile']['profile'] == '合成主播主页'
    assert stages['voice_metadata']['voiceDetailCalls'] == [], stages
    requests = done['voiceDetailCalls']
    assert [path for path, _ in requests] == [
        '/api/w/v1/user/detail/55', '/api/djradio/get/create/sub/count', '/api/user/playlist'], requests
    assert done['paletteRequests']['bodyBytes'] == 0
    assert len(done['paletteRequests']['urls']) == 1, done['paletteRequests']
    report = dict(actualWebView=True, nativeVoiceMetadata=True, creatorTarget='user:55',
                  squareCover=stages['voice_metadata']['artwork'],
                  imageFailure='probe CSP blocks image requests',
                  nativeDecoder='libmpv/null', stages=STAGES,
                  voiceDetailCalls=requests, realAccountWrites=0)
    finish(args, root, output, stages, report)


def finish(args, root, output, stages, report):
    if sys.platform == 'linux':
        locale = read(output / 'locale.json')
        assert locale['numeric'] == 'C' and all(
            locale[key].casefold().replace('-', '') == 'en_us.utf8' for key in ('characters', 'messages')), locale
    pictures = len(list(output.glob('ux_*.png')))
    assert not args.screenshots or pictures == len(stages), pictures
    report['screenshots'] = pictures
    (root / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    main()
