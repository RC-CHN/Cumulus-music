"""Run ordinary held-FLAC actor cases in fresh real Tauri WebViews."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys


def read_report(root, *, graph, peq, indexed=False):
    report = json.loads((root / 'report.json').read_text(encoding='utf-8'))
    assert report['ok'] and report['kind'] == 'ordinary_native_local_equalizer_v1'
    for flag in ('actualActor', 'webviewFinished',
                 'localGuest', 'sameLogicalPlayAcrossSeek'):
        assert report[flag] is True
    assert report['explicitNativeFile'] is (not indexed)
    assert report.get('indexedLocalFile', False) is indexed
    assert report['audioOutput'] == 'null' and report['realMusicRequests'] == report['realAccountWrites'] == 0
    assert report['fixedGraphSelected'] is graph and report['wrongDigestStockFallback'] is (not graph)
    assert report['pausedIndependentPeq'] is peq
    if sys.platform.startswith('linux'):
        assert report['numericLocaleIsC'] and report['messagesLocalePreserved']
    assert int(report['durableEqualizerRevision']) >= 2
    settings = report['durableEqualizerSettings']
    assert settings['enabled'] and settings['gains'] == [0, 0, 0, 6, 0, 0, 0, 0, 0, 0]
    assert (settings['bass_db'], settings['treble_db']) == (4, -2)
    pos = report['positions']
    assert pos['first'] >= .1 and pos['held'] is not None
    assert abs(pos['sought'] - .5) < .06 and pos['resumed'] > .65
    assert abs(pos['ended'] - 88_207 / 44_100) < .02
    native = report['native']
    assert native['ordinary_loads'] == 0
    load, = native['loads']
    seek, = native['seeks']
    assert load['native_file'] is (not indexed) and load['start'] == 0
    assert load.get('indexed_file', False) is indexed
    if indexed:
        assert load['codec'] == ('pcm_f32le' if graph else 'flac')
    assert seek['old_entry'] == load['entry'] and seek['seconds'] == .5
    if graph:
        assert load['graph_entry'] == load['entry']
        assert seek['replacement'] != load['entry'] and seek['graph_entry'] == seek['replacement']
        assert native['processed_entries'] == sorted([load['entry'], seek['replacement']])
        assert settings in native['local_updates']
        eof_entry = seek['replacement']
    else:
        assert load['graph_entry'] is seek['replacement'] is seek['graph_entry'] is None
        assert not native['processed_entries'] and not native['local_updates']
        eof_entry = load['entry']
    if peq:
        handoff, = native['yields']
        assert handoff['old_entry'] == seek['replacement'] and handoff['graph_entry'] is None
        assert handoff['replacement'] not in (load['entry'], seek['replacement'])
        assert native['peq_processed_entries'] == [handoff['replacement']]
        assert handoff['replacement'] not in native['processed_entries']
        clocks = report['peqHandoff']
        assert clocks['sameLogicalPlayAndQueue'] is True
        assert clocks['before'] > .65
        assert abs(clocks['actorPausedAfter'] - clocks['before']) < .1
        assert abs(clocks['nativePausedAfter'] - clocks['before']) < .1
        assert clocks['nativePausedAfter'] == native['stock_paused_position']
        assert clocks['processed'] > clocks['before'] + .05
        eof_entry = handoff['replacement']
    else:
        assert not native['yields'] and not native['peq_processed_entries']
        assert native['stock_paused_position'] is report['peqHandoff'] is None
    assert native['eof_entries'] == [eof_entry]
    return report



def verify_paused_native_clock(polls, replacement, target, rate):
    # At .5 s, one stale 44.1/48 kHz conversion shifts the actual native
    # clock by 40–45 ms. Keep the existing 44.1 kHz case unchanged.
    tolerance = .02 if rate == 48_000 else .06
    settled = [row['position'] for row in polls
               if row['entry'] == replacement and row['paused'] and not row['seeking']
               and row['position'] is not None and (rate != 48_000 or not row['buffering'])]
    assert any(abs(position - target) < tolerance for position in settled), {
        'replacement': replacement, 'rate': rate, 'target': target,
        'tolerance': tolerance, 'settledNativePositions': settled,
    }

def read_spatial_report(root, capture=False, source_kind='flac', rate=44_100):
    directory = root / 'spatial'
    names = [
        'ux_01_stock_saved', 'ux_02_hot_paused', 'ux_03_master_off',
        'ux_04_master_on', 'ux_05_paused_seek', 'ux_06_ended', 'ux_07_reset',
    ]
    assert sorted(path.stem for path in directory.glob('ux_*.json')) == names
    stages = [json.loads((directory / (name + '.json')).read_text(encoding='utf-8')) for name in names]
    complete = json.loads((directory / 'complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['startedSpatial'] and complete['stageCount'] == len(stages)
    stock, hot, off, on, sought, ended, reset = stages
    fixture = complete['fixtureSource']
    frames = rate * 2 + 7
    assert fixture['kind'] == source_kind and fixture['frames'] == frames and fixture['sampleRate'] == rate
    assert fixture['flacDecoderConfigured'] == (source_kind == 'flac')
    for row in (*stages, complete):
        assert row['webviewFinished'] and row['realMusicRequests'] == row['realAccountWrites'] == 0
        assert row['epoch'] == complete['epoch']
        assert row['fixtureSource'] == fixture
        if sys.platform.startswith('linux'):
            assert row['numericLocaleIsC'] and row['messagesLocalePreserved']
    for row, expected in zip(stages, [
        'unavailable', 'configured', 'bypassed', 'configured',
        'configured', 'pending', 'off',
    ], strict=True):
        eq = row['player']['equalizer']
        assert eq['spatial_status'] == expected and row['player']['error'] is None, row
        assert row['durable']['revision'] == int(eq['revision'])
        assert row['durable']['settings'] == eq['settings']
        assert row['player']['state'] == row['dom']['state']
        assert not eq['settings']['enabled'] and eq['settings']['gains'] == [0] * 10
    assert stock['fixtureOpens'] == 1 and stock['native']['ordinary_loads'] == 1
    assert not stock['native']['loads'] and not stock['native']['local_updates']
    assert stock['dom']['sliders'] == [3, 5] and stock['dom']['status'] == 'unavailable'
    source_pcm_sha = {
        44_100: 'fd4410b1284111d2f4a39fdbd252230e1f2d25b85f5aae146a980f7417b23f63',
        48_000: '0ed7634a1be5f5116c688980096c04b43e331022e69b3c3c990a1daaea09d0c4',
    }[rate]
    assert fixture['sourcePcmSha256'] == source_pcm_sha
    path, = root.glob('*.' + source_kind)
    with path.open('rb') as source:
        assert hashlib.file_digest(source, 'sha256').hexdigest() == fixture['sha256']
    if source_kind == 'wav':
        import wave
        with wave.open(str(path), 'rb') as source:
            assert (source.getnchannels(), source.getsampwidth(), source.getframerate(), source.getnframes()) == (2, 2, rate, frames)
            assert hashlib.sha256(source.readframes(frames)).hexdigest() == source_pcm_sha
        assert stock['native']['stock_codecs'] == ['pcm_s16le']
    elif rate == 48_000:
        assert fixture['sha256'] == '2caa47eb9e4aeb907424408ea3fe4efe931d2103969cf035154b68d3b36bc7b6'
        with path.open('rb') as source:
            header = source.read(42)
        assert header[:8] == b'fLaC\0\0\0\x22'
        streaminfo = int.from_bytes(header[18:26], 'big')
        assert (streaminfo >> 44, (streaminfo >> 41 & 7) + 1, (streaminfo >> 36 & 31) + 1) == (rate, 2, 16)
        assert streaminfo & ((1 << 36) - 1) == frames
        assert stock['native']['stock_codecs'] == ['flac']
    if source_kind == 'wav' or rate == 48_000:
        for row in (stock, hot, sought, ended):
            assert abs(row['player']['timeline']['total'] - frames / rate) <= 1 / rate
    assert hot['player']['playId'] != stock['player']['playId']
    assert hot['player']['entryId'] != stock['player']['entryId']
    for row in (hot, off, on):
        assert row['dom']['sliders'] == [6, 8]
        assert row['dom']['status'] == row['player']['equalizer']['spatial_status']
        assert row['dom']['master'] == ('false' if row is off else 'true')
        assert row['player']['state'] == 'paused'
        assert abs(row['player']['position'] - hot['player']['position']) < .06
        assert not row['native']['seeks'] and not row['native']['eof_entries']
    assert off['durable']['settings']['master_bypassed']
    assert not on['durable']['settings'].get('master_bypassed')
    identity = lambda row: tuple(row['player'][key] for key in ('playId', 'entryId', 'queueRevision'))
    assert all(identity(row) == identity(hot) for row in stages[1:])
    assert sought['player']['state'] == 'paused' and not sought['player']['nativeSeeking']
    seek_target = sought['seekTarget']
    assert seek_target['requested'] == .5 and seek_target['step'] > 0
    assert abs(seek_target['step'] - sought['player']['timeline']['total'] / 1000) < 1e-9
    assert abs(seek_target['accepted'] - seek_target['requested']) <= seek_target['step'] / 2 + 1e-9
    assert abs(sought['player']['position'] - seek_target['accepted']) < .06
    assert ended['player']['state'] == reset['player']['state'] == 'ended'
    assert abs(ended['player']['position'] - frames / rate) < .02
    assert reset['player']['position'] == ended['player']['position']
    assert reset['dom']['sliders'] == reset['dom']['tone'] == [0, 0]
    assert not any(reset['durable']['settings'].get(key, 0) for key in (
        'presence', 'stereoizer', 'bass_db', 'treble_db',
    ))

    native = complete['native']
    assert complete['fixtureOpens'] == 2 and native['ordinary_loads'] == 1 and not native['yields']
    load, = native['loads']
    seek, = native['seeks']
    rooted_windows = sys.platform == 'win32' and source_kind == 'wav'
    assert stock['native']['stock_rooted_windows'] == native['stock_rooted_windows'] == [rooted_windows]
    assert load['rooted_windows'] is rooted_windows
    assert load['native_file'] and load['codec'] == 'pcm_f32le' and load['graph_entry'] == load['entry']
    assert seek['seconds'] == seek_target['accepted'] and seek['old_entry'] == load['entry']
    assert seek['graph_entry'] == seek['replacement'] != load['entry']
    assert native['processed_entries'] == sorted([load['entry'], seek['replacement']])
    assert native['eof_entries'] == [seek['replacement']]
    if rate == 48_000:
        stock_entry, = native['stock_entries']
        expected_formats = {str(entry): [rate, 2] for entry in (stock_entry, load['entry'], seek['replacement'])}
        assert native['decoded_formats'] == expected_formats
        assert stock['native']['decoded_formats'] == {str(stock_entry): [rate, 2]}
        assert sought['native']['decoded_formats'] == expected_formats
    verify_paused_native_clock(sought['native']['last_polls'], seek['replacement'],
                               seek_target['accepted'], rate)
    updates = [(row.get('presence', 0), row.get('stereoizer', 0), row.get('master_bypassed', False))
               for row in on['native']['local_updates']]
    assert updates[-3:] == [(6, 8, False), (6, 8, True), (6, 8, False)], updates
    if capture:
        assert sorted(path.stem for path in directory.glob('ux_*.png')) == names
        for name, row in zip(names, stages, strict=True):
            expected = (['.equalizer-dialog'] if row['dom']['sliders'] is not None
                        else ['.sidebar-caption', '.playerbar'])
            assert [sample['source'] for sample in row['paintSamples']] == expected, row
            paint = json.loads((directory / ('paint-' + name + '.json')).read_text(encoding='utf-8'))
            assert len(paint['samples']) == len(expected) and all(
                sample['ok'] for sample in paint['samples']
            ), paint
    return {**complete, 'stages': stages, 'screenshots': len(names) if capture else 0}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--flac-library', type=Path)
    parser.add_argument('--flac-sha256', help='expected digest from the trusted stage')
    parser.add_argument('--root', required=True, type=Path)
    parser.add_argument('--mpv-library', type=Path)
    parser.add_argument('--filter-library', type=Path, help='also run the independent paused PEQ handoff')
    parser.add_argument('--frontend-dist', type=Path)
    spatial = parser.add_mutually_exclusive_group()
    spatial.add_argument('--indexed-flac-only', action='store_true',
                         help='run trusted graph and held stock via a native Linux library index')
    spatial.add_argument('--spatial-controls-only', action='store_true',
                         help='run the actual EQ UI with the ordinary held FLAC source')
    spatial.add_argument('--spatial-wav-only', action='store_true',
                         help='run the same actual EQ UI with held signed-16 PCM WAV and no FLAC configuration')
    spatial.add_argument('--spatial-wav-48k-only', action='store_true',
                         help='run the spatial WAV proof with actual 48 kHz native decoder checks')
    spatial.add_argument('--spatial-flac-48k-only', action='store_true',
                         help='run the same 48 kHz checks with a trusted FLAC library')
    parser.add_argument('--screenshots', action='store_true',
                        help='capture settled spatial stages on isolated Linux GTK')
    args = parser.parse_args()
    is_wave = args.spatial_wav_only or args.spatial_wav_48k_only
    rate_48k = args.spatial_wav_48k_only or args.spatial_flac_48k_only
    is_spatial = args.spatial_controls_only or is_wave or args.spatial_flac_48k_only
    if args.screenshots and (not sys.platform.startswith('linux') or not is_spatial):
        parser.error('--screenshots requires Linux and a spatial case')
    if is_wave and (args.flac_library or args.flac_sha256 or args.filter_library):
        parser.error('the WAV-only proof does not configure a FLAC or independent filter library')
    if not is_wave and not (args.flac_library and args.flac_sha256):
        parser.error('FLAC proofs require --flac-library and --flac-sha256')
    if args.indexed_flac_only and (not sys.platform.startswith('linux') or args.filter_library):
        parser.error('--indexed-flac-only requires Linux and no independent filter library')
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    binary = args.binary.resolve(strict=True)
    flac = []
    if args.flac_library:
        library = args.flac_library.resolve(strict=True)
        with library.open('rb') as source:
            assert hashlib.file_digest(source, 'sha256').hexdigest() == args.flac_sha256
        flac = [library, args.flac_sha256]
    mpv = [args.mpv_library.resolve(strict=True)] if args.mpv_library else []
    env = {key: value for key, value in os.environ.items() if not key.upper().startswith('CUMULUS_')}
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if sys.platform == 'win32' and mpv:
        # Native file inspection uses the adjacent package's ffprobe, not PATH.
        env['MPV_DLL_DIR_CUMULUS'] = str(mpv[0].parent)
    cases = [('trusted', True, []), ('wrong_digest', False, [])]
    if args.filter_library:
        cases.append(('paused_peq', True, [args.filter_library.resolve(strict=True)]))
    if args.spatial_controls_only:
        cases = [('spatial_controls', True, [])]
    elif args.spatial_wav_only:
        cases = [('spatial_wav', True, [])]
    elif args.spatial_wav_48k_only:
        cases = [('spatial_wav_48k', True, [])]
    elif args.spatial_flac_48k_only:
        cases = [('spatial_flac_48k', True, [])]
    elif args.indexed_flac_only:
        cases = [('indexed_flac', True, []), ('indexed_wrong_digest', False, [])]
    reports = {}
    for case, graph, filter_args in cases:
        work = root / case
        case_env = env.copy()
        for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'APPDATA', 'LOCALAPPDATA'):
            directory = root / f'{case}-profile' / key
            directory.mkdir(parents=True, mode=0o700)
            case_env[key] = str(directory)
        with (root / f'{case}.log').open('xb') as log:
            arguments = [case, *flac, work, *filter_args, *mpv]
            if args.screenshots:
                from window_manager import launch
                status = launch(binary, case_env, log, work / 'spatial', True, argv=arguments)
            else:
                status = subprocess.run([binary, *arguments], env=case_env, stdout=log,
                                        stderr=log, timeout=60).returncode
        failed = work / 'failed.json'
        if is_spatial:
            failed = work / 'spatial' / 'failed.json'
        assert status == 0 and not failed.exists(), (
            case, status, failed.read_text(encoding='utf-8') if failed.exists() else None,
        )
        reports[case] = (read_spatial_report(work, capture=args.screenshots,
                                             source_kind='wav' if is_wave else 'flac',
                                             rate=48_000 if rate_48k else 44_100) if is_spatial
                         else read_report(work, graph=graph, peq=bool(filter_args),
                                          indexed=args.indexed_flac_only))
    output = {'ok': True, 'actualWebViewAndActor': True, 'flacSha256': args.flac_sha256,
              'realMusicRequests': 0, 'physicalOutputVerified': False, 'cases': reports}
    (root / 'report.json').write_text(json.dumps(output, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({'ok': True, 'cases': list(reports), 'realMusicRequests': 0}))


if __name__ == '__main__':
    main()
