"""Real desktop startup and native media, without accounts or remote requests."""
import argparse
import json
import os
from pathlib import Path
import subprocess


def run_native(command, env, report, timeout=60):
    log_path = report.with_suffix('.log')
    try:
        with log_path.open('x', encoding='utf-8') as log:
            subprocess.run(command, env=env, stdout=log, stderr=subprocess.STDOUT,
                           check=True, timeout=timeout)
    except (subprocess.TimeoutExpired, subprocess.CalledProcessError, OSError) as error:
        issue = {'ok': False, 'stage': 'native_process', 'timeoutSeconds': timeout}
        if isinstance(error, subprocess.TimeoutExpired):
            issue['kind'] = 'timeout'
        elif isinstance(error, subprocess.CalledProcessError):
            issue.update(kind='exit', returnCode=error.returncode)
        else:
            issue.update(kind='spawn', errno=error.errno)
        report.with_suffix('.failure.json').write_text(json.dumps(issue, indent=2) + '\n', encoding='utf-8')
        if log_path.exists():
            print(log_path.read_text(encoding='utf-8', errors='replace')[-6000:])
        raise


def read_native_report(report):
    if not report.is_file():
        progress = report.with_suffix('.progress.jsonl')
        last = None
        if progress.is_file():
            for line in progress.read_text(encoding='utf-8', errors='replace').splitlines():
                try:
                    last = json.loads(line).get('stage')
                except (json.JSONDecodeError, AttributeError):
                    pass
        issue = {'ok': False, 'stage': 'native_report', 'kind': 'missing_report', 'lastStage': last}
        report.with_suffix('.failure.json').write_text(json.dumps(issue, indent=2) + '\n', encoding='utf-8')
        log = report.with_suffix('.log')
        if log.is_file():
            print(log.read_text(encoding='utf-8', errors='replace')[-6000:])
        raise AssertionError('native process returned without its required verification report')
    return json.loads(report.read_text(encoding='utf-8'))


def verify_stereo_native(proof):
    assert proof['stereoNativeBoundary'] and proof['syntheticMediaOnly']
    assert proof['nativeNullOutputOnly'] and proof['remoteRequests'] == 0 and proof['accountWrites'] == 0
    assert not proof['productionStereoAttachment'] and not proof['wholeMaterialVerified']
    assert not proof['physicalAudioOutputVerified']
    component = proof['dynamicComponent']
    assert component['ok'] and component['realDynamicLibrary'] and component['versionCheckedByHost'] == 1
    assert component['validPcmBitIdenticalToCore'] and component['configurationRevoked']
    assert component['retainedInstanceMuted'] and component['reopenedFreshInstance']
    assert (component['sampleRate'], component['channels'], component['capacity']) == (48000, 2, 2048)
    assert len(component['librarySha256']) == 64
    source = [block for block in component['blocks'] if block['phase'] == 'source']
    tail = [block for block in component['blocks'] if block['phase'] == 'eof']
    assert len(source) == 20 and 1 < len(tail) <= 128
    assert sum(block['reset'] for block in source) == 1
    assert len({block['revision'] for block in source}) == 4
    assert all(not block['end'] and block['inputFrames'] == block['outputFrames'] for block in source)
    assert all(block['inputFrames'] == 0 for block in tail)
    assert all(not block['end'] for block in tail[:-1]) and tail[-1]['end']
    assert sum(block['outputFrames'] for block in tail) == component['tailFrames'] == 48000
    assert component['tailNonzero'] and component['completeEof']
    assert all(0 <= block['outputFrames'] <= 2048 and len(block['validPcmSha256']) == 64
               for block in component['blocks'])
    stats = component['finalStats']
    assert stats['revision'] == stats['observed_revision'] == 4
    assert stats['instances_created'] == 2 and stats['active_instances'] == 1
    assert stats['failed_blocks'] == 0 and stats['resets'] == 1
    assert stats['input_frames'] == sum(block['inputFrames'] for block in source)
    assert stats['output_frames'] == sum(block['outputFrames'] for block in component['blocks'])
    formats = proof['actualDecodedFormats']
    assert formats['ok'] and formats['monoPreserved'] and not formats['outputDeviceCoercionUsed']
    assert formats['staleEntryRejected'] and formats['stoppedEntryRejected']
    assert len(formats['cases']) == 2
    assert [case['channels'] for case in formats['cases']] == [1, 2]
    assert len({case['entryId'] for case in formats['cases']}) == 2
    for case in formats['cases']:
        assert case['channels'] == case['expectedChannels']
        assert case['nativeStereoQualification'] == (case['channels'] == 2)
        assert case['sampleRate'] == 48000 and case['decoder'] == 'pcm_s16le'
        assert case['output'] == 'null' and case['actualPosition'] > .08
    qualification = formats['observedFormatQualification']
    assert qualification['instances_created'] == 1 and qualification['active_instances'] == 0
    assert qualification['observed_revision'] == 0
    assert qualification['input_frames'] == qualification['output_frames'] == 0


