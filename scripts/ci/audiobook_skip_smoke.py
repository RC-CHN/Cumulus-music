"""Prove audiobook skip behavior in Linux WebKitGTK with real, null-output libmpv."""
import argparse
import json
import os
from pathlib import Path
import sys

from window_manager import launch


SWITCH = '/api/voice/alg/skip/switch/get'
INTERVALS = '/api/voice/alg/skip/info/get'
OFF = {'kind': 'close'}
MANUAL = {'kind': 'custom', 'start': 35, 'end': 25}
EDITED = {'kind': 'custom', 'start': 65, 'end': 25}
SMART = {'kind': 'smart'}
LONG = (
    'manual_draft', 'manual_saved', 'manual_intro', 'enabled_label',
    'manual_readback', 'live_edit_no_jump', 'paused_past_tail', 'tail_advanced',
    'smart_saved', 'smart_intro', 'off_saved', 'off_intro', 'ordinary',
)
SHORT = (
    'manual_draft', 'manual_saved', 'short_intro_ignored', 'short_paused_tail',
    'short_tail_ignored',
)


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def playback(stage, voice, minimum, maximum, duration, state='playing'):
    player = stage['player']
    assert player['voice'] == voice and player['media'] == str(1000 + int(voice)), player
    assert player['timeline'] == 'full' and player['duration'] == duration, player
    assert player['state'] == state and minimum <= player['position'] < maximum, player
    return player


def check_long(stages, complete):
    first = playback(stages['manual_intro'], '501', 35, 40, 220)
    assert first['available'] and stages['enabled_label']['label'] == '跳过片头尾（已开启）'
    readback = stages['manual_readback']
    assert (readback['start'], readback['endRail'], readback['localBookSetting']) == ('35', '65', MANUAL)
    live = stages['live_edit_no_jump']
    assert live['localBookSetting'] == EDITED
    assert playback(live, '501', 35, 61, 220)['playId'] == first['playId']
    pause = stages['paused_past_tail']
    assert playback(pause, '501', 197.28, 197.52, 220, 'paused')['playId'] == first['playId']
    assert pause['checkpoints'] == [] and all(route == SWITCH for route, _ in pause['reads'])
    tail = stages['tail_advanced']
    second = playback(tail, '502', 65, 70, 220)
    assert second['playId'] != first['playId']
    assert tail['checkpoints'] == [dict(programId=501, listenLocation=220000, isListened=True)]
    assert stages['smart_saved']['localBookSetting'] == SMART
    smart = stages['smart_intro']
    assert playback(smart, '501', 45, 50, 220)['playId'] != second['playId']
    assert [INTERVALS, {'programIdsStr': ['501']}] in smart['reads'], smart['reads']
    assert stages['off_saved']['localBookSetting'] == OFF
    assert playback(stages['off_intro'], '502', .15, 5, 220)['available']
    ordinary = stages['ordinary']
    assert not playback(ordinary, '601', .15, 5, 220)['available']
    assert ordinary['detailMenu'] is ordinary['footerMenu'] is False
    assert [args for path, args in complete['reads'] if path == SWITCH] == [{'radioId': 8}] * 4
    assert all(path in {SWITCH, INTERVALS} for path, _ in complete['reads'])
    assert complete['localBookSetting'] == OFF


def check_short(stages, complete):
    start = playback(stages['short_intro_ignored'], '501', .15, 5, 199)
    paused = playback(stages['short_paused_tail'], '501', 177.28, 177.52, 199, 'paused')
    passed = playback(stages['short_tail_ignored'], '501', 178.5, 190, 199)
    assert start['available'] and start['playId'] == paused['playId'] == passed['playId']
    assert complete['localBookSetting'] == MANUAL
    assert complete['reads'] == [[SWITCH, {'radioId': 8}]] and complete['checkpoints'] == []


def run_case(binary, root, name, base, script, capture):
    directory = root / name
    directory.mkdir()
    run = directory / 'run'
    env = {**base, 'CUMULUS_PROOF_DIR': str(run), 'CUMULUS_UX_SCRIPT': str(script),
           'CUMULUS_AUDIOBOOK_SKIP_CASE': name}
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR'):
        location = directory / key
        location.mkdir(mode=0o700)
        env[key] = str(location)
    output = run / 'first'
    host = directory / 'host.log'
    with host.open('x', encoding='utf-8') as log:
        status = launch(binary, env, log, output, capture)
    failed = output / 'failed.json'
    assert not failed.exists(), read(failed) if failed.exists() else None
    assert status == 0 and 'panicked at' not in host.read_text(encoding='utf-8'), host
    complete = read(output / 'complete.json')
    assert complete['ok'] and complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    rows = {path.stem.split('_', 2)[2]: read(path) for path in sorted(output.glob('ux_*.json'))}
    assert tuple(rows) == (LONG if name == 'long' else SHORT)
    assert len(rows) == complete['stageCount'] and all(row['actual'] for row in rows.values())
    assert (rows['manual_draft']['start'], rows['manual_draft']['endRail']) == ('35', '65')
    assert rows['manual_draft']['localBookSetting'] == OFF
    assert rows['manual_saved']['localBookSetting'] == MANUAL
    for row in (*rows.values(), complete):
        assert row['localOrdinarySetting'] == row['localOtherAccountSetting'] == OFF
    (check_long if name == 'long' else check_short)(rows, complete)
    locale = read(output / 'locale.json')
    assert locale['numeric'] == 'C'
    assert all(locale[key].casefold().replace('-', '') == 'en_us.utf8'
               for key in ('messages', 'characters')), locale
    screenshots = len(list(output.glob('ux_*.png')))
    assert not capture or screenshots == len(rows)
    return {**complete, 'states': list(rows), 'screenshots': screenshots}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--root', required=True, type=Path)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--case', choices=('long', 'short'))
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    if sys.platform != 'linux':
        parser.error('this synthetic GUI proof is currently Linux only')
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    base = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    base.update(LC_ALL='en_US.UTF-8', GDK_BACKEND='x11')
    if args.frontend_dist:
        base['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    script = Path(__file__).resolve().parents[2] / 'desktop/examples/audiobook_skip_ui_probe.js'
    binary = args.binary.resolve(strict=True)
    cases = (args.case,) if args.case else ('long', 'short')
    report = {'actualWebView': True, 'decoder': 'libmpv', 'audioOutput': 'null',
              'cases': {name: run_case(binary, root, name, base, script, args.screenshots)
                        for name in cases}}
    (root / 'report.json').write_text(json.dumps(report, indent=2, ensure_ascii=False) + '\n',
                                      encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    main()
