"""An isolated restart can own a different window manager for each launch."""
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

import window_manager


@unittest.skipIf(os.name == 'nt', 'the explicit Openbox launcher is Linux-only')
class WindowManagerLogs(unittest.TestCase):
    def test_reopen_keeps_both_managers_logs_with_sibling_host_logs(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            manager = Mock()
            manager.poll.return_value = None

            def start(_argv, *, env, stdout, stderr):
                self.assertIs(stdout, stderr)
                stdout.write(f'{env["PHASE"]} manager\n')
                return manager

            with (
                patch.dict(os.environ, {'CUMULUS_TEST_WINDOW_MANAGER': 'openbox'}),
                patch.object(window_manager, 'ManagerCheck') as check,
                patch.object(window_manager.subprocess, 'Popen', side_effect=start) as spawn,
                patch.object(window_manager.motion_smoke, 'launch', return_value=0) as app,
            ):
                check.return_value.owner.return_value = 0
                check.return_value.ready.return_value = True
                for phase in ('first', 'reopen'):
                    with (root / f'{phase}.log').open('x', encoding='utf-8') as log:
                        self.assertEqual(window_manager.launch(
                            'probe', {'DISPLAY': ':1', 'PHASE': phase}, log, root / phase, False,
                        ), 0)
                self.assertEqual(spawn.call_count, 2)
                self.assertEqual(app.call_count, 2)
                self.assertEqual(manager.terminate.call_count, 2)
                self.assertEqual(check.return_value.close.call_count, 2)
            self.assertEqual(
                {path.name: path.read_text(encoding='utf-8')
                 for path in root.glob('*.window-manager.log')},
                {'first.log.window-manager.log': 'first manager\n',
                 'reopen.log.window-manager.log': 'reopen manager\n'},
            )
