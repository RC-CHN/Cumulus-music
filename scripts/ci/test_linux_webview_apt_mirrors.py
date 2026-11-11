"""Check watchdog and fallback choreography without changing host APT."""

from contextlib import contextmanager
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from linux_webview_apt import timed
from linux_webview_apt_mirrors import (
    AZURE_FIRST, CANONICAL_FIRST, DOWNLOAD_DEADLINE, SECURITY_FIRST, UPDATE_DEADLINE,
    AptDeadline, acquire, canonical_priority, deadline_command, is_signed_noble_source,
    security_priority,
)


SOURCE = '''Types: deb
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


class LinuxWebViewAptMirrors(unittest.TestCase):
    def fixture(self, fail_update=None, fail_download=None):
        events = []

        def operation(label, fail):
            def call(limit):
                events.append((label, limit))
                if fail is not None and events.count((label, limit)) == 1:
                    raise fail
            return call

        def alternate(name):
            @contextmanager
            def context():
                events.append(f'{name} first')
                try:
                    yield
                finally:
                    events.append(f'{name} priorities restored')
            return context

        args = (operation('update', fail_update), lambda: events.append('fresh cache plan'),
                operation('download-only', fail_download), lambda: events.append('discard archives'))
        return args, alternate('security'), alternate('canonical'), events

    def test_download_timeout_revalidates_in_fresh_cache_before_single_install(self):
        args, security, canonical, events = self.fixture(
            fail_download=AptDeadline('download', DOWNLOAD_DEADLINE))
        with acquire(*args, security, canonical) as ready:
            self.assertTrue(ready)
            events.append('normal install')
        self.assertEqual(events, [
            ('update', UPDATE_DEADLINE), 'fresh cache plan', ('download-only', DOWNLOAD_DEADLINE),
            'discard archives', 'security first', ('update', UPDATE_DEADLINE), 'fresh cache plan',
            ('download-only', DOWNLOAD_DEADLINE), 'normal install', 'security priorities restored',
        ])

    def test_update_timeout_never_admits_old_plan_and_unknown_preset_keeps_normal_apt(self):
        args, security, canonical, events = self.fixture(
            fail_update=AptDeadline('update', UPDATE_DEADLINE))
        with acquire(*args, security, canonical):
            events.append('normal install')
        self.assertEqual(events[:5], [('update', UPDATE_DEADLINE), 'discard archives',
                                     'security first', ('update', UPDATE_DEADLINE), 'fresh cache plan'])
        args, _, _, events = self.fixture()
        with acquire(*args, None) as ready:
            self.assertFalse(ready)
            events.append('normal install')
        self.assertEqual(events, [('update', None), 'fresh cache plan', 'normal install'])

    def test_fast_azure_path_does_not_touch_mirror_configuration(self):
        args, security, canonical, events = self.fixture()
        with acquire(*args, security, canonical) as ready:
            self.assertTrue(ready)
            events.append('normal install')
        self.assertEqual(events, [('update', UPDATE_DEADLINE), 'fresh cache plan',
                                  ('download-only', DOWNLOAD_DEADLINE), 'normal install'])

    def test_signature_or_other_apt_failure_is_not_treated_as_a_timeout(self):
        args, security, canonical, events = self.fixture(
            fail_update=subprocess.CalledProcessError(100, 'apt-get'))
        with self.assertRaises(subprocess.CalledProcessError), acquire(*args, security, canonical):
            self.fail('cannot install after an APT failure')
        self.assertEqual(events, [('update', UPDATE_DEADLINE)])

    def test_second_timeout_restores_security_then_tries_canonical_once(self):
        args, security, canonical, events = self.fixture()

        def download(limit):
            events.append(('download-only', limit))
            if events.count(('download-only', limit)) < 3:
                raise AptDeadline('download', limit)

        with acquire(args[0], args[1], download, args[3], security, canonical) as ready:
            self.assertTrue(ready)
            events.append('normal install')
        self.assertEqual(events, [
            ('update', UPDATE_DEADLINE), 'fresh cache plan', ('download-only', DOWNLOAD_DEADLINE),
            'discard archives', 'security first', ('update', UPDATE_DEADLINE), 'fresh cache plan',
            ('download-only', DOWNLOAD_DEADLINE), 'security priorities restored',
            'discard archives', 'canonical first', ('update', UPDATE_DEADLINE), 'fresh cache plan',
            ('download-only', DOWNLOAD_DEADLINE), 'normal install', 'canonical priorities restored',
        ])

    def test_final_timeout_stops_without_install_or_fourth_attempt(self):
        args, security, canonical, events = self.fixture()

        def timeout(limit):
            events.append(('download-only', limit))
            raise AptDeadline('download', limit)

        with self.assertRaises(AptDeadline), acquire(
                args[0], args[1], timeout, args[3], security, canonical):
            self.fail('cannot install after the third timeout')
        self.assertEqual(events.count(('download-only', DOWNLOAD_DEADLINE)), 3)
        self.assertEqual(events.count('discard archives'), 2)
        self.assertEqual(events.count('security first'), 1)
        self.assertEqual(events.count('canonical first'), 1)
        self.assertEqual(events[-1], 'canonical priorities restored')

    def test_fallback_signature_error_or_install_failure_never_reaches_next_mirror(self):
        for fail_install in (False, True):
            with self.subTest(fail_install=fail_install):
                args, security, canonical, events = self.fixture()

                def update(limit):
                    events.append(('update', limit))
                    if 'security first' not in events:
                        raise AptDeadline('update', limit)
                    if not fail_install:
                        raise subprocess.CalledProcessError(100, 'apt-get')

                # Even an AptDeadline from outside acquisition is not
                # classified for a retry, since installation may have begun.
                error = AptDeadline if fail_install else subprocess.CalledProcessError
                with self.assertRaises(error), acquire(
                        update, args[1], args[2], args[3], security, canonical):
                    events.append('normal install')
                    raise AptDeadline('install fixture', 1)
                self.assertNotIn('canonical first', events)
                self.assertEqual(events.count('discard archives'), 1)
                self.assertEqual(events[-1], 'security priorities restored')

    def test_alternates_keep_all_mirrors_and_restore_bytes_after_failure(self):
        with tempfile.TemporaryDirectory() as work:
            root = Path(work)
            path = root / 'mirrors.txt'
            path.write_bytes(AZURE_FIRST)

            def fake_cp(command, **kwargs):
                self.assertEqual(command[:2], ['sudo', 'cp'])
                shutil.copyfile(command[-2], command[-1])

            with patch('linux_webview_apt_mirrors.MIRRORS', path), patch(
                    'linux_webview_apt_mirrors.subprocess.run', side_effect=fake_cp):
                for priority, preset in [(security_priority, SECURITY_FIRST),
                                         (canonical_priority, CANONICAL_FIRST)]:
                    with self.assertRaisesRegex(RuntimeError, 'failed install'), priority(root):
                        self.assertEqual(path.read_bytes(), preset)
                        raise RuntimeError('failed install')
                    self.assertEqual(path.read_bytes(), AZURE_FIRST)
            self.assertEqual(path.read_bytes(), AZURE_FIRST)
        urls = [line.split(b'\t')[0] for line in AZURE_FIRST.splitlines()]
        for preset in (CANONICAL_FIRST, SECURITY_FIRST):
            self.assertEqual(urls, [line.split(b'\t')[0] for line in preset.splitlines()])

    def test_only_runner_ubuntu_signed_mirror_stanzas_are_eligible(self):
        self.assertTrue(is_signed_noble_source(SOURCE))
        for other in (SOURCE.replace('noble-security', 'jammy-security'),
                      SOURCE.replace('Signed-By:', 'Trusted:'),
                      SOURCE.replace('mirror+file:', 'file:')):
            with self.subTest(source=other):
                self.assertFalse(is_signed_noble_source(other))

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
