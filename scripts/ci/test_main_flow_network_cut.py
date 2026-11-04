import os
from pathlib import Path
import sys
import unittest

from main_flow_network_cut import isolated_namespaces


@unittest.skipUnless(sys.platform.startswith('linux'), 'Linux network namespace guard')
class NetworkCutGuardTests(unittest.TestCase):
    def test_rejects_pid_that_is_not_an_outer_launcher(self):
        with self.assertRaisesRegex(RuntimeError, 'actual outer launcher'):
            isolated_namespaces(os.getpid())

    def test_rejects_an_outer_launcher_in_the_same_namespace(self):
        parent = os.getppid()
        if parent <= 1 or Path(f'/proc/{parent}/ns/net').readlink() != Path('/proc/self/ns/net').readlink():
            self.skipTest('requires an ordinary same-namespace parent')
        with self.assertRaisesRegex(RuntimeError, 'invoking network namespace'):
            isolated_namespaces(parent)
