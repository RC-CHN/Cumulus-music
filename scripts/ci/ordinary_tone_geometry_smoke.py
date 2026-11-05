"""Opt-in Linux WebView + instrumented stock tone callback witness."""
import argparse
from collections import defaultdict
import json
import math
import os
from pathlib import Path
import sys

from motion_smoke import launch


STAGES = ('01_mono_active', '02_mono_hot', '03_mono_paused_seek', '04_mono_resumed', '05_stereo_active')
CREATED, ACTIVATED, RUN, RETIRED = 1, 2, 3, 4
QUANTUM, STEREO_FRAMES = 2048, 144137


def same_play(a, b):
    assert a['playId'] and a['playId'] == b['playId'] and a['entryId'] == b['entryId']


def verify(output, screenshots):
    def read(name):
        return json.loads((output / (name + '.json')).read_text(encoding='utf-8'))

    locale = read('locale')
    assert locale['numeric'] == 'C' and locale['messages'] != 'C' and locale['characters'] != 'C'
    captures = [read('ux_' + stage) for stage in STAGES]
    complete = read('complete')
    assert complete['ok'] and complete['stageCount'] == len(STAGES)
    assert len(list(output.glob('ux_*.png'))) == (len(STAGES) if screenshots else 0)
    a, hot, sought, resumed, stereo = [record['observed'] for record in captures]
    for capture in captures:
        same_play(capture['observed'], capture['nativePlayer'])
    for capture in captures:
        stats = capture['toneGeometry']['equalizerStats']
        assert stats['active_instances'] > 0 and stats['processed_samples'] > 0 and stats['failed_blocks'] == 0
    for value in (hot, sought, resumed):
        same_play(a, value)
        assert value['track']['id'] == '1' and value['actualQuality'] == 'standard'
    assert a['track']['id'] == '1' and a['actualQuality'] == 'standard'
    assert a['position'] < hot['position'] and a['equalizer']['settings']['bass_db'] == 4
    assert hot['equalizer']['settings']['treble_db'] == -3
    assert sought['state'] == 'paused' and not sought['nativeSeeking'] and abs(sought['position'] - 1.37) < .12
    assert resumed['state'] == 'playing' and resumed['position'] > 1.56
    assert stereo['state'] == 'playing' and stereo['track']['id'] == '2'
    assert stereo['playId'] != resumed['playId'] and stereo['entryId'] != resumed['entryId']
    assert stereo['equalizer']['settings'] == hot['equalizer']['settings']
    assert complete['observed']['state'] == 'ended' and complete['observed']['track']['id'] == '2'

    trace = complete['toneGeometry']
    assert 'error' not in trace, trace
    assert trace['oneEngine']
    assert trace['sourceCalls'] == trace['loads'] == [1, 2]
    rows = trace['callbackRows']
    assert 0 < len(rows) <= trace['snapshotCursor'] <= 2048
    assert all(len(row) == 6 and row[0] == sequence for sequence, row in enumerate(rows, 1))
    instances = {}
    active, retired = set(), set()
    for _, lane, event, rate, frames, revision in rows:
        assert lane > 0 and rate in (44100, 48000)
        if event == CREATED:
            assert lane not in instances and frames == revision == 0
            instances[lane] = rate
        elif event == ACTIVATED:
            assert instances[lane] == rate and lane not in retired and frames == 0
            active.add(lane)
        elif event == RUN:
            assert instances[lane] == rate and lane in active and lane not in retired
            assert 0 < frames <= QUANTUM and revision > 0
        else:
            assert event == RETIRED and instances[lane] == rate and lane not in retired
            assert frames == revision == 0
            retired.add(lane)

    intervals = trace['intervals']
    assert all(0 <= mark['before'] <= mark['after'] <= trace['snapshotCursor'] for mark in intervals)
    loads = [row for row in intervals if row['kind'] == 'load']
    assert [row['song'] for row in loads] == [1, 2]
    assert all(row['detail']['codec'] == 'pcm_s16le' for row in loads)
    assert loads[0]['nativeEntry'] != loads[1]['nativeEntry']
    assert math.isclose(loads[1]['detail']['duration'], STEREO_FRAMES / 48000, abs_tol=.01)
    seeks = [row for row in intervals if row['kind'] == 'seek']
    assert len(seeks) == 1 and seeks[0]['song'] == 1 and seeks[0]['detail']['seconds'] == 1.37
    assert seeks[0]['nativeEntry'] == loads[0]['nativeEntry']
    ends = trace['eof']
    assert len(ends) == 1 and ends[0]['song'] == 2 and ends[0]['nativeEntry'] == loads[1]['nativeEntry']
    assert ends[0]['cursor'] <= len(rows)
    # A physical device or FFmpeg's speaker map is not exposed by this ABI.
    formats = [row for row in intervals if row['kind'].endswith('_format')]
    assert [(row['kind'], row['song'], row['detail']['rate'], row['detail']['channels']) for row in formats] == [
        ('load_format', 1, 44100, 1), ('seek_format', 1, 44100, 1), ('load_format', 2, 48000, 2),
    ]
    assert all(row['nativeEntry'] == loads[row['song'] - 1]['nativeEntry'] and row['detail']['output'] == 'null'
               for row in formats)
    assert all(row['before'] >= loads[row['song'] - 1]['before'] for row in formats)
    assert formats[1]['detail']['paused']
    assert formats[1] in captures[2]['toneGeometry']['intervals']
    first_revision = next(row for row in intervals if row['kind'] == 'material' and row['song'] == 1 and
                          row['detail']['settings'].get('bass_db') == 4 and
                          not row['detail']['settings'].get('treble_db'))
    hot_revision = next(row for row in intervals if row['kind'] == 'material' and row['song'] == 1 and
                        row['detail']['settings'].get('treble_db') == -3)
    assert first_revision['nativeEntry'] == hot_revision['nativeEntry'] == loads[0]['nativeEntry']
    assert first_revision['detail']['revision'] < hot_revision['detail']['revision']
    stops = [row for row in intervals if row['kind'] == 'stop' and row['song'] == 1]
    assert len(stops) == 1 and stops[0]['nativeEntry'] == loads[0]['nativeEntry']
    assert stops[0]['after'] <= loads[1]['before']
    resume = next(row for row in intervals if row['kind'] == 'resume' and row['song'] == 1 and
                  row['before'] >= seeks[0]['after'])

    def run_between(first, last, rate, revision=None):
        result = defaultdict(list)
        for seq, lane, event, observed_rate, frames, observed_revision in rows:
            if first < seq <= last and event == RUN:
                assert observed_rate == rate
                if revision is None or observed_revision == revision:
                    result[lane].append(frames)
        return result

    mono, mono_lanes = [], []
    for start, capture, material in ((first_revision['after'], captures[0], first_revision),
                                     (hot_revision['after'], captures[1], hot_revision),
                                     (resume['after'], captures[3], hot_revision)):
        lane_runs = run_between(start, len(capture['toneGeometry']['callbackRows']), 44100,
                                material['detail']['revision'])
        assert len(lane_runs) == 1 and all(set(frames) == {QUANTUM} for frames in lane_runs.values())
        mono_lanes.append(next(iter(lane_runs)))
        mono.append(dict(laneCount=len(lane_runs), callbackSizes=sorted({n for runs in lane_runs.values() for n in runs})))
    original, hot_lane, restarted = mono_lanes
    assert original == hot_lane and restarted != original
    seek_lifecycle = [(lane, event, revision) for seq, lane, event, rate, _, revision in rows
                      if seeks[0]['before'] < seq <= resume['before'] and event != RUN and rate == 44100]
    assert seek_lifecycle == [(original, RETIRED, 0), (restarted, CREATED, 0),
                              (restarted, ACTIVATED, hot_revision['detail']['revision'])], seek_lifecycle
    stereo_runs = run_between(loads[1]['before'], ends[0]['cursor'], 48000)
    assert len(stereo_runs) == 2, stereo_runs
    full, tail = divmod(STEREO_FRAMES, QUANTUM)
    assert all(runs == [QUANTUM] * full + [tail] for runs in stereo_runs.values()), stereo_runs
    return dict(ok=True, actualWebView=True, actualNativeCallbacks=True, nullOutput=True,
                syntheticAuthorizedSources=[1, 2], realNetworkRequests=0, realAccountWrites=0,
                decoded=[{'rate': row['detail']['rate'], 'channels': row['detail']['channels']} for row in formats],
                mono=mono, stereo={'lanes': len(stereo_runs), 'framesPerLane': STEREO_FRAMES,
                                   'fullCallbacksPerLane': full, 'tail': tail},
                geometryAndMaterialIntervals=True, hotKeptFilterInstance=True,
                seekSameEntry=True, seekRecreatedFilterWithHotRevision=True, officialPcmParity=False,
                physicalSpeakerLayout=False, actualHttpsDecode=False, installedDSPChanged=False,
                laneAttribution='opaque instances in single-engine intervals; no L/R or physical speaker names',
                callbackRows=len(rows), screenshots=len(STAGES) if screenshots else 0)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--library', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--outer-network', required=True, help='network namespace readlink before unshare')
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    assert sys.platform.startswith('linux')
    assert os.readlink('/proc/self/ns/net') != args.outer_network
    devices = {line.split(':', 1)[0].strip() for line in
               Path('/proc/self/net/dev').read_text(encoding='ascii').splitlines()[2:]}
    assert devices == {'lo'}, 'tone proof requires a loopback-only network namespace'
    root = args.root.resolve()
    root.mkdir(parents=True, mode=0o700, exist_ok=False)
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/tone_geometry_ui_probe.js'
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_UX_SCRIPT=str(source), CUMULUS_MOTION_PROOF='1',
               CUMULUS_TONE_GEOMETRY_PROOF='1', CUMULUS_AUDIO_FILTER=str(args.library.resolve(strict=True)))
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR', 'APPDATA', 'LOCALAPPDATA'):
        profile = root / 'profile' / key
        profile.mkdir(parents=True, mode=0o700)
        env[key] = str(profile)
    output = root / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(strict=True), env, log, output, args.screenshots)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'probe exited {code}'
    report = verify(output, args.screenshots)
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
