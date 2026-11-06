"""Exercise native program descriptions and their real vinyl WebView card."""
import argparse
from contextlib import ExitStack
import json
import os
from pathlib import Path
import sys

from podcast_scene_home_smoke import CHECK, DETAIL, DIRECTORY, EXPERIMENT, MEDIA, RESUME, check_calls, read, requests
from queue_collection_smoke import isolated_profile
from vinyl_lyric_copy_smoke import require_xvfb
from vinyl_text_copy_smoke import TrustedVinylTextInput
from window_manager import launch

LINES = (
    '第一行合成节目简介', '第二行说明不改变播放', '第三行保留换行',
    '第四行仍然可见', '第五行仅展开后可见', '第六行结尾标记',
)
LONG = '\n'.join(LINES)
FOUR = '\n'.join(LINES[:4])
SHORT = '这是合成短简介'
BASE = ('playing', 'collapsed', 'expanded', 'recollapsed')
COPY = ('copy_drag_ready', 'copy_ready', 'copied')
FINAL = ('resumed', 'next_voice')
SENTINEL = 'cumulus sealed program description clipboard'


class DescriptionInput(TrustedVinylTextInput):
    def at_capture(self, path, payload, pid):
        label = path.stem.split('_', 2)[2]
        if label == 'copy_drag_ready':
            self.drag(path, payload, pid, 'description', SENTINEL)
        elif label == 'copy_ready':
            assert self.read_clipboard() == SENTINEL, 'drag alone must not change the clipboard'
            self.key('Control_L', 'c')
        elif label == 'copied':
            receipt = {'clipboard': self.read_clipboard(), 'richHtmlPresent': self.clipboard_has_html()}
            path.with_name('description-clipboard.json').write_text(
                json.dumps(receipt, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
            assert receipt == {'clipboard': FOUR + '\n', 'richHtmlPresent': False}, receipt
            self.readbacks['description'] = receipt


def primary_calls(calls):
    # A real actor can preauthorize the next row in the background. The
    # layout/clipboard actions are synchronous local operations; only those
    # exact, independently counted 602 prefetch reads may vary between them.
    def prefetch(path, body):
        return ((path == DETAIL and str(body['id']) == '602')
                or (path == CHECK and str(body['voiceId']) == '602')
                or (path == MEDIA and json.loads(body['ids']) in ([1602], ['1602'])))
    return [(path, body) for path, body in calls if not prefetch(path, body)]


def verify_trace(calls, final=False):
    check_calls(calls, directory_count=1, playback=True)
    assert [path for path, _ in calls[:2]] == [EXPERIMENT, DIRECTORY], calls
    details = [str(p['id']) for p in requests(calls, DETAIL)]
    assert len(details) == len(set(details)) and set(details) <= {'601', '602'}, calls
    resume = requests(calls, RESUME)
    assert len(resume) in ((1, 2) if final else (1,)) and all(
        body == {'radioId': '80'} for body in resume), calls
    permission = [str(p['voiceId']) for p in requests(calls, CHECK)]
    media = [str(json.loads(p['ids'])[0]) for p in requests(calls, MEDIA)]
    assert not final or (permission == ['601', '602'] and media == ['1601', '1602']), calls
    for voice in permission:
        permit = next(i for i, (path, p) in enumerate(calls) if path == CHECK and str(p['voiceId']) == voice)
        matches = [i for i, (path, p) in enumerate(calls)
                   if path == MEDIA and str(json.loads(p['ids'])[0]) == str(1000 + int(voice))]
        assert not matches or permit < matches[0], calls


def check_card(stage, text, lines, toggle):
    if not text:
        assert stage['layout'] is None, stage
        return
    layout = stage['layout']
    assert layout['text'] == text and layout['selectable'] and layout['heading'] == '简介', layout
    assert layout['lineHeight'] == 26 and abs(layout['height'] - 26 * lines) < .5, layout
    if toggle is None:
        assert layout['toggle'] is None, layout
    else:
        expected, focused = toggle
        assert layout['toggle']['name'] == ('收起' if expected else '展开'), layout
        assert layout['toggle']['expanded'] == str(expected).lower() and layout['toggle']['ownsBody'], layout
        if focused is not None:
            assert layout['toggle']['focused'] is focused, layout


def run(case, root, binary, dist, screenshots, clipboard):
    root.mkdir()
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/voice_description_ui_probe.js'
    script = root / 'probe.js'
    script.write_text('window.__voiceDescriptionProof = ' + json.dumps({'case': case, 'clipboard': clipboard})
                      + ';\n' + source.read_text(encoding='utf-8'), encoding='utf-8')
    env.update(CUMULUS_PROOF_DIR=str(root / 'run'), CUMULUS_VOICE_DETAIL_PROOF='1',
               CUMULUS_VOICE_DESCRIPTION_PROOF=case, CUMULUS_UX_SCRIPT=str(script),
               CUMULUS_PROOF_FRONTEND_DIST=str(dist), LC_ALL='en_US.UTF-8', GDK_BACKEND='x11')
    isolated_profile(env, root)
    output = root / 'run' / 'first'
    with ExitStack() as stack:
        driver = DescriptionInput() if clipboard else None
        if driver:
            stack.callback(driver.close)
        log = stack.enter_context((root / 'host.log').open('x', encoding='utf-8'))
        status = launch(binary, env, log, output, screenshots, before_ack=driver.at_capture if driver else None)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    host = (root / 'host.log').read_text(encoding='utf-8')
    assert status == 0 and 'panicked at' not in host, host
    complete = read(output / 'complete.json')
    stages = {p.stem.split('_', 2)[2]: read(p) for p in sorted(output.glob('ux_*.json'))}
    names = BASE + (COPY if clipboard else ()) + FINAL
    assert tuple(stages) == names and complete['ok'] and complete['stageCount'] == len(names), complete
    assert complete['case'] == case and complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    assert complete['physicalAudioVerified'] is False and complete['voiceDetailCalls'] == []
    verify_trace(complete['calls'], final=True)
    first = stages['playing']
    paused = stages['collapsed']
    start = first['state']
    baseline = paused['state']
    next_stage = stages['next_voice']
    for label, stage in stages.items():
        assert stage['actual'] and stage['case'] == case and stage['voiceDetailCalls'] == [], stage
        native = stage['state']
        changed = label == 'next_voice'
        playing = label in ('playing', 'resumed', 'next_voice')
        voice = '602' if changed else '601'
        assert stage['title'].endswith(voice), stage
        assert native['state'] == ('playing' if playing else 'paused') and not native['error']
        assert not native['seeking'] and native['playId'] and native['entryId'], native
        assert native['voice'] == native['voiceDetail']['id'] == voice, native
        expected_description = (SHORT if case == 'short' else None) if changed else LONG
        assert native['voiceDetail'].get('description') == expected_description, native
        assert stage['queue']['entries'] == first['queue']['entries'], stage
        assert [e[1] for e in stage['queue']['entries']] == ['601', '602']
        assert stage['queue']['current'] == native['entryId'] == stage['queue']['entries'][int(changed)][0]
        assert (native['playId'] != start['playId']) == changed, native
        assert (native['entryId'] != start['entryId']) == changed, native
        if not changed:
            assert stage['queue'] == first['queue'] and native['queueRevision'] == start['queueRevision'], stage
        if not playing:
            assert native['position'] == baseline['position'], native
        verify_trace(stage['calls'], final=changed)
        if not changed:
            assert primary_calls(stage['calls']) == primary_calls(first['calls']), stage
    assert start['position'] > .2 and next_stage['state']['position'] > .2
    assert stages['resumed']['state']['position'] > baseline['position']
    for label in ('playing', 'collapsed', 'resumed'):
        check_card(stages[label], FOUR + '…', 4, (False, None))
    check_card(stages['expanded'], LONG, 6, (True, True))
    check_card(stages['recollapsed'], FOUR + '…', 4, (False, True))
    check_card(next_stage, SHORT if case == 'short' else None, 1, None)
    if clipboard:
        for label in COPY:
            check_card(stages[label], FOUR + '…', 4, (False, None))
        assert stages['copied']['copy'] == {'trusted': True, 'handled': True, 'hasClipboardData': True}
        assert stages['copied']['selection'] == {'text': FOUR, 'collapsed': False, 'bodyOnly': True}
        assert list(driver.readbacks) == ['description']
    locale = read(output / 'locale.json')
    assert locale['numeric'] == 'C' and all(
        locale[key].casefold().replace('-', '') == 'en_us.utf8' for key in ('characters', 'messages')), locale
    pictures = len(list(output.glob('ux_*.png')))
    assert pictures == (len(stages) if screenshots else 0), pictures
    return {'stages': list(stages), 'screenshots': pictures, 'calls': complete['calls'],
            'trustedClipboard': driver.readbacks if driver else None,
            'pausedPosition': baseline['position'], 'oldPlayId': baseline['playId'],
            'nextPlayId': next_stage['state']['playId']}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--case', choices=('short', 'missing'), action='append')
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--clipboard', action='store_true', help='use trusted X11/GTK input in the short case')
    args = parser.parse_args()
    cases = args.case or ['short', 'missing']
    if sys.platform != 'linux' or (args.clipboard and (not args.screenshots or 'short' not in cases)):
        parser.error('requires Linux X11; clipboard requires screenshots and the short case')
    if args.clipboard:
        require_xvfb()
    binary = args.binary.resolve(strict=True)
    dist = args.frontend_dist.resolve(strict=True)
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    reports = {case: run(case, root / case, binary, dist, args.screenshots, args.clipboard and case == 'short')
               for case in cases}
    result = {'ok': True, 'actualWebView': True, 'decoder': 'libmpv/null', 'realAccountWrites': 0,
              'transport': 'synthetic only', 'physicalAudioOutputVerified': False, 'cases': reports}
    (root / 'report.json').write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(result, ensure_ascii=False))


if __name__ == '__main__':
    main()