STEREO_PCM_STAGES = [
    'stereo_pcm_seed_start', 'stereo_pcm_download_published',
    'stereo_pcm_cold_grant_opened', 'stereo_pcm_native_load_start',
    'stereo_pcm_paused_controls_verified', 'stereo_pcm_live_controls_verified',
    'stereo_pcm_stop_close_join_verified', 'stereo_pcm_seek_replacement_verified',
    'stereo_pcm_native_tail_eof_verified', 'stereo_pcm_buffered_revocation_verified',
    'stereo_pcm_native_closed',
]


def verify_stereo_pcm(proof):
    assert proof['kind'] == 'stereo_pcm_fixed_local_native_v3'
    assert proof['graphScope'] == 'complete_fixed_local_equalizer' and 'subgraph' not in proof
    assert all(proof[key] is True for key in [
        'ok', 'productionDesktopStartup', 'numericLocaleCompatible', 'guiInitialized',
        'actualDecoder', 'unknownLength', 'strictNcm', 'syntheticDownloadPublished', 'fixedLocalMaterial',
        'workerJoinedBeforeSeek', 'sourcePathReplaced', 'reopenRejected',
        'actualNativeEof', 'revokedReclaimDenied',
    ])
    assert all(proof[key] is False for key in [
        'fullSdkGraph', 'wholeMaterial', 'generalSdkGraph', 'settingWriteApplyOrderVerified',
        'officialUiTested', 'physicalAudioOutputVerified',
    ])
    assert proof['realMusicRequests'] == proof['coldHttpAttempts'] == 0
    assert proof['syntheticMediaTransfers'] == 1
    assert proof['format'] == {'rate': 44100, 'channels': 2, 'bitsPerSample': 16,
                               'maxBlockFrames': 1024, 'sourceFrames': 88207}
    assert proof['stages'] == STEREO_PCM_STAGES
    paused, live = proof['paused'], proof['live']
    entry = proof['sameControlEntry']
    assert proof['nativeDecodedFormat'] == {'rate': 44100, 'channels': 2, 'entry': entry}
    assert paused['nativeEntry'] == live['nativeEntry'] == entry
    assert paused['paused'] and not paused['graphEnd'] and not paused['failed']
    assert paused['nativeOutputFrame'] == 0 and paused['decodedSourceFrame'] < 88207
    assert 0 < paused['producedFrames'] <= (44100 + 7) // 8 + 4096
    revisions = proof['requestedControls']
    assert len(revisions) == 2 and 0 < revisions[0] < revisions[1]
    assert paused['requestedRevision'] == paused['appliedRevision'] == revisions[0]
    assert live['requestedRevision'] == live['appliedRevision'] == revisions[1]
    assert not live['failed'] and live['producedFrames'] <= live['nativeOutputFrame'] + 11025 + 2048
    assert proof['positionAfter'] > proof['positionBefore'] > .15
    retired = proof['retired']
    assert retired['nativeEntry'] == entry and retired['readerClosed'] and retired['nativeSinkRetired']
    seek = proof['seek']
    assert seek['oldSink'] == 1 and seek['newSink'] == 2
    # libmpv playlist entry numbers are scoped to one native handle.
    assert seek['oldEntry'] == entry and seek['newEntry'] > 0
    assert seek['oldRetirement'] in ('exact_stop_ack', 'native_destroyed')
    assert seek['firstSourceFrame'] == 22050 and seek['paused'] and seek['sameHeldSource']
    eof = proof['eof']
    assert eof['nativeEntry'] == seek['newEntry']
    assert eof['graphEnd'] and eof['readerClosed'] and eof['nativeSinkRetired']
    assert eof['producedFrames'] == 88207 - 22050 + 44100
    assert eof['decodedSourceFrame'] == eof['nativeSourceFrame'] == 88207
    assert proof['tailFrames'] == 44100
    revoked = proof['revoked']
    assert revoked['nativeEntry'] == proof['revokedEntry'] != seek['newEntry']
    assert revoked['readerClosed'] and revoked['nativeSinkRetired'] and revoked['cancelled']
    assert proof['revokedRetirement'] in ('exact_stop_ack', 'native_destroyed')
    assert proof['bufferedAheadAtRevocation'] > 2048


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--mpv-library', type=Path, help='explicit trusted libmpv for the stereo PCM proof')
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--auto', action='store_true', help='use the system audio output at zero volume')
    mode.add_argument('--filter-library', type=Path, help='verify recovered loudness PCM after GUI initialization')
    mode.add_argument('--bound-peq-library', type=Path,
                      help='verify supervised PEQ and real PCM after GUI initialization')
    mode.add_argument('--stereo-native-library', type=Path,
                      help='verify dynamic stereo ABI and actual decoder format, without attachment')
    mode.add_argument('--stereo-pcm-flac-library', type=Path,
                      help='verify sealed cold NCM/FLAC complete fixed local equalizer scope through unknown-size PCM after GUI initialization')
    args = parser.parse_args()
    if args.mpv_library and not args.stereo_pcm_flac_library:
        parser.error('--mpv-library requires --stereo-pcm-flac-library')
    report = args.report.resolve()
    assert not report.exists(), 'proof report must be new'
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env['CUMULUS_PROOF_REPORT'] = str(report)
    profile = report.with_name(report.stem + '-profile')
    profile.mkdir(exist_ok=False)
    env['CUMULUS_PROOF_WEBVIEW_DIR'] = str(profile / 'webview')
    for variable, directory in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                                ('XDG_CACHE_HOME', 'cache'), ('APPDATA', 'roaming'),
                                ('LOCALAPPDATA', 'local')]:
        path = profile / directory
        path.mkdir()
        env[variable] = str(path)
    if args.auto:
        env['CUMULUS_PROOF_AUTO'] = '1'
    if args.filter_library:
        env['CUMULUS_LOUDNESS_LIBRARY'] = str(args.filter_library.resolve(strict=True))
    if args.bound_peq_library:
        env['CUMULUS_BOUND_PEQ_LIBRARY'] = str(args.bound_peq_library.resolve(strict=True))
    if args.stereo_native_library:
        env['CUMULUS_STEREO_NATIVE_LIBRARY'] = str(args.stereo_native_library.resolve(strict=True))
    if args.stereo_pcm_flac_library:
        env['CUMULUS_STEREO_PCM_FLAC_LIBRARY'] = str(args.stereo_pcm_flac_library.resolve(strict=True))
    if args.mpv_library:
        env['CUMULUS_STEREO_PCM_MPV_LIBRARY'] = str(args.mpv_library.resolve(strict=True))
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    run_native([str(args.binary.resolve())], env, report)
    proof = read_native_report(report)
    assert proof['ok'] and proof['productionDesktopStartup'] and proof['numericLocaleCompatible']
    if args.stereo_native_library:
        verify_stereo_native(proof)
        print(json.dumps({'ok': True, 'desktopStereoNativeBoundary': True,
                          'sourceBlocks': 20, 'completeTailFrames': 48000,
                          'decodedChannels': [1, 2], 'productionAttachment': False}))
        return
    if args.stereo_pcm_flac_library:
        verify_stereo_pcm(proof)
        if args.mpv_library and os.name == 'nt':
            runtime = proof['nativeRuntime']
            assert runtime['kind'] == 'explicit_windows_runtime'
            assert runtime['libmpvApi'] > 0
            assert os.path.samefile(runtime['libmpvModule'], args.mpv_library)
        print(json.dumps({'ok': True, 'desktopPacedStereoPcm': True,
                          'outputFrames': proof['eof']['producedFrames'],
                          'graphScope': 'complete_fixed_local_equalizer', 'fixedLocalMaterial': True,
                          'generalSdkGraph': False, 'officialUiTested': False,
                          'physicalAudioOutputVerified': False}))
        return
    if args.bound_peq_library:
        assert proof['boundParametric'] and proof['actualDecoder']
        assert not proof['physicalAudioOutputVerified'] and proof['realMusicRequests'] == 0
        assert proof['syntheticRegistrationSetup'] == 1 and proof['materialReads'] == 1
        stats = proof['processed']
        assert stats['processed_samples'] > 1024 and stats['active_instances'] > 0
        assert stats['failed_blocks'] == 0 and stats['observed_revision'] == stats['revision']
        assert proof['confirmedWithoutClearing'] and proof['revocationDetached']
        assert proof['samePlay'] and proof['positionAfter'] > proof['positionBefore'] > 0.15
        print(json.dumps({'ok': True, 'desktopBoundParametric': True, 'processedSamples': stats['processed_samples']}))
        return
    if args.filter_library:
        assert proof['actualDecoder'] and proof['realMusicRequests'] == 0
        assert not proof['physicalAudioOutputVerified'] and not proof['businessPreferenceTested']
        assert len(proof['cases']) == 12
        assert {(c['rate'], c['channels']) for c in proof['cases']} == {(44100, 1), (48000, 2), (96000, 6)}
        for case in proof['cases']:
            assert case['maxAbsError'] <= 2e-6 and case['changed'] == case['enabled']
            assert case['stats']['processed_samples'] == case['samples']
            assert case['stats']['failed_blocks'] == 0 and case['stats']['active_instances'] == 0
        controls = proof['controls']
        assert controls['bypass']['observed_revision'] > controls['first']['observed_revision']
        assert controls['seek']['revision'] == controls['seek']['observed_revision']
        assert controls['reloaded']['processed_samples'] > 0 and controls['reloaded']['failed_blocks'] == 0
        print(json.dumps({'ok': True, 'desktopNativeLoudness': True, 'pcmCases': len(proof['cases'])}))
        return
    assert proof['metadata_probe'] and proof['remote_requests'] == 0
    assert all(proof[key] for key in ['progress', 'pause', 'seek', 'paused_seek_verified_on_resume', 'speed', 'cue_engine_eof', 'cue_end_reset', 'cancel'])
    print(json.dumps({'ok': True, 'desktopNativeMedia': True, 'output': proof['output']}))


if __name__ == '__main__':
    main()
