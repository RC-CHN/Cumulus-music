"""Measure the production Lyrics table, resizing and a fresh desktop process."""

import argparse
import json
import os
from pathlib import Path

from motion_smoke import launch


FIRST = (
    'wide_defaults', 'title_dragged', 'album_keyboard', 'cancelled',
    'category_return', 'compact', 'wide_restored',
)
REOPEN = ('fresh_process',)


def close(a, b):
    return abs(a - b) < 1.3


def same_widths(a, b):
    return len(a) == len(b) and all(close(x, y) for x, y in zip(a, b, strict=True))


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def verify(output, labels, screenshots):
    files = [f'ux_{i:02}_{label}' for i, label in enumerate(labels, 1)]
    assert [path.stem for path in sorted(output.glob('ux_*.json'))] == files
    assert [path.stem for path in sorted(output.glob('ux_*.png'))] == (files if screenshots else [])
    complete = read(output / 'complete.json')
    assert complete['ok'] and complete['stageCount'] == len(labels)
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    rows = [read(output / (name + '.json')) for name in files]
    assert [row['label'] for row in rows] == list(labels)
    assert same_widths(rows[-1]['columns'], complete['columns'])
    for row in rows:
        if row['label'] == 'compact':
            assert row['viewport'] == [760, 540] and not row['handles']
        else:
            assert row['viewport'] == [1120, 760]
            assert row['handles'] == ['调整标题列宽', '调整专辑列宽']
            index, title, album, likes, duration = row['columns']
            assert close(index, 45) and close(likes, 60)
            assert title >= 179 and album >= 114 and duration >= 63
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--frontend-dist', required=True, type=Path)
    parser.add_argument('--root', required=True, type=Path)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    if os.name == 'nt' and args.screenshots:
        parser.error('pixel capture uses Linux GDK')

    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False, mode=0o700)
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/search_lyric_columns_ui_probe.js'
    base = {key: value for key, value in os.environ.items()
            if not key.upper().startswith('CUMULUS_')}
    base.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1',
                CUMULUS_SEARCH_UI_PROOF='1',
                CUMULUS_PROOF_FRONTEND_DIST=str(args.frontend_dist.resolve(strict=True)))
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        directory = root / 'os-profile' / name
        directory.mkdir(parents=True, mode=0o700)
        base[key] = str(directory)
    if args.runtime:
        base['PATH'] = str(args.runtime.resolve(strict=True)) + os.pathsep + base.get('PATH', '')

    reports = {}
    for phase, labels in [('first', FIRST), ('reopen', REOPEN)]:
        env = base.copy()
        script = root / (phase + '.js')
        script.write_text(
            'window.__lyricColumnsReopen = ' + json.dumps(phase == 'reopen') + ';\n'
            + source.read_text(encoding='utf-8'), encoding='utf-8',
        )
        env['CUMULUS_UX_SCRIPT'] = str(script)
        if phase == 'reopen':
            env['CUMULUS_PROOF_REOPEN'] = '1'
        output = root / phase
        with (root / (phase + '.log')).open('x', encoding='utf-8') as log:
            result = launch(args.binary.resolve(strict=True), env, log, output, args.screenshots,
                            acknowledge_receipts=True)
        failure = output / 'failed.json'
        assert not failure.exists(), read(failure) if failure.exists() else None
        assert result == 0, f'{phase} WebView exited {result}'
        reports[phase] = verify(output, labels, args.screenshots)

    defaults, dragged, keyboard, cancelled, returned, compact, restored = reports['first']
    fresh, = reports['reopen']
    initial, moved, committed = (row['columns'] for row in (defaults, dragged, keyboard))
    assert close(moved[1], initial[1] + 36) and close(moved[2], initial[2] - 36)
    assert close(moved[4], initial[4]) and close(committed[4], 64)
    assert all(same_widths(committed, row['columns'])
               for row in (cancelled, returned, restored, fresh))
    assert all(row['selected'] and row['expanded'] for row in (dragged, keyboard, cancelled))
    assert returned['expanded'] and not fresh['expanded']
    assert dragged['focused'] == '调整标题列宽' and keyboard['focused'] == '调整专辑列宽'
    audit = dict(ok=True, actualWebView=True, stageCount=len(FIRST) + len(REOPEN),
                 screenshots=(len(FIRST) + len(REOPEN)) if args.screenshots else 0,
                 headerBodyAlignment=True, initialColumns=initial, savedColumns=committed,
                 sameLayoutAfterRestart=True, keyboardFocusRetained=True,
                 compactHandlesHidden=True, realMusicRequests=0, realAccountWrites=0)
    (root / 'report.json').write_text(json.dumps(audit, ensure_ascii=False, indent=2) + '\n',
                                      encoding='utf-8')
    print(json.dumps(audit, ensure_ascii=False))


if __name__ == '__main__':
    main()
