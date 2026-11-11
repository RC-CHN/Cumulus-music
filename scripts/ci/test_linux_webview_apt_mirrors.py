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
    MAX_POCKET_AGE, MAX_RELEASE, PROBE_TIMEOUT, SUITES, UPDATE_DEADLINE, AptDeadline, AptIndexRegression,
    Mirror, acquire, deadline_command,
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

    def test_signed_index_regression_retries_one_mirror_without_lowering_the_date_floor(self):
        older = Mirror(MIT.name, MIT.uri, dict.fromkeys(SUITES, NOW - timedelta(hours=8)), MIT.seconds)
        for final_date in (NOW, NOW - timedelta(hours=8)):
            with self.subTest(final_date=final_date), tempfile.TemporaryDirectory() as work:
                args, events = self.fixture()
                lists = Path(work)
                for route in (CANONICAL, MIT):
                    prefix = route.uri.split('://')[1].rstrip('/').replace('/', '_')
                    for suite in SUITES:
                        (lists / f'{prefix}_dists_{suite}_InRelease').write_bytes(b'signed fixture')

                def update(mirror, limit):
                    events.append(('update', mirror.name, limit))
                    date = NOW - timedelta(hours=8) if mirror.name == 'Canonical' else final_date
                    with patch('linux_webview_apt_mirrors.release_date', return_value=date):
                        verify_fresh_indexes(mirror, lists)

                before_install = [('update', 'Canonical', DIRECT_UPDATE_DEADLINE), 'discard archives',
                                  ('update', 'MIT', DIRECT_UPDATE_DEADLINE)]
                if final_date == NOW:
                    with acquire(update, *args[1:], (CANONICAL, older)) as ready:
                        self.assertEqual(ready, (MIT, True))
                        events.append('normal install')
                    self.assertEqual(events, [*before_install, ('fresh plan', 'MIT'),
                                              ('download-only', 'MIT', DIRECT_DOWNLOAD_DEADLINE), 'normal install'])
                else:
                    with self.assertRaisesRegex(AptIndexRegression, 'MIT noble regressed'), acquire(
                            update, *args[1:], (CANONICAL, older)):
                        self.fail('the fallback cannot install from an index below the first route’s floor')
                    self.assertEqual(events, before_install)

    def test_signed_index_regression_cannot_fall_back_to_an_unverified_route(self):
        args, events = self.fixture(fail_update=AptIndexRegression('signed index regression'))
        with self.assertRaises(AptIndexRegression), acquire(*args, (CANONICAL, None)):
            self.fail('cannot install after index regression without another verified preflight')
        self.assertEqual(events, [('update', 'Canonical', DIRECT_UPDATE_DEADLINE)])

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

    def test_preflight_and_authoritative_refresh_observe_the_same_apt_transport(self):
        # Hosted 43102930045: curl saw a newer signed Security snapshot on
        # one hostname, but both Apt mirror requests saw the prior version.
        release = b'apt-mutable-pockets'
        fresh_curl = b'other-http-client-mutable-pockets'
        distant = b'lagging-mirror-pockets'
        rolled_back = b'rolled-back-after-apt-preflight'
        dates = {
            release: NOW - timedelta(hours=4),
            fresh_curl: NOW,
            distant: NOW - timedelta(hours=8),
            rolled_back: NOW - timedelta(hours=5),
        }
        comparable = (CANONICAL.uri, DIRECT_MIRRORS[2][1])

        def fetch(command, **options):
            if command[0] == 'curl':  # the old, mismatching implementation
                uri = command[-1]
                body = (fresh_curl if uri == f'{CANONICAL.uri}dists/noble-security/InRelease'
                        else release if uri.startswith(comparable) else distant)
                return subprocess.CompletedProcess(command, 0, body + b'\n200')
            tool = '/usr/lib/apt/apt-helper'
            self.assertIn(tool, command)
            self.assertIn('Acquire::https::No-Cache=true', command)
            self.assertIn('Acquire::https::AllowRedirect=false', command)
            self.assertIn('Acquire::Retries=0', command)
            self.assertIn(f'{PROBE_TIMEOUT}s', command)
            self.assertIn(f'--fsize={MAX_RELEASE}:{MAX_RELEASE}', command)
            self.assertEqual(command[-3], 'download-file')
            self.assertEqual(options['timeout'], PROBE_TIMEOUT + 3)
            uri, path = command[-2:]
            Path(path).write_bytes(release if uri.startswith(comparable) else distant)
            return subprocess.CompletedProcess(command, 0)

        for genuine_regression in (False, True):
            with self.subTest(genuine_regression=genuine_regression), tempfile.TemporaryDirectory() as work:
                lists = Path(work)
                args, events = self.fixture()

                def update(mirror, limit):
                    events.append(('update', mirror.name, limit))
                    prefix = mirror.uri.split('://')[1].rstrip('/').replace('/', '_')
                    for suite in SUITES:
                        body = rolled_back if genuine_regression and suite == 'noble-security' else release
                        (lists / f'{prefix}_dists_{suite}_InRelease').write_bytes(body)
                    verify_fresh_indexes(mirror, lists)

                with patch('linux_webview_apt_mirrors.subprocess.run', side_effect=fetch), patch(
                        'linux_webview_apt_mirrors.release_date', side_effect=lambda data, *_: dates[data]):
                    routes = preflight()
                    self.assertEqual({route.uri for route in routes}, set(comparable))
                    if genuine_regression:
                        with self.assertRaises(AptIndexRegression), acquire(update, *args[1:], routes):
                            self.fail('must not install with a real rollback after Apt preflight')
                        self.assertEqual([event[0] for event in events if isinstance(event, tuple)],
                                         ['update', 'update'])
                    else:
                        with acquire(update, *args[1:], routes) as ready:
                            self.assertEqual(ready, (routes[0], True))
                        self.assertEqual([event[0] for event in events if isinstance(event, tuple)],
                                         ['update', 'fresh plan', 'download-only'])

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
