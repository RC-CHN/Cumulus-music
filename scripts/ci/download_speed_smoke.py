"""Check the real GTK WebView download rate against the native scheduler and published FLAC."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import socket
import sqlite3
import sys

from motion_smoke import launch

STAGES = ('open', 'flowing', 'stalled', 'pausing', 'paused', 'resumed', 'completed')
SIZE = 187478
FLAC_SHA = 'e2fe2ad3ecd24483db4ad23cade06bc2403f688dc13304f5c2ed8933486d0247'


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def digest(path):
    with path.open('rb') as file:
        return hashlib.file_digest(file, 'sha256').hexdigest()


def frontend_digest(dist):
    manifest = hashlib.sha256()
    for path in sorted(path for path in dist.rglob('*') if path.is_file()):
        manifest.update(path.relative_to(dist).as_posix().encode() + b'\0')
        manifest.update(bytes.fromhex(digest(path)))
    return manifest.hexdigest()


def verify(root, screenshots):
    out = root / 'first'
    complete = read(out / 'complete.json')
    assert complete['ok'] and complete['liveDownloadSpeedUi']
    assert complete['stageCount'] == len(STAGES)
    expected = [f'ux_{i:02d}_{label}' for i, label in enumerate(STAGES, 1)]
    assert sorted(path.stem for path in out.glob('ux_*.json')) == expected
    assert sorted(path.stem for path in out.glob('ux_*.png')) == (expected if screenshots else [])
    values = [read(out / f'{stage}.json') for stage in expected]
    assert all(state['visibleRow'] and state['id'] == complete['taskId'] for state in values)
    assert [state['status'] for state in values] == ['downloading'] * 4 + ['paused', 'downloading', 'complete']
    assert [state['phase'] for state in values] == ['transfer'] * 6 + ['complete']
    assert [state['activeCount'] for state in values] == [1] * 6 + [0]
    assert [state['completedCount'] for state in values] == [0] * 6 + [1]
    assert all(state['total'] == SIZE for state in values)
    assert [state['rate'] for i, state in enumerate(values) if i != 1] == [0, 0, None, None, 0, None]
    assert type(values[1]['rate']) is int and values[1]['rate'] > 0
    assert [state['label'] for i, state in enumerate(values) if i != 1] == [
        '0 B/s', '0 B/s', '正在下载', '已暂停', '0 B/s', None,
    ]
    assert re.fullmatch(r'[1-9][0-9]*(?:\.[0-9])? (?:B|KB|MB)/s', values[1]['label'])

    reports = [state['downloadSpeedFixture'] for state in values]
    assert all(report['metadataReads'] == 1 and report['rejected'] == 0 for report in reports)
    assert [report['authorizations'] for report in reports] == [1] * 5 + [2, 2]
    assert [len(report['opens']) for report in reports] == [1] * 5 + [2, 2]
    assert [report['dropped'] for report in reports] == [[False, False]] * 4 + [
        [True, False], [True, False], [True, True],
    ]
    assert [report['pauseRequested'] for report in reports[:4]] == [False, False, False, True]
    assert reports[0]['delivered'] == [0, 0]
    assert 0 < reports[1]['delivered'][0] <= reports[2]['delivered'][0] < SIZE
    assert reports[2]['delivered'] == reports[3]['delivered']
    accepted = values[4]['bytes']
    assert accepted == values[5]['bytes'] == reports[2]['delivered'][0]
    wake = reports[4]['pauseWakeBytes']
    assert wake > 0 and reports[4]['delivered'] == [accepted + wake, 0]
    assert reports[5]['delivered'] == [accepted + wake, 0]
    assert reports[6]['delivered'] == [accepted + wake, SIZE - accepted]
    assert reports[6]['opens'] == [
        {'offset': 0, 'status': 200, 'validator': False},
        {'offset': accepted, 'status': 206, 'validator': False},
    ]
    assert complete['downloadSpeedFixture'] == reports[6]
    assert values[6]['progress'] == 1 and values[6]['bytes'] == SIZE

    with sqlite3.connect((root / 'state/core.sqlite3').as_uri() + '?mode=ro', uri=True) as db:
        rows = db.execute(
            "SELECT kind,key,value FROM state WHERE owner='2' "
            "AND kind IN ('download_tasks_v1','media_transfers_v1') ORDER BY kind",
        ).fetchall()
    assert [kind for kind, _, _ in rows] in (
        ['download_tasks_v1'], ['download_tasks_v1', 'media_transfers_v1'],
    )
    assert all(key == complete['taskId'] for _, key, _ in rows)
    records = [json.loads(raw) for _, _, raw in rows]
    assert all(not any('speed' in key.lower() or key.lower() in ('rate', 'bytes_per_second')
                       for key in record) for record in records)
    task = records[0]
    assert task['song_id'] == '7101' and task['status'] == 'complete'
    assert task['authorization_count'] == 2
    published = Path(task['target']).resolve(strict=True)
    assert published.is_relative_to(root / 'media') and published.suffix == '.flac'
    assert digest(published) == FLAC_SHA
    locale = read(out / 'locale.json')
    assert locale == {'numeric': 'C', 'messages': 'en_US.UTF-8', 'characters': 'en_US.UTF-8'}
    return {
        'ok': True, 'actualWebView': True, 'stages': len(STAGES), 'linuxX11': True,
        'liveRateBytesPerSecond': values[1]['rate'], 'resumedAtBytes': accepted,
        'publishedBytes': SIZE, 'publishedSha256': FLAC_SHA, 'locale': locale,
        'screenshotCount': len(expected) if screenshots else 0,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path, required=True)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    if not sys.platform.startswith('linux') or not os.environ.get('DISPLAY'):
        parser.error('an actual Linux GTK/X11 desktop is required')
    if ({name for _, name in socket.if_nameindex()} != {'lo'}
            or len(Path('/proc/net/route').read_text(encoding='ascii').splitlines()) != 1):
        parser.error('run in a sealed network namespace with only loopback and no external routes')
    root = args.root.resolve()
    root.mkdir(parents=True, mode=0o700)
    binary = args.binary.resolve(strict=True)
    dist = args.frontend_dist.resolve(strict=True)
    script = Path(__file__).resolve().parents[2] / 'desktop/examples/download_speed_ui_probe.js'
    env = {key: value for key, value in os.environ.items() if not key.upper().startswith('CUMULUS_')}
    env.update(LC_ALL='en_US.UTF-8', GDK_BACKEND='x11', CUMULUS_PROOF_DIR=str(root),
               CUMULUS_MOTION_PROOF='1', CUMULUS_DOWNLOAD_SPEED_UI_PROOF='1',
               CUMULUS_UX_SCRIPT=str(script), CUMULUS_PROOF_FRONTEND_DIST=str(dist))
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR'):
        path = root / key
        path.mkdir(mode=0o700)
        env[key] = str(path)
    provenance = {
        'binarySha256': digest(binary), 'frontendTreeSha256': frontend_digest(dist),
        'proofScriptSha256': digest(script),
    }
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        result = launch(binary, env, log, root / 'first', args.screenshots)
    failed = root / 'first/failed.json'
    assert not failed.exists(), read(failed) if failed.exists() else None
    assert result == 0, f'native probe exit {result}'
    report = verify(root, args.screenshots)
    assert provenance == {
        'binarySha256': digest(binary), 'frontendTreeSha256': frontend_digest(dist),
        'proofScriptSha256': digest(script),
    }, 'proof inputs changed while WebView was open'
    report.update(provenance)
    (root / 'audit.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    main()
