"""Sealed Linux WebView check for the PlayerBar's purchased Song download."""
import argparse
import hashlib
import json
import os
from pathlib import Path

from search_group_download_smoke import flac_frames
from vinyl_lyric_copy_smoke import require_xvfb
from window_manager import launch

BASE = ('purchased', 'cancelled', 'stale', 'ordinary', 'baseline', 'selected')
MUSIC = ('authorize', 'media')
MUTATIONS = ('playlist_add', *MUSIC)
GUIDE = 'https://y.music.163.com/m/at/648fba3cbe7c73d87cbc2e81'


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def kinds(rows):
    return [row['kind'] for row in rows]


def verify(root, mode, fixture):
    output = root / 'first'
    locale = read(output / 'locale.json')
    assert locale['numeric'] == 'C' and locale['messages'] not in ('C', 'POSIX')
    complete = read(output / 'complete.json')
    tail = {
        'liked': ('transferred',),
        'playlist': ('picker', 'uncertain', 'transferred'),
        'directory': ('directory', 'settings'),
    }[mode]
    labels = (*BASE, *tail)
    stages = {}
    assert sorted(path.stem for path in output.glob('ux_*.json')) == [
        f'ux_{index:02}_{label}' for index, label in enumerate(labels, 1)
    ]
    assert complete['ok'] and complete['mode'] == mode and complete['stageCount'] == len(labels)
    assert complete['realAccountWrites'] == complete['realMusicRequests'] == 0
    assert not complete['legacyWrites']
    for index, label in enumerate(labels, 1):
        path = output / f'ux_{index:02}_{label}.json'
        assert path.with_suffix('.png').is_file()
        stage = stages[label] = read(path)
        assert stage['visible'] and stage['mode'] == mode and not stage['legacyWrites']
        current = stage['current']
        assert current['kind'] == 'catalog' and current['state'] == 'paused'
        assert current['entryId'] == current['queueCurrent']
        assert [item[1] for item in current['queue']] == ['2', '3']
        assert current['trackId'] == ('3' if label in ('stale', 'ordinary') else '2')
        if index >= labels.index('baseline') + 1:
            assert current == stages['baseline']['current']
        native = kinds(stage['currentSongDownloadCalls'])
        assert not {'unexpected_write', 'unexpected_auth', 'unguarded_write'}.intersection(native)
        if label not in ('uncertain', 'transferred'):
            assert not set(MUTATIONS).intersection(native)
    assert stages['purchased']['dom']['quality'] and stages['purchased']['dom']['selected'] == 'hires'
    assert stages['purchased']['dom']['tiers'] == ['hires', 'lossless', 'exhigh', 'standard']
    assert stages['cancelled']['dom']['focus'] and not stages['cancelled']['dom']['quality']
    assert not any(stages['stale']['dom'][modal] for modal in ('quality', 'ordinary', 'picker'))
    assert stages['ordinary']['dom']['ordinary'] and not stages['ordinary']['dom']['quality']
    assert stages['selected']['dom']['quality'] and stages['selected']['dom']['selected'] == 'lossless'

    actions = complete['actions']
    prepares = [row for row in actions if row['kind'] == 'prepare']
    assert [row['decision'] for row in prepares] == [
        'purchased_quality_required', 'purchased_quality_required', 'ordinary', 'purchased_quality_required',
    ]
    assert [row['trackId'] for row in prepares] == ['2', '2', '3', '2']
    assert prepares[0]['playId'] == prepares[1]['playId'] != prepares[2]['playId']
    assert len({prepares[index]['playId'] for index in (0, 2, 3)}) == 3
    assert [row['kind'] for row in actions[:len(stages['stale']['actions'])]].count('select') == 0
    selected, = [row for row in actions if row['kind'] == 'select']
    started, = [row for row in actions if row['kind'] == 'start']
    for row in [selected, started, *[row for row in actions if row['kind'] in
                                   ('destinations', 'choose_playlist', 'picker_status')]]:
        current = stages['baseline']['current']
        assert all(row[key] == current[key] for key in ('epoch', 'playId', 'entryId'))
        assert row['trackId'] == '2'
    assert selected['quality'] == started['quality'] == 'lossless'
    assert started['options'] == {
        'quality': 'lossless', 'tags': False, 'cover': False, 'lyrics': False, 'container': 'audio',
    }
    assert selected['decision'] == ('playlist_required' if mode == 'playlist' else 'ready')
    native = complete['currentSongDownloadCalls']
    assert [row['ids'] for row in native if row['kind'] == 'detail'] == [['2', '3'], ['3'], ['2', '3']]
    assert not {'unexpected_write', 'unexpected_auth', 'unguarded_write'}.intersection(kinds(native))

    if mode == 'playlist':
        assert stages['picker']['dom']['destinations'] == ['81']
        assert stages['uncertain']['dom']['readback'] and not set(MUSIC).intersection(
            kinds(stages['uncertain']['currentSongDownloadCalls']))
        assert [row['kind'] for row in actions if row['kind'] in ('choose_playlist', 'picker_status')] == [
            'choose_playlist', 'picker_status',
        ]
        add, = [row for row in native if row['kind'] == 'playlist_add']
        assert add['args'] == {'pid': '81', 'op': 'add', 'trackIds': '["2"]'}
        reads = [row for row in native if row['kind'] == 'playlist_detail']
        assert any(row['afterAdd'] and not row['present'] for row in reads)
        assert reads[-1]['present'] and native.index(add) < native.index(reads[-1])
        assert kinds(native).index('authorize') > native.index(reads[-1])
    else:
        assert not any(row['kind'] in ('destinations', 'playlist_detail', 'playlist_add') for row in native)

    opened = root / 'bin/xdg-open.receipt'
    if mode == 'liked':
        guide, = [row for row in actions if row['kind'] == 'guide']
        assert guide['result'] == 'opened' and opened.read_text(encoding='utf-8').splitlines() == ['1', GUIDE]
    else:
        assert not opened.exists() and 'guide' not in kinds(actions)
    if mode == 'directory':
        assert started['result'] == 'directory_required'
        assert stages['directory']['dom']['notice'].startswith('请先设置下载目录')
        settings = stages['settings']['dom']
        assert settings['settings'] and settings['settingsFocus'] and settings['settingsDirectoryAction']
        assert settings['settingsDirectoryPath'] == complete['settingsDirectoryPath'] == '未设置'
        assert complete['directoryChooserCalls'] == [stages['settings']['current']['epoch']]
        assert complete['directoryChooserCancelled'] and complete['directoryPreviewCalls'] == []
        assert not settings['ordinary'] and complete['tasks'] == []
        sample, = read(output / 'paint-ux_08_settings.json')['samples']
        assert sample['region'] == 'download-settings-heading' and sample['ok'] and sample['nearColorPixels'] >= 3
        assert not set(MUTATIONS).intersection(kinds(native))
        assert not any((root / 'media').rglob('*'))
        digest = None
    else:
        assert not complete['directoryChooserCalls'] and not complete['directoryPreviewCalls']
        assert not complete['directoryChooserCancelled']
        assert started['result'] == 'started' and started['startedCount'] == 1
        assert stages['transferred']['dom']['status'] == 'complete'
        task, = complete['tasks']
        assert (task['trackId'], task['status'], task['source']) == ('2', 'complete', 'song')
        assert task['requestedQuality'] == task['actualQuality'] == 'lossless'
        auth, = [row for row in native if row['kind'] == 'authorize']
        assert auth['args'] == {'id': '2', 'level': 'lossless', 'immerseType': 'c51'} and not auth['cached']
        media, = [row for row in native if row['kind'] == 'media']
        assert media['offset'] == 0 and media['bytes'] == fixture.stat().st_size
        file, = list((root / 'media').rglob('*.flac'))
        assert not file.is_symlink() and file.resolve().is_relative_to((root / 'media').resolve())
        assert flac_frames(file) == flac_frames(fixture)
        digest = hashlib.sha256(file.read_bytes()).hexdigest()
    return {'ok': True, 'actualLinuxWebView': True, 'mode': mode, 'stageCount': len(labels),
            'screenshots': len(labels), 'guardedPlaylistWrites': int(mode == 'playlist'),
            'nativeTransfers': int(mode != 'directory'), 'publishedSha256': digest}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--case', choices=('liked', 'playlist', 'directory'), required=True)
    args = parser.parse_args()
    require_xvfb()
    repo = Path(__file__).resolve().parents[2]
    root = args.root.resolve()
    root.mkdir(parents=True, mode=0o700)
    media = root / 'media'
    media.mkdir(mode=0o700)
    launcher = root / 'bin/xdg-open'
    launcher.parent.mkdir(mode=0o700)
    launcher.write_text('#!/bin/sh\nprintf "%s\\n" "$#" "$1" > "$0.receipt"\n', encoding='utf-8')
    launcher.chmod(0o700)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1',
               CUMULUS_CURRENT_SONG_DOWNLOAD_PROOF=args.case,
               CUMULUS_PROOF_FRONTEND_DIST=str(args.frontend_dist.resolve(strict=True)),
               PATH=str(launcher.parent) + os.pathsep + env.get('PATH', ''))
    if os.environ.get('CUMULUS_TEST_WINDOW_MANAGER'):
        env['CUMULUS_TEST_WINDOW_MANAGER'] = os.environ['CUMULUS_TEST_WINDOW_MANAGER']
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime')]:
        profile = root / 'os-profile' / name
        profile.mkdir(parents=True, mode=0o700)
        env[key] = str(profile)
    probe = repo / 'desktop/examples/current_song_download_ui_probe.js'
    source = root / 'current-song-download.js'
    source.write_text(f'window.__currentSongDownloadMode={json.dumps(args.case)};\n'
                      f'window.__currentSongDownloadDirectory={json.dumps(str(media))};\n'
                      + probe.read_text(encoding='utf-8'), encoding='utf-8')
    env['CUMULUS_UX_SCRIPT'] = str(source)
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        result = launch(args.binary.resolve(strict=True), env, log, root / 'first', True)
    failed = root / 'first/failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert result == 0, f'WebView probe exited {result}'
    report = verify(root, args.case, repo / 'core/tests/fixtures/stereo-pcm-source.flac')
    (root / 'audit.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
