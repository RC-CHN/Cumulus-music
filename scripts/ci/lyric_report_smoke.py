"""Actual lyric report form, native account fences and durable one-send intents."""
import argparse
import json
import os
from pathlib import Path
from window_manager import launch

LABELS = ['blank_light', 'valid_light', 'valid_dark', 'short_dark', 'utf16_valid_dark',
          'too_long_dark', 'closed_dark', 'loading_dark', 'loading_cancelled_light',
          'types_failed_light', 'types_empty_light', 'types_recovered_dark',
          'long_types_light', 'long_types_small_dark', 'normal_restored_dark',
          'rejected_dark', 'rejected_light', 'not_sent_light', 'sent_light',
          'mini_entry_dark', 'mini_handoff_dark', 'target_frozen_light',
          'unknown_light', 'unknown_reopened_dark', 'unknown_read_light',
          'abandoned_light', 'fresh_dark', 'save_failed_dark', 'sending_light',
          'sending_changed_dark', 'sent_after_hold_dark', 'yrc_only_light',
          'yrc_sent_light', 'no_lyrics_dark', 'no_lyrics_sent_dark',
          'version_missing_light', 'version_missing_sent_light', 'account_closed_light']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_LYRIC_REPORT_PROOF='1',
               CUMULUS_MINI_SCRIPT=str(Path(__file__).resolve().parents[2]/'desktop/examples/lyric_report_probe.js'))
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        path = root/'os-profile'/name
        path.mkdir(parents=True, mode=0o700)
        env[key] = str(path)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve())+os.pathsep+env.get('PATH', '')
    with (root/'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, root/'first', args.screenshots)
    failed = root/'first/failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'lyric report process exit {code}'
    complete = json.loads((root/'first/complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['lyricReportUi'] and complete['stageCount'] == len(LABELS)
    assert complete['exactPayloadsVerified'] and complete['frozenTargetVerified'] and complete['duplicateSubmitBlocked']
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    rows = {}
    for i, label in enumerate(LABELS, 1):
        path = root/'first'/f'ux_{i:02}_{label}.json'
        row = json.loads(path.read_text(encoding='utf-8'))
        rows[label] = row
        assert row['theme'] == ('dark' if label.endswith('_dark') else 'light')
        assert row['factoryCount'] == 1 and not row['nativePlayer']['error']
        assert not row['overflow'] and row['focusInside']
        if label == 'mini_entry_dark':
            assert row['windowKind'] == 'mini' and row['reportEntry'] and not row['mainVisible']
            assert len(row['miniWindows']) == 1
        else:
            assert row['windowKind'] == 'main' and row['mainVisible'] and not row['miniWindows']
            if row['dialog']:
                assert all(b['hit'] for b in row['buttons'])
                rect = row['rect']
                assert rect['left'] >= 7 and rect['top'] >= 7
                assert rect['right'] <= row['width']-7 and rect['bottom'] <= row['height']-7
                assert not any(v in ['1', '2', '3'] for v in row['options'])
        if label != 'account_closed_light':
            player = row['nativePlayer']
            assert player['state'] == 'paused' and abs(player['position']-10) < .2
        if args.screenshots:
            assert path.with_suffix('.png').is_file()
    for name in ['valid_light', 'valid_dark', 'utf16_valid_dark']:
        assert rows[name]['sendDisabled'] is False
        assert rows[name]['text'] and rows[name]['selected'] == ['10'] and not rows[name]['reading']
    for name in ['blank_light', 'short_dark', 'too_long_dark', 'loading_dark', 'types_empty_light']:
        assert rows[name]['sendDisabled'] is True
    assert rows['long_types_light']['options'] == [str(i) for i in range(4, 44)]
    assert rows['types_empty_light']['options'] == [] and not rows['types_empty_light']['alerts']
    assert rows['types_recovered_dark']['options'] == [str(i) for i in range(4,15)] and not rows['types_recovered_dark']['alerts']
    small = rows['long_types_small_dark']
    assert (small['width'], small['height']) == (760, 540)
    assert small['bodyScroll']['scrollHeight'] > small['bodyScroll']['height'] and small['bodyScroll']['top'] > 0
    for name in ['rejected_dark', 'rejected_light', 'not_sent_light']:
        assert rows[name]['phase'] == 'rejected' and rows[name]['text'] and rows[name]['selected'] == ['14']
    for name in ['unknown_light', 'unknown_reopened_dark', 'unknown_read_light']:
        assert rows[name]['phase'] == 'uncertain' and rows[name]['sendDisabled'] is None
    assert rows['mini_handoff_dark']['reportId'] == rows['target_frozen_light']['reportId'] == rows['unknown_reopened_dark']['reportId']
    assert rows['target_frozen_light']['nativePlayer']['track']['id'] == '8' and '合成歌曲 7' in rows['target_frozen_light']['target']
    assert rows['save_failed_dark']['text'] and rows['save_failed_dark']['sendDisabled'] is False
    for name in ['sending_light', 'sending_changed_dark']:
        row = rows[name]
        assert row['working'] and row['sendDisabled'] and all(b['disabled'] for b in row['buttons'])
    assert not rows['account_closed_light']['dialog']
    assert not rows['account_closed_light']['lyricsPanel']
    assert not rows['account_closed_light']['oldAccountNotice']
    final = complete['syntheticLyricReports']
    assert rows['blank_light']['options'] == [str(i) for i in range(4, 15)]
    assert [v['correctType'] for v in final['writes']] == [14,14,14,10,10,10,10,10]
    assert len(final['writes']) == 8 and len(final['accepted']) == 6 and final['realRequests'] == 0
    for payload in final['writes'][:5]:
        assert payload['songId'] == '7' and payload['lyricVersion'] == 17
        assert set(payload) == {'songId', 'lyricVersion', 'content', 'correctType'}
    assert final['writes'][5]['songId'] == '8' and final['writes'][5]['lyricVersion'] == 42
    assert final['writes'][6]['songId'] == '9' and final['writes'][6]['lyricVersion'] == -1
    assert final['writes'][7]['songId'] == '10' and 'lyricVersion' not in final['writes'][7]
    report = dict(ok=True, states=len(LABELS), screenshots=len(LABELS) if args.screenshots else 0,
                  actualWebViews=2, syntheticWriteAttempts=8, syntheticAccepted=6,
                  exactPayloadsVerified=True, realMusicRequests=0, realAccountWrites=0,
                  physicalDisplayVerified=False, physicalAudioOutputVerified=False)
    (root/'report.json').write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
