"""Verify the current podcast resume setting in the native WebView with synthetic PC EAPI."""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import struct
import sys

from search_signed_trace import ACCOUNT, ASSIGNMENT, assignment_arguments
from window_manager import launch


STAGES = ('before_cohort', 'opening', 'cancelled', 'disabled', 'lost_ack', 'reconciled', 'retired')
READ = '/api/dj/user/setting/resumeplay/get'
WRITE = '/api/dj/user/setting/resumeplay/update'
RADIO = {'radioId': '80'}
SUCCESS = 'success'


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def signed_calls(receipt):
    trace = receipt['voiceFreshActionTrace']
    assert trace['transport'] == 'MusicHttpClient', trace
    assert trace['syntheticOnly'] is True and trace['sameProfileAccountRestore'] is True, trace
    return [(call['path'], call['args'], call['outcome']) for call in trace['signedCalls']]


def verify_wire(stages, complete, full_assignment):
    account = (ACCOUNT, {}, SUCCESS)
    assignment = (ASSIGNMENT, full_assignment, SUCCESS)
    get = (READ, RADIO, SUCCESS)
    disable = (WRITE, {**RADIO, 'resumePlayValue': '0'}, SUCCESS)
    enable = (WRITE, {**RADIO, 'resumePlayValue': '1'}, 'lost_ack')
    before = [account, get]
    opening = [*before, assignment]
    disabled = [*opening, get, disable, get, get]
    lost = [*disabled, get, enable]
    reconciled = [*lost, get, get]
    retired = [*reconciled, assignment]
    for name, expected in (
        ('before_cohort', before), ('opening', opening), ('cancelled', opening),
        ('disabled', disabled), ('lost_ack', lost),
        ('reconciled', reconciled), ('retired', retired),
    ):
        actual = signed_calls(stages[name])
        assert actual == expected, (name, actual, expected)
    assert signed_calls(complete) == retired, complete
    assert complete['voiceFreshActionTrace']['resumeEnabled'] is True, complete
    return complete['voiceFreshActionTrace']['signedCalls']


def verify_dom(stages):
    for name, stage in stages.items():
        dom = stage['dom']
        assert dom['trigger'] is (name not in ('before_cohort', 'retired')), (name, dom)
        assert dom['dialog'] is (name in ('opening', 'lost_ack')), (name, dom)
        assert dom['readback'] is (name == 'lost_ack'), (name, dom)
        assert dom['staleRejected'] is (name == 'retired'), (name, dom)
        assert dom['cohort'] == {'before_cohort': None, 'retired': 'c'}.get(name, 't1'), (name, dom)
        assert dom['menu'] == {
            'disabled': '当前播客断点续播 (关)', 'reconciled': '当前播客断点续播 (开)',
        }.get(name), (name, dom)
        assert dom['notice'] == ('未能确认断点续播设置' if name == 'lost_ack' else None), (name, dom)
        if name != 'lost_ack':
            assert dom['selected'] == ('on' if name == 'opening' else None), (name, dom)
        expected_focus = {
            'opening': 'on', 'cancelled': 'trigger', 'disabled': 'menu', 'lost_ack': 'readback',
            'reconciled': 'menu', 'retired': 'transport',
        }.get(name)
        if expected_focus:
            assert dom['focus'] == expected_focus, (name, dom)


def verify_playback(stages):
    baseline = stages['before_cohort']['playback']
    assert baseline['state'] == 'paused' and baseline['voice'] == '601', baseline
    assert baseline['playId'] and baseline['entryId'] and baseline['seeking'] is False, baseline
    assert not baseline.get('error'), baseline
    position = baseline['position']
    assert type(position) in (int, float) and math.isfinite(position) and position >= 0, baseline
    queue = baseline['queue']
    assert queue['revision'] and queue['current'] == baseline['entryId'], queue
    first, second = queue['entries']
    assert first == [baseline['entryId'], '601', '1601'], queue
    assert second[0] and second[0] != first[0] and second[1:] == ['602', '1602'], queue
    for name, stage in stages.items():
        assert stage['actual'] is True and stage['playback'] == baseline, (name, stage)
    return baseline


