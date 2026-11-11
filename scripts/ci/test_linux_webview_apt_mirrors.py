"""Exercise direct signed mirror selection without mutating host APT."""

from datetime import datetime, timedelta, timezone
from pathlib import Path
import subprocess
import tempfile
from threading import Barrier
import unittest
from unittest.mock import patch

from linux_webview_apt import DIRECT_INDEX_OPTIONS, timed
from linux_webview_apt_mirrors import (
    DIRECT_DOWNLOAD_DEADLINE, DIRECT_MIRRORS, DIRECT_UPDATE_DEADLINE, DOWNLOAD_DEADLINE, KEYRING,
    MAX_POCKET_AGE, SUITES, UPDATE_DEADLINE, AptDeadline, Mirror, acquire, deadline_command,
    direct_sources, preflight, release_date,
    signed_noble_suites, verify_fresh_indexes,
)


SOURCE = '''# untouched runner source
Types: deb
URIs: mirror+file:/etc/apt/apt-mirrors.txt
Suites: noble noble-updates noble-backports
Components: main restricted universe multiverse
Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg

Types: deb
URIs: mirror+file:/etc/apt/apt-mirrors.txt
Suites: noble-security
Components: main restricted universe multiverse
Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg
'''
NOW = datetime(2026, 11, 11, 14, 0, tzinfo=timezone.utc)
CANONICAL = Mirror(*DIRECT_MIRRORS[1], dict.fromkeys(SUITES, NOW), 0.2)
MIT = Mirror(*DIRECT_MIRRORS[-1], dict.fromkeys(SUITES, NOW), 0.3)


