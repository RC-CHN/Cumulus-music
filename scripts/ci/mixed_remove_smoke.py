"""Trusted Linux WebView selection and read-only recovery of a mixed delete."""

import argparse
from contextlib import ExitStack
import ctypes
import json
import os
from pathlib import Path
import time

from queue_collection_smoke import isolated_profile
from queue_row_collection_smoke import QueueCollectInput
from vinyl_lyric_copy_smoke import require_xvfb
from window_manager import launch


STAGES = ('selection_ready', 'remove_menu', 'confirm', 'cancelled', 'uncertain', 'reopened', 'readback')


class RemoveInput(QueueCollectInput):
    def __init__(self):
        super().__init__()
        self.x11.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
        self.x11.XKeysymToKeycode.restype = ctypes.c_uint8
        self.xtest.XTestFakeKeyEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
        self.control = self.x11.XKeysymToKeycode(self.display, 0xffe3)  # XK_Control_L
        assert self.control

    def ctrl(self, pressed):
        assert self.xtest.XTestFakeKeyEvent(self.display, self.control, int(pressed), 0)
        self.x11.XSync(self.display, 0)

    def context(self):
        for pressed in (1, 0):
            assert self.xtest.XTestFakeButtonEvent(self.display, 3, pressed, 0)
        self.x11.XSync(self.display, 0)

    def at_capture(self, path, payload, pid):
        if path.stem == 'ux_01_selection_ready':
            online, local = (self.pointer_target(point, pid) for point in payload['points'])
            self.move(online)
            self.button(True)
            self.button(False)
            time.sleep(.06)
            self.ctrl(True)
            try:
                self.move(local)
                self.button(True)
                self.button(False)
            finally:
                self.ctrl(False)
            time.sleep(.06)
            self.context()
        elif path.stem == 'ux_04_cancelled':
            self.move(self.pointer_target(payload['points'][0], pid))
            self.context()


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    args = parser.parse_args()
    require_xvfb()
    assert os.environ.get('CUMULUS_TEST_WINDOW_MANAGER') == 'openbox'
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    run = root / 'run'
    env = {key: value for key, value in os.environ.items() if not key.upper().startswith(('CUMULUS_', 'WEBVIEW2_'))}
    isolated_profile(env, root)
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/mixed_remove_ui_probe.js'
    script = root / source.name
    script.write_bytes(source.read_bytes())
    env.update(CUMULUS_PROOF_DIR=str(run), CUMULUS_MIXED_REMOVE_PROOF='1',
               CUMULUS_MIXED_REORDER_SCRIPT=str(script))
    with ExitStack() as stack:
        driver = RemoveInput()
        stack.callback(driver.close)
        log = stack.enter_context((root / 'host.log').open('x', encoding='utf-8'))
        code = launch(args.binary.resolve(strict=True), env, log, run, True, before_ack=driver.at_capture)
    failed = run / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8')
    assert code == 0, f'mixed removal WebView exited {code}'
    paths = sorted(run.glob('ux_*.json'))
    assert [path.stem for path in paths] == [f'ux_{i:02}_{label}' for i, label in enumerate(STAGES, 1)]
    assert all(path.with_suffix('.png').is_file() for path in paths)
    rows = {label: read(path) for label, path in zip(STAGES, paths, strict=True)}
    complete = read(run / 'complete.json')
    assert complete['ok'] and complete['gestures'] == [True, True]
    local = complete['localId']
    native = rows['selection_ready']['state']
    assert native['state'] == 'paused' and native['track']['local'] and native['track']['id'] == local
    write = {'pid': '11', 'op': 'del', 'trackIds': '["701"]'}
    for label, sample in rows.items():
        proof = sample['nativeReceipt']
        assert proof['unexpected'] == 0 and proof['membership']['remoteMutations'] == 0
        assert proof['membership']['originalUnchanged']
        assert sample['queue'] == [local, '701', '702'], label
        assert all(sample['state'][key] == native[key] for key in ('state', 'playId', 'entryId', 'position', 'queueRevision'))
        if label in STAGES[:4]:
            assert proof['writes'] == [] and proof['onlineOrder'] == ['701', '702'], label
            assert proof['journal']['phase'] is None and sample['receipt'] is None
        else:
            assert proof['writes'] == [write] and proof['onlineOrder'] == ['702'], label
    pending = rows['uncertain']['nativeReceipt']
    reopened = rows['reopened']['nativeReceipt']
    settled = rows['readback']['nativeReceipt']
    assert rows['uncertain']['rows'] == [local, '701', '702']
    assert rows['reopened']['rows'] == [local, '702']
    assert rows['readback']['rows'] == ['702']
    assert pending['journal']['phase'] == reopened['journal']['phase'] == 'uncertain'
    assert pending['journal']['operation'] == reopened['journal']['operation'] == settled['journal']['operation']
    assert pending['membership']['localMembers'] == reopened['membership']['localMembers'] == 1
    assert settled['membership']['localMembers'] == 0
    assert settled['membership']['generation'] == pending['membership']['generation'] + 1
    assert settled['detailReads'] > reopened['detailReads'] > pending['detailReads']
    assert settled['journal']['phase'] == 'confirmed' and settled['journal']['evidence'] == 'changed_readback'
    assert complete['nativeReceipt'] == settled
    locale = read(run / 'native-startup.json')
    assert locale['numericLocale'] == 'C' and locale['messageLocale'] not in ('C', 'POSIX')
    report = dict(ok=True, actualLinuxWebView=True, trustedContextClicks=2, states=len(STAGES),
                  screenshots=len(STAGES), writes=[write], fileAndPausedQueueRetained=True,
                  readbackEvidence='changed_readback', realMusicRequests=0, realAccountWrites=0)
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
