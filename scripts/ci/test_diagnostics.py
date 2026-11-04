import os
from pathlib import Path
import re
import sqlite3
import tarfile
import tempfile
import traceback
import unittest
from unittest.mock import patch

from diagnostics import archive


class ProofArchive(unittest.TestCase):
    def core_database(self, build, relative, entries):
        path = build / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        database = sqlite3.connect(path)
        try:
            database.execute('CREATE TABLE state (owner TEXT, kind TEXT, key TEXT, value TEXT)')
            database.executemany('INSERT INTO state VALUES (?, ?, ?, ?)', entries)
            database.commit()
        finally:
            database.close()
        return path

    def test_synthetic_account_credentials_and_recovery_databases_are_preserved(self):
        with tempfile.TemporaryDirectory() as directory:
            build = Path(directory)
            path = self.core_database(build, 'ci-login-notice-linux/run/state/core.sqlite3', [
                ('transport', 'saved-profiles-v1', 'synthetic', '{"uid":"synthetic-account"}'),
                ('account', 'phone-registration-v1', 'recovery', '{"status":"synthetic-pending"}'),
                ('device', 'synthetic-only', 'marker', '{"synthetic":true}'),
                ('transport', 'identity', 'marker', '{"synthetic":true}'),
            ])
            session = self.core_database(build, 'ci-login-notice-linux/run/state/sessions/synthetic/core.sqlite3', [
                ('transport', 'credentials', 'session', '{"cookies":["synthetic-cookie"]}'),
            ])
            self.core_database(build, 'private-session/core.sqlite3', [
                ('device', 'identity', 'last_observed', '{"private":"excluded-synthetic-sentinel"}'),
            ])
            contents = {path.relative_to(build).as_posix(): path.read_bytes(),
                        session.relative_to(build).as_posix(): session.read_bytes()}
            target = build / 'diagnostics/proofs.tar.gz'
            self.assertEqual(archive(build, target), 1)
            with tarfile.open(target) as saved:
                for relative, content in contents.items():
                    self.assertEqual(saved.extractfile(relative).read(), content)
                self.assertNotIn('private-session/core.sqlite3', saved.getnames())

    def test_device_identity_database_refuses_archive_before_tar_creation(self):
        with tempfile.TemporaryDirectory() as directory:
            build = Path(directory)
            sentinel = 'synthetic-private-device-facts-must-not-be-reported'
            self.core_database(build, 'ci-login-notice-linux/run/state/core.sqlite3', [
                ('device', 'identity', 'last_observed', '{"facts":"' + sentinel + '"}'),
            ])
            target = build / 'diagnostics/proofs.tar.gz'
            with self.assertRaisesRegex(ValueError, 'device identity records') as rejected:
                archive(build, target)
            self.assertNotIn(sentinel, ''.join(traceback.format_exception(rejected.exception)))
            self.assertFalse(target.exists())
            self.assertFalse(target.parent.exists())

    def test_identity_committed_only_in_wal_also_refuses_archive(self):
        with tempfile.TemporaryDirectory() as directory:
            build = Path(directory)
            path = self.core_database(build, 'ci-login-notice-windows/run/state/core.sqlite3', [])
            database = sqlite3.connect(path)
            try:
                self.assertEqual(database.execute('PRAGMA journal_mode = WAL').fetchone(), ('wal',))
                database.execute('PRAGMA wal_autocheckpoint = 0')
                database.execute('PRAGMA wal_checkpoint(TRUNCATE)')
                sentinel = 'synthetic-device-facts-in-uncheckpointed-wal'
                database.execute('INSERT INTO state VALUES (?, ?, ?, ?)',
                                 ('device', 'identity', 'last_observed', sentinel))
                database.commit()
                self.assertTrue(Path(str(path) + '-wal').is_file())
                target = build / 'diagnostics/proofs.tar.gz'
                with self.assertRaisesRegex(ValueError, 'device identity records') as rejected:
                    archive(build, target)
                self.assertNotIn(sentinel, ''.join(traceback.format_exception(rejected.exception)))
                self.assertFalse(target.exists())
            finally:
                database.close()

    def test_unverifiable_core_database_refuses_archive_without_database_text(self):
        for condition in ('corrupt', 'missing-state', 'missing-owner', 'directory'):
            with self.subTest(condition=condition), tempfile.TemporaryDirectory() as directory:
                build = Path(directory)
                path = build / 'ci-login-notice-linux/run/state/core.sqlite3'
                path.parent.mkdir(parents=True)
                sentinel = 'synthetic-unverifiable-database-value-must-not-be-reported'
                if condition == 'corrupt':
                    path.write_bytes(('not a SQLite database: ' + sentinel).encode())
                elif condition == 'directory':
                    path.mkdir()
                else:
                    database = sqlite3.connect(path)
                    try:
                        table = 'unrelated' if condition == 'missing-state' else 'state'
                        database.execute(f'CREATE TABLE {table} (value TEXT)')
                        database.execute(f'INSERT INTO {table} VALUES (?)', (sentinel,))
                        database.commit()
                    finally:
                        database.close()
                target = build / 'diagnostics/proofs.tar.gz'
                with self.assertRaisesRegex(ValueError, 'could not be verified') as rejected:
                    archive(build, target)
                self.assertNotIn(sentinel, ''.join(traceback.format_exception(rejected.exception)))
                self.assertFalse(target.exists())
                self.assertFalse(target.parent.exists())

    def test_preserves_fixture_names_and_excludes_unrelated_build_data(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            name = "滤镜 [test], quote'" + (' colon:' if os.name != 'nt' else '')
            proof = root / 'package-stage/linux-x64/equalizer-proof' / name / 'filter.txt'
            proof.parent.mkdir(parents=True)
            proof.write_text('synthetic', encoding='utf-8')
            (root / 'ci-engine-roundtrip-windows-x64.log').write_text('stage engine_start')
            (root / 'private-session').write_text('must not be archived')
            target = root / 'diagnostics/proofs.tar.gz'
            self.assertEqual(archive(root, target), 2)
            with tarfile.open(target) as saved:
                self.assertEqual(saved.extractfile(proof.relative_to(root).as_posix()).read(), b'synthetic')
                self.assertNotIn('private-session', saved.getnames())
                self.assertEqual(saved.extractfile('ci-engine-roundtrip-windows-x64.log').read(), b'stage engine_start')

    def test_webview2_lock_does_not_truncate_neighboring_failure_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            locked = root / 'ci-main-flow-windows/run/first/webview/EBWebView/lockfile'
            locked.parent.mkdir(parents=True)
            locked.touch()
            retained = [
                'ci-main-flow-windows/run/first/failed.json',
                'ci-main-flow-windows/run/first/webview/EBWebView/synthetic.log',
                'ci-main-flow-windows/run/first/lockfile',
                'ci-native-audio-windows.json',
            ]
            for name in retained:
                (root / name).write_text('synthetic evidence', encoding='utf-8')
            original = tarfile.bltn_open

            def open_with_windows_lock(name, *args, **kwargs):
                if Path(name) == locked:
                    raise PermissionError('synthetic WebView2 lock')
                return original(name, *args, **kwargs)

            target = root / 'diagnostics/proofs.tar.gz'
            with patch('tarfile.bltn_open', side_effect=open_with_windows_lock):
                self.assertEqual(archive(root, target), 2)
            with tarfile.open(target) as saved:
                self.assertNotIn(locked.relative_to(root).as_posix(), saved.getnames())
                for name in retained:
                    self.assertEqual(saved.extractfile(name).read(), b'synthetic evidence')

    def test_every_literal_workflow_smoke_root_and_neighboring_log_is_retained(self):
        repository = Path(__file__).resolve().parents[2]
        roots = set()
        for workflow in sorted((repository / '.github/workflows').glob('*.yml')):
            source = workflow.read_text(encoding='utf-8')
            # Derive expectations from the actual command arguments, without
            # importing the production root parser or its allowlist.
            for argument in re.findall(r'(?<!\S)--root[ \t]+(\.build/[^\s]+)', source):
                relative = argument.removeprefix('.build/')
                self.assertRegex(relative, r'^[A-Za-z0-9][A-Za-z0-9_-]*$')
                roots.add(relative)
        required = {
            'ci-account-login-windows',
            'ci-device-login-windows', 'ci-device-login-legacy-windows',
            'ci-sms-login-windows', 'ci-sms-login-legacy-windows',
            'ci-sms-send-failure-windows', 'ci-sms-send-failure-legacy-windows',
            'ci-phone-entry-windows', 'ci-phone-entry-legacy-windows',
            'ci-fm-feedback-dialog-windows', 'ci-fm-state-recovery-windows',
            'ci-fm-blacklist-windows', 'ci-queue-ui-windows',
            'ci-queue-selection-windows', 'ci-queue-collection-windows',
            'ci-queue-room-windows', 'ci-intelligence-ui-windows',
            'ci-intelligence-start-windows', 'ci-intelligence-cycle-windows',
            'ci-loudness-ui-windows',
        }
        self.assertEqual(required - roots, set())
        with tempfile.TemporaryDirectory() as directory:
            build = Path(directory)
            expected = {}
            for name in sorted(roots):
                failure = f'{name}/first/failed.json'
                path = build / failure
                path.parent.mkdir(parents=True)
                path.write_text('{"ok":false,"synthetic":true}', encoding='utf-8')
                expected[failure] = b'{"ok":false,"synthetic":true}'
                log = f'{name}.log'
                (build / log).write_text('synthetic WebView failure', encoding='utf-8')
                expected[log] = b'synthetic WebView failure'
            target = build / 'diagnostics/proofs.tar.gz'
            self.assertEqual(archive(build, target), 2 * len(roots))
            with tarfile.open(target) as saved:
                for relative, content in expected.items():
                    with self.subTest(path=relative):
                        self.assertIn(relative, saved.getnames())
                        self.assertEqual(saved.extractfile(relative).read(), content)

    def test_positional_login_native_reports_and_supported_platform_proofs_are_retained(self):
        paths = [
            'ci-login/first/failed.json', 'ci-login.log',
            'ci-native-audio-linux.json',
            'ci-native-audio-linux.progress.jsonl',
            'ci-native-audio-loudness-linux.failure.json',
            'ci-native-audio-windows.log',
            'ci-native-audio-loudness-windows.json',
            'ci-native-audio-loudness-windows-profile/webview/failed.json',
            'ci-engine-roundtrip-linux-x64.log',
            'ci-equalizer-roundtrip-windows-x64.log',
            'package-stage/windows-x64/equalizer-proof/filter.txt',
            'windows-media/equalizer-proof/audit.json',
            'windows-media/plan.json',
        ]
        with tempfile.TemporaryDirectory() as directory:
            build = Path(directory)
            for relative in paths:
                path = build / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text('synthetic declared proof', encoding='utf-8')
            target = build / 'diagnostics/proofs.tar.gz'
            self.assertEqual(archive(build, target), len(paths))
            with tarfile.open(target) as saved:
                for relative in paths:
                    with self.subTest(path=relative):
                        self.assertEqual(
                            saved.extractfile(relative).read(),
                            b'synthetic declared proof',
                        )

    def test_undeclared_private_neighbors_and_unsupported_package_platforms_are_excluded(self):
        with tempfile.TemporaryDirectory() as directory:
            build = Path(directory)
            included = build / 'ci-fm-state-recovery-windows/first/failed.json'
            included.parent.mkdir(parents=True)
            included.write_text('synthetic expected proof', encoding='utf-8')
            private = [
                'research/account.json',
                'references/session.json',
                'private-session/cookies.json',
                'ci-account-login-windows-private/first/failed.json',
                'ci-account-login-windows-private.log',
                'ci-fm-real-account/first/failed.json',
                'ci-fm-real-account.log',
                'ci-login-private-session/account.json',
                'ci-login-windows-private/account.json',
                'ci-native-audio-real-device.json',
                'ci-download-private-account/account.json',
                'ci-cloud-private-account/account.json',
                'ci-ux-real-account/account.json',
                'ci-email-login-private/account.json',
                'ci-question-login-private/account.json',
                'ci-reuse-login-private/account.json',
                'ci-mini-real-account/account.json',
                'ci-engine-roundtrip-windows.log',
                'ci-engine-roundtrip-private-windows-x64.log',
                'ci-equalizer-roundtrip-windows-x64-private.log',
                'package-stage/private-platform/equalizer-proof/filter.txt',
                'package-stage/linux-x64/equalizer-proof-private/filter.txt',
                'windows-media/private-session/account.json',
                'windows-media/player-probe-private.log',
            ]
            for relative in private:
                path = build / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text('synthetic excluded data', encoding='utf-8')
            target = build / 'diagnostics/proofs.tar.gz'
            self.assertEqual(archive(build, target), 1)
            with tarfile.open(target) as saved:
                names = set(saved.getnames())
                self.assertEqual(
                    saved.extractfile(included.relative_to(build).as_posix()).read(),
                    b'synthetic expected proof',
                )
                for relative in private:
                    with self.subTest(path=relative):
                        self.assertNotIn(relative, names)

    def test_early_failure_can_archive_an_empty_proof_set(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = root / 'diagnostics/proofs.tar.gz'
            self.assertEqual(archive(root, target), 0)
            with tarfile.open(target) as saved:
                self.assertEqual(saved.getnames(), [])

    def test_download_fixture_failure_and_neighboring_ui_logs_are_retained(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            paths = ['ci-download-page-linux/first.log', 'ci-download-menu-windows/first/failed.json',
                     'ci-playlist-menu-linux/host.log', 'ci-song-menu-linux/audit.json',
                     'ci-search-results-linux/first/failed.json', 'ci-playback-quality-linux/first/failed.json',
                     'ci-quality-trial-native-windows/reopen/failed.json', 'ci-quality-rights-ui-linux/first/failed.json',
                     'ci-volume-ui-windows/first/failed.json',
                     'ci-mode-ui-linux/reopen/failed.json',
                     'ci-session-renewal-windows/run/first/failed.json',
                     'ci-email-login-linux/run/first/failed.json',
                     'ci-question-login-windows/run/first/failed.json',
                     'ci-jsk-auth-windows/audit.log',
                     'ci-empty-fm-ui-windows/reopen/failed.json',
                     'windows-media/player-probe.stdout.log',
                     'ci-cloud-local-delete-windows/host.log', 'ci-local-resource-windows/first/failed.json',
                     'ci-rankings-linux/run/first/failed.json', 'ci-library-linux/first/failed.json',
                     'ci-card-source-windows/host.log', 'ci-collections-context-linux/run/first/failed.json', 'ci-album-card-windows/run/first/failed.json', 'ci-artist-card-windows/run/first/failed.json', 'ci-comment-read-windows/run/first/failed.json', 'ci-comment-resource-windows/run/first/failed.json']
            for relative in paths:
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text('synthetic', encoding='utf-8')
            (root / 'private-download-session').write_text('excluded')
            target = root / 'diagnostics/proofs.tar.gz'
            self.assertEqual(archive(root, target), len(paths))
            with tarfile.open(target) as saved:
                for relative in paths:
                    self.assertEqual(saved.extractfile(relative).read(), b'synthetic')
                self.assertNotIn('private-download-session', saved.getnames())


if __name__ == '__main__':
    unittest.main()
