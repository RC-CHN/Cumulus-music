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
    AZURE_FIRST, CANONICAL_FIRST, DOWNLOAD_DEADLINE, UPDATE_DEADLINE,
    AptDeadline, acquire, canonical_priority, deadline_command, is_signed_noble_source,
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

        @contextmanager
        def alternate():
            events.append('canonical first')
            try:
                yield
            finally:
                events.append('original priorities restored')

        args = (operation('update', fail_update), lambda: events.append('fresh cache plan'),
                operation('download-only', fail_download), lambda: events.append('discard archives'))
        return args, alternate, events

    def test_download_timeout_revalidates_in_fresh_cache_before_single_install(self):
        args, alternate, events = self.fixture(fail_download=AptDeadline('download', DOWNLOAD_DEADLINE))
        with acquire(*args, alternate) as ready:
            self.assertTrue(ready)
            events.append('normal install')
        self.assertEqual(events, [
            ('update', UPDATE_DEADLINE), 'fresh cache plan', ('download-only', DOWNLOAD_DEADLINE),
            'discard archives', 'canonical first', ('update', UPDATE_DEADLINE), 'fresh cache plan',
            ('download-only', DOWNLOAD_DEADLINE), 'normal install', 'original priorities restored',
        ])

    def test_update_timeout_never_admits_old_plan_and_unknown_preset_keeps_normal_apt(self):
        args, alternate, events = self.fixture(fail_update=AptDeadline('update', UPDATE_DEADLINE))
        with acquire(*args, alternate):
            events.append('normal install')
        self.assertEqual(events[:5], [('update', UPDATE_DEADLINE), 'discard archives',
                                     'canonical first', ('update', UPDATE_DEADLINE), 'fresh cache plan'])
        args, _, events = self.fixture()
        with acquire(*args, None) as ready:
            self.assertFalse(ready)
            events.append('normal install')
        self.assertEqual(events, [('update', None), 'fresh cache plan', 'normal install'])

    def test_fast_azure_path_does_not_touch_mirror_configuration(self):
        args, alternate, events = self.fixture()
        with acquire(*args, alternate) as ready:
            self.assertTrue(ready)
            events.append('normal install')
        self.assertEqual(events, [('update', UPDATE_DEADLINE), 'fresh cache plan',
                                  ('download-only', DOWNLOAD_DEADLINE), 'normal install'])

    def test_signature_or_other_apt_failure_is_not_treated_as_a_timeout(self):
        args, alternate, events = self.fixture(fail_update=subprocess.CalledProcessError(100, 'apt-get'))
        with self.assertRaises(subprocess.CalledProcessError), acquire(*args, alternate):
            self.fail('cannot install after an APT failure')
        self.assertEqual(events, [('update', UPDATE_DEADLINE)])

    def test_second_timeout_stops_without_install_or_second_retry(self):
        args, alternate, events = self.fixture()

        def timeout(limit):
            events.append(('download-only', limit))
            raise AptDeadline('download', limit)

        with self.assertRaises(AptDeadline), acquire(args[0], args[1], timeout, args[3], alternate):
            self.fail('cannot install after a second timeout')
        self.assertEqual(events.count('canonical first'), 1)
        self.assertEqual(events[-1], 'original priorities restored')

    def test_alternate_keeps_both_mirrors_and_restores_bytes_after_failure(self):
        with tempfile.TemporaryDirectory() as work:
            root = Path(work)
            path = root / 'mirrors.txt'
            path.write_bytes(AZURE_FIRST)

            def fake_cp(command, **kwargs):
                self.assertEqual(command[:2], ['sudo', 'cp'])
                shutil.copyfile(command[-2], command[-1])

            with patch('linux_webview_apt_mirrors.MIRRORS', path), patch(
                    'linux_webview_apt_mirrors.subprocess.run', side_effect=fake_cp):
                with self.assertRaisesRegex(RuntimeError, 'failed install'), canonical_priority(root):
                    self.assertEqual(path.read_bytes(), CANONICAL_FIRST)
                    raise RuntimeError('failed install')
            self.assertEqual(path.read_bytes(), AZURE_FIRST)
        self.assertEqual([line.split(b'\t')[0] for line in AZURE_FIRST.splitlines()],
                         [line.split(b'\t')[0] for line in CANONICAL_FIRST.splitlines()])

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
