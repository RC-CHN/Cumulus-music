"""Trusted X11 reorder gestures over a sealed mixed playlist and real local audio."""
import argparse
from contextlib import ExitStack
import json
import os
from pathlib import Path
import time

from queue_collection_smoke import isolated_profile
from queue_row_collection_smoke import QueueCollectInput
from vinyl_lyric_copy_smoke import require_xvfb
from window_manager import launch


STAGES = ('outside_ready', 'outside_marker', 'outside_cancelled', 'local_ready', 'local_marker',
          'sending', 'acknowledged', 'online_ready', 'online_marker', 'uncertain', 'readback')


class ReorderInput(QueueCollectInput):
    def at_capture(self, path, payload, pid):
        label = path.stem.split('_', 2)[2]
        if label.endswith('_ready'):
            assert not self.down, label
            start, end = (self.pointer_target(payload['pointer'][key], pid) for key in ('from', 'to'))
            assert abs(start['y'] - end['y']) > 30, (start, end)
            self.move(start)
            self.button(True)
            time.sleep(.06)
            for step in range(1, 9):
                self.move({axis: round(start[axis] + (end[axis] - start[axis]) * step / 8)
                           for axis in ('x', 'y')})
                time.sleep(.025)
            path.with_suffix('.pointer').write_text(json.dumps({'from': start, 'to': end}) + '\n',
                                                    encoding='utf-8')
        elif label.endswith('_marker'):
            assert self.down, label
            if label == 'outside_marker':
                outside = self.pointer_target(payload['outside'], pid)
                self.move(outside)
                time.sleep(.05)
                path.with_suffix('.pointer').write_text(json.dumps({'outside': outside}) + '\n',
                                                        encoding='utf-8')
            self.button(False)


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    args = parser.parse_args()
    require_xvfb()
    assert os.environ.get('CUMULUS_TEST_WINDOW_MANAGER') == 'openbox', 'use the isolated Openbox launcher'
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    run = root / 'run'
    env = {key: value for key, value in os.environ.items()
           if not key.upper().startswith(('CUMULUS_', 'WEBVIEW2_'))}
    isolated_profile(env, root)
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/mixed_reorder_ui_probe.js'
    script = root / source.name
    script.write_bytes(source.read_bytes())
    env.update(CUMULUS_PROOF_DIR=str(run), CUMULUS_MIXED_REORDER_PROOF='1',
               CUMULUS_MIXED_REORDER_SCRIPT=str(script))
    with ExitStack() as stack:
        driver = ReorderInput()
        stack.callback(driver.close)
        log = stack.enter_context((root / 'host.log').open('x', encoding='utf-8'))
        code = launch(args.binary.resolve(strict=True), env, log, run, True, before_ack=driver.at_capture)
    failed = run / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8')
    assert code == 0, f'mixed reorder WebView exited {code}'
    paths = sorted(run.glob('ux_*.json'))
    assert [path.stem for path in paths] == [f'ux_{i:02}_{label}' for i, label in enumerate(STAGES, 1)]
    assert all(path.with_suffix('.png').is_file() for path in paths)
    states = {label: read(path) for label, path in zip(STAGES, paths, strict=True)}
    complete = read(run / 'complete.json')
    assert complete['ok']
    local = complete['localId']
    initial, after_local, desired = [local, '701', '702'], ['701', local, '702'], ['702', '701', local]
    trusted = [{'event': event, 'trusted': True} for event in ('pointerdown', 'pointerup') * 3]
    assert complete['gestures'] == trusted
    baseline = states['outside_ready']['native']
    assert baseline['state'] == 'paused' and baseline['track']['local'] and baseline['position'] > .08
    for label, row in states.items():
        receipt = row['nativeReceipt']
        member = receipt['membership']
        assert member['localId'] == local and member['originalUnchanged'] and member['localMembers'] == 1
        assert member['remoteMutations'] == 0 and receipt['unexpected'] == 0, label
        assert receipt['audioChecks'] == ['701'], label
        assert row['queue'] == initial and not row['native']['error'], label
        assert all(row['native'][key] == baseline[key]
                   for key in ('state', 'position', 'playId', 'entryId', 'queueRevision')), label

    first_write = {'pid': '11', 'op': 'update', 'trackIds': '["701","702"]'}
    second_write = {'pid': '11', 'op': 'update', 'trackIds': '["702","701"]'}
    for label in STAGES[:5]:
        sample = states[label]
        receipt = sample['nativeReceipt']
        assert sample['rows'] == initial and receipt['writes'] == [], label
        assert receipt['membership']['generation'] == 1 and receipt['membership']['localPosition'] == 0
        assert receipt['onlineOrder'] == ['701', '702'] and receipt['journal']['phase'] is None
    before_send = states['local_marker']['nativeReceipt']
    sending = states['sending']['nativeReceipt']
    assert sending['writes'] == [first_write] and sending['onlineOrder'] == ['701', '702']
    assert sending['detailReads'] == before_send['detailReads'] + 1
    assert sending['membership']['generation'] == 1 and sending['membership']['localPosition'] == 0
    assert sending['journal']['phase'] == 'sending' and sending['journal']['desiredPosition'] == 1
    assert sending['journal']['beforeOnline'] == sending['journal']['desiredOnline'] == ['701', '702']
    assert states['sending']['receipt']['phase'] == 'sending'
    for label in ('acknowledged', 'online_ready', 'online_marker'):
        sample = states[label]
        receipt = sample['nativeReceipt']
        assert sample['rows'] == after_local and receipt['writes'] == [first_write], label
        assert receipt['membership']['generation'] == 2 and receipt['membership']['localPosition'] == 1
        assert receipt['journal']['phase'] == 'confirmed' and receipt['journal']['evidence'] == 'acknowledged'
        assert receipt['journal']['operation'] == sending['journal']['operation']
    assert states['acknowledged']['receipt']['evidence'] == 'acknowledged'
    uncertain, readback = (states[label]['nativeReceipt'] for label in ('uncertain', 'readback'))
    assert uncertain['writes'] == readback['writes'] == [first_write, second_write]
    assert uncertain['onlineOrder'] == readback['onlineOrder'] == ['702', '701']
    assert uncertain['detailReads'] == states['online_marker']['nativeReceipt']['detailReads'] + 1
    assert uncertain['journal']['phase'] == 'uncertain' and uncertain['journal']['evidence'] is None
    assert uncertain['journal']['operation'] != sending['journal']['operation']
    assert uncertain['journal']['beforeOnline'] == ['701', '702']
    assert uncertain['journal']['desiredOnline'] == ['702', '701'] and uncertain['journal']['desiredPosition'] == 2
    assert uncertain['membership']['generation'] == 2 and uncertain['membership']['localPosition'] == 1
    assert states['uncertain']['receipt']['phase'] == 'uncertain'
    assert states['uncertain']['rows'].index(local) == 1
    assert readback['detailReads'] >= uncertain['detailReads'] + 1
    assert readback['journal']['phase'] == 'confirmed' and readback['journal']['evidence'] == 'changed_readback'
    assert readback['journal']['operation'] == uncertain['journal']['operation']
    assert readback['membership']['generation'] == 3 and readback['membership']['localPosition'] == 2
    assert states['readback']['receipt']['evidence'] == 'changed_readback'
    assert states['readback']['rows'] == desired and complete['nativeReceipt'] == readback
    locale = read(run / 'native-startup.json')
    assert locale['numericLocale'] == 'C' and locale['messageLocale'] not in ('C', 'POSIX')
    result = dict(ok=True, actualLinuxWebView=True, trustedPointer=True, states=len(STAGES),
                  screenshots=len(STAGES), originalFilePreserved=True, currentQueuePreserved=True,
                  syntheticWrites=[first_write, second_write], readbackEvidence='changed_readback',
                  realMusicRequests=0, realAccountWrites=0)
    (root / 'report.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(result))


if __name__ == '__main__':
    main()