def screenshots(output, capture_names, enabled):
    pictures = sorted(output.glob('ux_*.png'))
    markers = sorted(output.glob('ux_*.captured'))
    expected = capture_names if enabled else []
    assert [path.stem for path in pictures] == [path.stem for path in markers] == expected
    evidence = {}
    for path, marker in zip(pictures, markers, strict=True):
        assert marker.read_text(encoding='utf-8') == 'captured\n', marker
        with path.open('rb') as image:
            header = image.read(24)
            assert len(header) == 24 and header[:16] == b'\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR', path
            width, height = struct.unpack('>II', header[16:])
            assert width > 0 and height > 0, path
            image.seek(-12, os.SEEK_END)
            assert image.read() == b'\x00\x00\x00\x00IEND\xaeB`\x82', path
            image.seek(0)
            digest = hashlib.file_digest(image, 'sha256').hexdigest()
        evidence[path.name] = {'sha256': digest, 'width': width, 'height': height}
    return evidence


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--root', required=True, type=Path)
    parser.add_argument('--frontend-dist', type=Path, help='override the probe\'s embedded frontend')
    parser.add_argument('--runtime', type=Path, help='Windows packaged native directory containing libmpv-2.dll')
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    if sys.platform not in ('linux', 'win32') or args.screenshots and sys.platform != 'linux':
        parser.error('use Linux X11 or Windows; screenshots require Linux X11')
    if sys.platform == 'win32' and (not args.runtime or not (args.runtime / 'libmpv-2.dll').is_file()):
        parser.error('Windows requires --runtime with the packaged libmpv-2.dll')
    binary = args.binary.resolve(strict=True)
    root = args.root.resolve()
    env = {key: value for key, value in os.environ.items() if not key.upper().startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root / 'run'), CUMULUS_PLAYER_PODCAST_RESUME_PROOF='1',
               CUMULUS_PODCAST_SCENE_HOME_PROOF='manual')
    if sys.platform == 'linux':
        env.update(LC_ALL='en_US.UTF-8', GDK_BACKEND='x11')
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve(strict=True)) + os.pathsep + env.get('PATH', '')
    env['CUMULUS_UX_SCRIPT'] = str(Path(__file__).resolve().parents[2]
                                   / 'desktop/examples/player_podcast_resume_ui_probe.js')
    if args.frontend_dist:
        dist = args.frontend_dist.resolve(strict=True)
        if not (dist / 'index.html').is_file():
            parser.error('--frontend-dist must contain index.html')
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(dist)
    root.mkdir(parents=True, exist_ok=False)
    for key, name in (('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')):
        location = root / 'os-profile' / name
        location.mkdir(parents=True, mode=0o700)
        env[key] = str(location)
    output = root / 'run' / 'first'
    host = root / 'host.log'
    with host.open('x', encoding='utf-8') as log:
        status = launch(binary, env, log, output, args.screenshots)
    failed = output / 'failed.json'
    assert not failed.exists(), read(failed) if failed.exists() else None
    assert status == 0 and 'panicked at' not in host.read_text(encoding='utf-8'), host
    complete = read(output / 'complete.json')
    captures = sorted(output.glob('ux_*.json'))
    assert [path.stem for path in captures] == [f'ux_{i:02}_{name}' for i, name in enumerate(STAGES, 1)]
    stages = {name: read(path) for name, path in zip(STAGES, captures, strict=True)}
    assert complete['ok'] is True and complete['stageCount'] == len(STAGES), complete
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0, complete
    playback = verify_playback(stages)
    verify_dom(stages)
    signed = verify_wire(stages, complete, assignment_arguments(Path(__file__).resolve().parents[2]))
    locale = None
    if sys.platform == 'linux':
        locale = read(output / 'locale.json')
        assert locale['numeric'] == 'C' and all(locale[key].casefold().replace('-', '') == 'en_us.utf8'
                                                for key in ('characters', 'messages')), locale
    pictures = screenshots(output, [path.stem for path in captures], args.screenshots)
    runtime = 'WebKitGTK' if sys.platform == 'linux' else 'WebView2'
    report = dict(ok=True, actualWebView=True, webViewRuntime=runtime, stages=list(STAGES),
                  stageCount=len(STAGES), playback=playback, signedCalls=signed, locale=locale,
                  screenshots=len(pictures), screenshotEvidence=pictures,
                  realMusicRequests=0, realAccountWrites=0)
    path = root / 'report.json'
    path.write_text(json.dumps(report, indent=2, ensure_ascii=False) + '\n', encoding='utf-8')
    print(json.dumps(dict(ok=True, webViewRuntime=runtime, stageCount=len(STAGES),
                          signedCalls=len(signed), screenshots=len(pictures), report=str(path))))


if __name__ == '__main__':
    main()
