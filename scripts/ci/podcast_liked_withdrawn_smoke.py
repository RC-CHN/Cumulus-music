"""Run the liked-voice availability cases in the real, isolated Linux WebView."""

import argparse
import json
import os
from pathlib import Path

from queue_collection_smoke import isolated_profile
from vinyl_lyric_copy_smoke import require_xvfb
from window_manager import launch


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    args = parser.parse_args()
    require_xvfb()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    run = root / 'run'
    output = run / 'first'
    env = {key: value for key, value in os.environ.items() if not key.upper().startswith(('CUMULUS_', 'WEBVIEW2_'))}
    isolated_profile(env, root)
    script = Path(__file__).resolve().parents[2] / 'desktop/examples/podcast_liked_withdrawn_ui_probe.js'
    env.update(CUMULUS_PROOF_DIR=str(run), CUMULUS_PROOF_CAPTURE_ACK='1',
               CUMULUS_PODCAST_LIKED_WITHDRAWN_PROOF='1', CUMULUS_UX_SCRIPT=str(script))
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(strict=True), env, log, output, True)
    failed = output / 'failed.json'
    assert not failed.exists(), read(failed)
    assert code == 0, f'WebView exited {code}; see {root / "host.log"}'
    names = ['ux_01_liked_rows', 'ux_02_withdrawn', 'ux_03_valid_native', 'ux_04_missing_retained']
    assert sorted(path.stem for path in output.glob('ux_*.json')) == names
    assert sorted(path.stem for path in output.glob('ux_*.png')) == names
    stages = [read(output / f'{name}.json') for name in names]
    assert all(stage['actual'] for stage in stages)
    assert stages[0]['ids'] == ['501', '502', '503', '504']
    assert stages[1]['notice'] == '该声音已下线不可收听' and stages[1]['queue'] == 0
    assert stages[2]['voice'] == stages[3]['voice'] == '504'
    assert stages[2]['queue'] == ['503', '504'] and stages[2]['position'] > 0
    assert stages[3]['playIdRetained']
    complete = read(output / 'complete.json')
    assert complete['ok']
    calls = complete['calls']
    assert not any(path == '/api/dj/program/detail' for path, _ in stages[1]['calls'])
    assert [body['id'] for path, body in calls if path == '/api/dj/program/detail'] == ['502', '503', '504', '502']
    privileges = [body['voiceId'] for path, body in calls if path == '/api/dj/play/url/check']
    assert '504' in privileges and set(privileges) <= {'503', '504'}
    urls = [str(id_) for path, body in calls if path == '/api/song/enhance/player/url/v1' for id_ in json.loads(body['ids'])]
    assert '1504' in urls and set(urls) <= {'1503', '1504'}
    locale = read(output / 'locale.json')
    assert locale['numeric'] == 'C' and all(locale[key].casefold().replace('-', '') == 'en_us.utf8'
                                           for key in ('messages', 'characters')), locale
    result = {'ok': True, 'stages': len(stages), 'screenshots': len(names), 'privileges': privileges,
              'mediaIds': urls, 'actualWebView': True, 'realAccountWrites': 0}
    (root / 'report.json').write_text(json.dumps(result, ensure_ascii=False) + '\n', encoding='utf-8')
    print(json.dumps(result, ensure_ascii=False))


if __name__ == '__main__':
    main()
