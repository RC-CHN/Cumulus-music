"""Revoke the isolated native held-FLAC graph without touching its media bytes."""
import argparse
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import re
import sqlite3
import sys

from search_download_row_smoke import DOWNLOAD, SEARCH, read
from vinyl_lyric_copy_smoke import require_xvfb
from window_manager import launch

OWNER = '2'
LEDGER = 'offline_music_v1'


def owned_fds(pid, media_stat):
    result = []
    for descriptor in Path(f'/proc/{pid}/fd').iterdir():
        try:
            info = descriptor.stat()
        except FileNotFoundError:
            continue
        if (info.st_dev, info.st_ino) == (media_stat.st_dev, media_stat.st_ino):
            result.append(descriptor.name)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path, required=True)
    parser.add_argument('--flac-library', type=Path, required=True)
    parser.add_argument('--flac-sha256', required=True)
    parser.add_argument('--root', type=Path, required=True)
    args = parser.parse_args()
    require_xvfb()
    if sys.platform != 'linux' or len(Path('/proc/net/route').read_text().splitlines()) != 1:
        parser.error('the proof requires a Linux Xvfb namespace without routes')
    binary, frontend, library = (
        p.resolve(strict=True) for p in (args.binary, args.frontend_dist, args.flac_library)
    )
    digest = args.flac_sha256
    assert re.fullmatch('[0-9a-f]{64}', digest)
    assert hashlib.sha256(library.read_bytes()).hexdigest() == digest
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False, mode=0o700)
    repo = Path(__file__).resolve().parents[2]
    seed = (repo / 'core/tests/fixtures/stereo-pcm-source.flac').read_bytes()
    seed_sha = hashlib.sha256(seed).hexdigest()
    env = {key: value for key, value in os.environ.items()
           if not key.upper().startswith(('CUMULUS_', 'WEBVIEW2_')) and key.upper() != 'MPV_DLL_DIR_CUMULUS'}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1',
               CUMULUS_SEARCH_DOWNLOAD_ROW_PROOF='1', CUMULUS_SEARCH_DOWNLOAD_HELD_REPLACEMENT='1',
               CUMULUS_SEARCH_DOWNLOAD_EQ_LIBRARY=str(library), CUMULUS_SEARCH_DOWNLOAD_EQ_SHA256=digest,
               CUMULUS_PROOF_FRONTEND_DIST=str(frontend), LC_ALL='en_US.UTF-8', GDK_BACKEND='x11',
               CUMULUS_UX_SCRIPT=str(repo / 'desktop/examples/search_download_revocation_ui_probe.js'))
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR'):
        folder = root / key
        folder.mkdir(mode=0o700)
        env[key] = str(folder)
    output = root / 'first'
    revoked = {}

    def checkpoint(path, payload, pid):
        if path.stem == 'ux_01_revoke_ready':
            assert not revoked
            assert payload['view']['playing']['equalizer']['spatial_status'] == 'processing'
            assert payload['nativePlayer']['state'] == 'paused'
            [media] = (root / 'search-download-media').glob('*.flac')
            assert not media.is_symlink() and media.read_bytes() == seed
            media_stat = media.stat()
            descriptors = owned_fds(pid, media_stat)
            assert len(descriptors) == 1, 'the paused graph must still own the checked media handle'
            db = (root / 'state' / 'core.sqlite3').resolve(strict=True)
            assert db.is_relative_to(root)
            with closing(sqlite3.connect(db.as_uri() + '?mode=rw', uri=True, timeout=2)) as store, store:
                store.execute('BEGIN IMMEDIATE')
                [(key, raw)] = store.execute(
                    'SELECT key,value FROM state WHERE owner=? AND kind=?', (OWNER, LEDGER)
                ).fetchall()
                record = json.loads(raw)
                publication = payload['searchDownloadFixture']['publication']
                assert record['path'] == str(media) and record['song_id'] == '881'
                assert not record['encrypted'] and record['integrity']['sha256'] == seed_sha
                assert record['task_id'] == publication['taskId']
                assert store.execute(
                    'DELETE FROM state WHERE owner=? AND kind=? AND key=? AND value=?',
                    (OWNER, LEDGER, key, raw)
                ).rowcount == 1
            revoked.update(media=media, stat=media_stat, before=len(descriptors))
        elif path.stem == 'ux_02_revoked':
            assert revoked and payload['nativePlayer']['state'] == 'error'
            assert not owned_fds(pid, revoked['stat']), 'revoked source must release the actual decoder handle'
            revoked['after'] = 0
        else:
            raise AssertionError(f'unexpected capture {path.name}')

    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(binary, env, log, output, True, before_ack=checkpoint)
    failed = output / 'failed.json'
    assert not failed.exists(), read(failed) if failed.exists() else None
    complete = read(output / 'complete.json')
    assert code == 0 and complete['ok'] and complete['stageCount'] == 2
    before, after = [read(output / name) for name in ('ux_01_revoke_ready.json', 'ux_02_revoked.json')]
    ready, stopped = before['nativePlayer'], after['nativePlayer']
    assert ready['state'] == 'paused' and ready['catalogFileSource'] and ready['playId']
    assert stopped['state'] == 'error' and stopped['error'] and not stopped.get('playId')
    assert before['nativeQueue'] == after['nativeQueue']
    assert ready['queueRevision'] == stopped['queueRevision']
    assert len(before['nativeQueue']['tracks']) == 1 and before['nativeQueue']['tracks'][0]['track']['id'] == '881'
    for item in (before, after, complete):
        trace = item['searchDownloadFixture']
        assert trace['search'] == [SEARCH] and trace['download'] == [DOWNLOAD] and trace['mediaReads'] == 1
        assert trace['online'] == [] and not trace['corrupted']
        assert trace['seeks'] == [] and not any(event['reason'] == 'Eof' for event in trace['ends'])
        [loaded] = trace['loads']
        assert loaded['kind'] == 'published_flac' and loaded['codec'] == 'pcm_f32le'
        assert loaded['transport'] == 'held_equalizer_pcm' and loaded['heldReplacementVerified']
        assert loaded['handleBytes'] == len(seed) and loaded['handleSha256'] == seed_sha
    assert revoked['before'] == 1 and revoked['after'] == 0
    assert revoked['media'].read_bytes() == seed and not list(revoked['media'].parent.glob('*.held-original'))
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    assert read(output / 'locale.json') == {
        'numeric': 'C', 'messages': 'en_US.UTF-8', 'characters': 'en_US.UTF-8',
    }
    report = dict(ok=True, actualWebView=True, nativeNullOutput=True, sameVerifiedDescriptor=True,
                  processedPcmBeforeRevocation=True, syntheticLedgerRemoved=True,
                  descriptorBefore=revoked['before'], descriptorAfter=revoked['after'],
                  oldQueuePreserved=True, naturalEof=False, nativeLoads=1,
                  authorizationForFixtureDownload=1, fixtureCdnReads=1, extraAuthorizationOrCdnReads=0,
                  realMusicRequests=0, realAccountWrites=0, physicalAudioOutputVerified=False)
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