class LinuxWebViewAptMirrors(unittest.TestCase):
    def fixture(self, fail_update=None, fail_download=None):
        events = []

        def operation(label, fail):
            def call(mirror, limit):
                events.append((label, mirror and mirror.name, limit))
                if fail is not None and mirror == CANONICAL:
                    raise fail
            return call

        args = (operation('update', fail_update),
                lambda mirror: events.append(('fresh plan', mirror and mirror.name)),
                operation('download-only', fail_download),
                lambda: events.append('discard archives'))
        return args, events

    def test_download_deadline_revalidates_before_single_install(self):
        args, events = self.fixture(fail_download=AptDeadline('download', DIRECT_DOWNLOAD_DEADLINE))
        with acquire(*args, (CANONICAL, MIT)) as ready:
            self.assertEqual(ready, (MIT, True))
            events.append('normal install')
        self.assertEqual(events, [
            ('update', 'Canonical', DIRECT_UPDATE_DEADLINE),
            ('fresh plan', 'Canonical'), ('download-only', 'Canonical', DIRECT_DOWNLOAD_DEADLINE),
            'discard archives', ('update', 'MIT', DIRECT_UPDATE_DEADLINE),
            ('fresh plan', 'MIT'), ('download-only', 'MIT', DIRECT_DOWNLOAD_DEADLINE), 'normal install',
        ])

    def test_update_deadline_never_admits_old_plan(self):
        args, events = self.fixture(fail_update=AptDeadline('update', DIRECT_UPDATE_DEADLINE))
        with acquire(*args, (CANONICAL, MIT)) as ready:
            self.assertEqual(ready, (MIT, True))
        self.assertEqual(events[:4], [
            ('update', 'Canonical', DIRECT_UPDATE_DEADLINE), 'discard archives',
            ('update', 'MIT', DIRECT_UPDATE_DEADLINE), ('fresh plan', 'MIT'),
        ])

    def test_unknown_image_has_stock_behavior_and_no_ready_mirrors_try_stock_once(self):
        for routes, limit, downloaded in ((None, None, False), ((None,), UPDATE_DEADLINE, True)):
            with self.subTest(routes=routes):
                args, events = self.fixture()
                with acquire(*args, routes) as ready:
                    self.assertEqual(ready, (None, downloaded))
                expected = [('update', None, limit), ('fresh plan', None)]
                if downloaded:
                    expected.append(('download-only', None, DOWNLOAD_DEADLINE))
                self.assertEqual(events, expected)

    def test_authoritative_signature_or_install_error_never_tries_next_route(self):
        args, events = self.fixture(fail_update=subprocess.CalledProcessError(100, 'apt-get'))
        with self.assertRaises(subprocess.CalledProcessError), acquire(*args, (CANONICAL, MIT)):
            self.fail('cannot install after an authoritative update error')
        self.assertEqual(events, [('update', 'Canonical', DIRECT_UPDATE_DEADLINE)])

        args, events = self.fixture()
        with self.assertRaises(AptDeadline), acquire(*args, (CANONICAL, MIT)):
            raise AptDeadline('install fixture', 1)
        self.assertEqual(events, [('update', 'Canonical', DIRECT_UPDATE_DEADLINE),
                                  ('fresh plan', 'Canonical'),
                                  ('download-only', 'Canonical', DIRECT_DOWNLOAD_DEADLINE)])

    def test_final_deadline_stops_without_install_or_third_attempt(self):
        args, events = self.fixture()

        def update(mirror, limit):
            events.append((mirror.name, limit))
            raise AptDeadline('update', limit)

        with self.assertRaises(AptDeadline), acquire(update, *args[1:], (CANONICAL, MIT)):
            self.fail('cannot install after the last timeout')
        self.assertEqual(events, [('Canonical', DIRECT_UPDATE_DEADLINE), 'discard archives',
                                  ('MIT', DIRECT_UPDATE_DEADLINE)])

    def test_direct_variant_replaces_only_ubuntu_uris_without_touching_other_sources(self):
        with tempfile.TemporaryDirectory() as work:
            root = Path(work)
            stock = root / 'stock'
            stock.mkdir()
            original = stock / 'ubuntu.sources'
            original.write_text(SOURCE)
            vendor = stock / 'microsoft-prod.list'
            vendor.write_text('deb https://packages.microsoft.com/ubuntu/24.04/prod noble main\n')
            (stock / 'ignored.backup').touch()
            private = root / 'private'
            with patch('linux_webview_apt_mirrors.SOURCES', original):
                direct_sources(SOURCE, private, MIT.uri)
            self.assertEqual((private / original.name).read_text(), SOURCE.replace(
                'mirror+file:/etc/apt/apt-mirrors.txt', MIT.uri))
            self.assertEqual(original.read_text(), SOURCE)
            self.assertEqual((private / vendor.name).readlink(), vendor)
            self.assertFalse((private / 'ignored.backup').exists())

    def test_direct_options_disable_only_non_package_indexes(self):
        keys = []
        for flag, option in zip(DIRECT_INDEX_OPTIONS[::2], DIRECT_INDEX_OPTIONS[1::2], strict=True):
            self.assertEqual(flag, '-o')
            key, value = option.split('=', 1)
            self.assertEqual(value, 'none' if key == 'Acquire::Languages' else 'false')
            keys.append(key)
        self.assertEqual(set(keys), {
            'Acquire::Languages',
            *(f'Acquire::IndexTargets::deb::{name}::DefaultEnabled' for name in (
                'DEP-11', 'DEP-11-icons-small', 'DEP-11-icons', 'DEP-11-icons-hidpi',
                'DEP-11-icons-large', 'DEP-11-icons-large-hidpi', 'CNF')),
        })

    def test_only_the_exact_signed_noble_source_is_recognized(self):
        self.assertEqual(signed_noble_suites(SOURCE), SUITES)
        for other in (SOURCE.replace('noble-security', 'jammy-security'),
                      SOURCE.replace('noble-security', 'noble'),
                      SOURCE.replace('Signed-By:', 'Trusted:'),
                      SOURCE.replace('mirror+file:', 'file:'),
                      SOURCE.replace('Types: deb', 'Types: deb\nCheck-Valid-Until: no'),
                      SOURCE.replace('Types: deb', 'Types: deb\nValid-Until-Min: 99999999'),
                      SOURCE.replace('Types: deb', 'Types: deb\nAllow-Weak: yes'),
                      SOURCE.replace('Types: deb', 'Types: deb\nURIs: http://example.invalid/')):
            with self.subTest(source=other):
                self.assertFalse(signed_noble_suites(other))

    def test_parallel_preflight_requires_every_suite_prefers_recent_metadata_and_bounds_routes(self):
        barrier = Barrier(len(DIRECT_MIRRORS) * len(SUITES), timeout=5)

        def probe(uri, suite, now):
            barrier.wait()
            if uri == DIRECT_MIRRORS[0][1] and suite == 'noble-backports':
                return None, 0.1, 'HTTP 404'
            if uri == MIT.uri:
                return NOW - timedelta(hours=2), 0.1, None
            return NOW, 0.2 if uri == CANONICAL.uri else 0.3, None

        with patch('linux_webview_apt_mirrors.probe_suite', side_effect=probe):
            chosen = preflight()
        self.assertEqual([mirror.name for mirror in chosen], ['Canonical', 'Security'])
        with patch('linux_webview_apt_mirrors.probe_suite', return_value=(None, 0, 'deadline')):
            self.assertEqual(preflight(), (None,))

    def test_release_checks_signature_fields_and_mutable_dates(self):
        def payload(suite, when, **kwargs):
            fields = dict(Origin='Ubuntu', Suite=suite, Codename='noble', Date=when.strftime(
                '%a, %d %b %Y %H:%M:%S %z'))
            fields.update(kwargs)
            return ''.join(f'{k}: {v}\n' for k, v in fields.items()).encode()

        stale = NOW - MAX_POCKET_AGE - timedelta(days=1)
        for suite, date, valid in [('noble-security', NOW, True), ('noble-updates', stale, False),
                                   ('noble-backports', stale, True), ('noble', stale, True),
                                   ('noble-security', NOW + timedelta(minutes=1), False)]:
            with self.subTest(suite=suite, date=date), patch('linux_webview_apt_mirrors.subprocess.run',
                    return_value=subprocess.CompletedProcess([], 0, payload(suite, date))) as gpgv:
                if valid:
                    self.assertEqual(release_date(b'fixture signed bytes', suite, NOW), date)
                else:
                    with self.assertRaises(ValueError):
                        release_date(b'fixture signed bytes', suite, NOW)
                self.assertEqual(gpgv.call_args.args[0], ['gpgv', '--keyring', KEYRING, '--output', '-', '-'])
                self.assertTrue(gpgv.call_args.kwargs['check'])
        for invalid in (payload('noble-security', NOW, Codename='jammy'),
                        payload('noble-security', NOW, **{'Valid-Until': 'Mon, 09 Nov 2026 14:00:00 +0000'})):
            with patch('linux_webview_apt_mirrors.subprocess.run',
                       return_value=subprocess.CompletedProcess([], 0, invalid)), self.assertRaises(ValueError):
                release_date(b'fixture signed bytes', 'noble-security', NOW)

    def test_authoritative_lists_must_exist_and_never_go_back_behind_preflight(self):
        with tempfile.TemporaryDirectory() as work:
            lists = Path(work)
            for suite in SUITES:
                (lists / f'mirrors.mit.edu_ubuntu_dists_{suite}_InRelease').write_text(suite)
            with patch('linux_webview_apt_mirrors.release_date', return_value=NOW):
                verify_fresh_indexes(MIT, lists)
            with patch('linux_webview_apt_mirrors.release_date', return_value=NOW - timedelta(minutes=1)):
                with self.assertRaisesRegex(ValueError, 'regressed'):
                    verify_fresh_indexes(MIT, lists)
            (lists / 'mirrors.mit.edu_ubuntu_dists_noble-security_InRelease').unlink()
            with patch('linux_webview_apt_mirrors.release_date', return_value=NOW):
                with self.assertRaises(FileNotFoundError):
                    verify_fresh_indexes(MIT, lists)

    def test_watchdog_runs_inside_sudo_and_does_not_wrap_dpkg(self):
        command = ['sudo', 'apt-get', '--download-only', 'install', '-y', 'test']
        self.assertEqual(deadline_command(command, 150), [
            'sudo', 'timeout', '--signal=TERM', '--kill-after=5s', '150s', *command[1:],
        ])
        with self.assertRaises(ValueError):
            deadline_command(['sudo', 'apt-get', 'install', '-y', 'test'], 150)
        for code, elapsed, expected in ((124, 151, AptDeadline), (137, 155, AptDeadline),
                                        (137, 4, subprocess.CalledProcessError),
                                        (100, 4, subprocess.CalledProcessError)):
            with self.subTest(code=code, elapsed=elapsed), patch('linux_webview_apt.subprocess.run',
                    side_effect=subprocess.CalledProcessError(code, command)), patch(
                    'linux_webview_apt.time.monotonic', side_effect=(0, elapsed, elapsed)):
                with self.assertRaises(expected):
                    timed('download', command, {}, 150)


if __name__ == '__main__':
    unittest.main()
