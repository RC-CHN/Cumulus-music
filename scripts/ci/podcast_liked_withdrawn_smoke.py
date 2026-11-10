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
    names = ['ux_01_liked_rows', 'ux_02_withdrawn', 'ux_03_menu', 'ux_04_single_play', 'ux_05_next',
             'ux_06_share', 'ux_07_download', 'ux_08_valid_native', 'ux_09_missing_retained',
             'ux_10_liked_control', 'ux_11_recent_control', 'ux_12_recent_treatment']
    assert sorted(path.stem for path in output.glob('ux_*.json')) == names
    assert sorted(path.stem for path in output.glob('ux_*.png')) == names
    stages = [read(output / f'{name}.json') for name in names]
    assert all(stage['actual'] for stage in stages)
    assert stages[0]['ids'] == ['501', '502', '503', '504'] and stages[0]['height'] == 56
    assert stages[1]['notice'] == '该声音已下线不可收听' and stages[1]['queue'] == 0
    assert stages[2]['labels'] == ['播放', '下一首播放', '查看评论(54)', '下载', '分享...', '复制链接']
    assert stages[3]['voice'] == stages[7]['voice'] == stages[8]['voice'] == '504'
    assert stages[3]['queue'] == ['504'] and stages[4]['queue'] == ['504', '503'] and stages[4]['playIdRetained']
    assert stages[3]['indexPauseRetained']
    assert stages[5]['link'] == 'https://music.163.com/dj?id=503'
    assert stages[6]['target'] == 503 and stages[6]['beforeCancel']
    assert stages[7]['queue'] == ['504'] and stages[7]['position'] > 0 and stages[8]['playIdRetained']
    assert stages[7]['toggleRetained']
    dimensions = lambda row: (row['height'], row['cover'], row['title'])
    assert dimensions(stages[0]) == dimensions(stages[11]) == (56, 36, 14)
    assert dimensions(stages[9]) == dimensions(stages[10]) == (64, 48, 16)
    assert stages[9]['moreRetained'] and stages[9]['keyboard'] and stages[11]['scrollRowRetained']
    assert all(0 < row['mounted'] < 30 for row in stages[10:12])
    complete = read(output / 'complete.json')
    assert complete['ok']
    calls = complete['calls']
    liked = '/api/content/my/liked/voice'
    assert [body for path, body in calls if path == liked] == [{'limit': 100, 'likeTime': 0}]
    assert [row for row in stages[9]['calls'] if row[0] == liked] == [row for row in stages[8]['calls'] if row[0] == liked]
    recent = '/api/social/my/recent/update/voice/list/v1'
    assert [body for path, body in calls if path == recent] == [{}]
    assert not any(path == '/api/dj/program/detail' for path, _ in stages[1]['calls'])
    detail = [body['id'] for path, body in calls if path == '/api/dj/program/detail']
    assert set(detail) == {'502', '503'} and detail[-1] == '502', detail
    assert [body for path, body in calls if path == '/api/dj/voices/download/check'] == [{'voiceIdsStr': ['503']}]
    assert [body for path, body in calls if path == '/api/dj/program/song/musics'] == [{'ids': ['503']}]
    assert not any(path == '/api/song/enhance/download/url/v1' for path, _ in calls)
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
