"""Separate OS key producer for the mini-player WebView proof; no business IPC."""
import json
import os
import subprocess
import sys
import time


def launch(binary, env, log, output, capture):
    windows = sys.platform == 'win32'
    if windows:
        from shortcut_windows_keys import Keyboard
    else:
        from shortcut_smoke import Keyboard
    keyboard = Keyboard()
    if capture:
        import gi
        gi.require_version('Gdk', '3.0')
        from gi.repository import Gdk
    seen, sent = set(), []
    # Permission is granted by the foreground fixture to this exact producer,
    # never ASFW_ANY. Keys still come from independent SendInput after native
    # GetForegroundWindow proves our own foreign window really owns input.
    if windows:
        env = {**env, 'CUMULUS_KEY_PRODUCER_PID': str(os.getpid())}
    process = subprocess.Popen([str(binary)], env=env, stdout=log, stderr=log)
    try:
        deadline = time.monotonic() + 180
        while process.poll() is None:
            if time.monotonic() >= deadline:
                raise TimeoutError('mini shortcut WebView did not finish')
            if windows:
                keyboard.drain()
            for path in sorted(output.glob('key-*.json')):
                reply = path.with_suffix('.done')
                if reply.exists():
                    continue
                value = json.loads(path.read_text(encoding='utf-8'))
                assert value['key'] in ['q', 'm', 'p'], value
                if windows:
                    grant_path = path.with_name('foreground-' + path.stem.split('-', 1)[1] + '.json')
                    grant = json.loads(grant_path.read_text(encoding='utf-8'))
                    assert grant['producerPid'] == os.getpid() and grant['fixturePid'] == process.pid
                    assert grant['foregroundPid'] in [0, process.pid, os.getpid()]
                    assert grant['granted'] or grant['foregroundPid'] == os.getpid()
                keyboard.chord(value['key'])
                sent.append(value['key'])
                reply.write_text('sent', encoding='utf-8')
            if capture:
                for path in output.glob('ux_*.json'):
                    if path in seen:
                        continue
                    window = Gdk.get_default_root_window()
                    picture = Gdk.pixbuf_get_from_window(window, 0, 0, window.get_width(), window.get_height())
                    assert picture is not None
                    picture.savev(str(path.with_suffix('.png')), 'png', [], [])
                    seen.add(path)
            time.sleep(.05)
        if process.returncode == 0:
            assert sent == ['q', 'm', 'q', 'p', 'p', 'q'], sent
        return process.returncode
    finally:
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
        keyboard.close()
