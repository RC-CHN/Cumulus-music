"""Linux WebView: ordinary catalog picker with a mixed target and a lost ACK."""
import argparse
from contextlib import ExitStack
import json
import os
from pathlib import Path

from mixed_remove_smoke import RemoveInput, read
from queue_collection_smoke import isolated_profile
from vinyl_lyric_copy_smoke import require_xvfb
from window_manager import launch


STAGES = ('source', 'picker', 'uncertain', 'earlier', 'readback', 'target')


class AddInput(RemoveInput):
    def at_capture(self, path, payload, pid):
        if path.stem in ('ux_01_source', 'ux_04_earlier'):
            self.move(self.pointer_target(payload['point'], pid))
            self.context()


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
    env = {key: value for key, value in os.environ.items()
           if not key.upper().startswith(('CUMULUS_', 'WEBVIEW2_'))}
    isolated_profile(env, root)
    script = Path(__file__).resolve().parents[2] / 'desktop/examples/mixed_add_ui_probe.js'
    env.update(CUMULUS_PROOF_DIR=str(run), CUMULUS_MIXED_ADD_PROOF='1',
               CUMULUS_MIXED_REORDER_SCRIPT=str(script))
    with ExitStack() as stack:
        driver = AddInput()
        stack.callback(driver.close)
        log = stack.enter_context((root / 'host.log').open('x', encoding='utf-8'))
        code = launch(args.binary.resolve(strict=True), env, log, run, True,
                      before_ack=driver.at_capture)
    failed = run / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8')
    assert code == 0, f'mixed addition WebView exited {code}'
    paths = sorted(run.glob('ux_*.json'))
    assert [path.stem for path in paths] == [
        f'ux_{i:02}_{label}' for i, label in enumerate(STAGES, 1)
    ]
    assert all(path.with_suffix('.png').is_file() for path in paths)
    states = [read(path) for path in paths]
    complete = read(run / 'complete.json')
    assert complete['ok'] and complete['context'] == [['703', True], ['704', True]]
    local = complete['localId']
    base = states[0]['state']
    assert base['state'] == 'paused' and base['track']['id'] == local
    request = {'pid': '11', 'op': 'add', 'trackIds': '["703"]'}
    for i, state in enumerate(states):
        native = state['nativeReceipt']
        assert state['queue'] == [local, '701', '702']
        assert all(state['state'][key] == base[key]
                   for key in ('state', 'playId', 'entryId', 'position', 'queueRevision'))
        assert native['membership']['originalUnchanged'] and native['unexpected'] == 0
        assert native['writes'] == ([] if i < 2 else [request])
        assert native['membership']['localMembers'] == 1
        assert native['membership']['localPosition'] == (0 if i < 4 else 1)
        assert state['rows'] == (['703', '704'] if i < 5 else ['703', local, '701', '702'])
    before, _, uncertain, reopened, readback, done = [row['nativeReceipt'] for row in states]
    assert uncertain['journal']['phase'] == reopened['journal']['phase'] == 'uncertain'
    assert uncertain['journal']['operation'] == readback['journal']['operation']
    assert uncertain['detailReads'] == reopened['detailReads']
    assert readback['detailReads'] == reopened['detailReads'] + 1
    assert readback['journal']['phase'] == 'confirmed'
    assert readback['journal']['evidence'] == 'changed_readback'
    assert readback['membership']['generation'] == before['membership']['generation'] + 1
    assert done['onlineOrder'] == ['703', '701', '702']
    locale = read(run / 'native-startup.json')
    assert locale['numericLocale'] == 'C' and locale['messageLocale'] not in ('C', 'POSIX')
    report = dict(ok=True, actualLinuxWebView=True, states=len(states), screenshots=len(states),
                  trustedContextClicks=2, writes=[request], fileAndPausedQueueRetained=True,
                  secondSongNotSubmitted=True, readbackEvidence='changed_readback',
                  realMusicRequests=0, realAccountWrites=0)
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
