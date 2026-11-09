"""Real Linux GTK folder chooser, WebView, native index, and guest null playback."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import sys
import time

from installed_linux_audio import Input, start_accessibility
from window_manager import launch

STAGES = (
    'picker_requested', 'unchecked_after_cancel', 'checked', 'paused_root',
    'deselected', 'rechecked',
)
CHOOSER_TITLE = '选择本地音乐目录'


def wait(check, description):
    deadline = time.monotonic() + 6
    while time.monotonic() < deadline:
        value = check()
        if value:
            return value
        time.sleep(.075)
    raise AssertionError(description)


def children(node):
    return [child for i in range(node.get_child_count())
            if (child := node.get_child_at_index(i)) is not None]


def native_choose(atspi, pid, selected):
    from gi.repository import GLib

    def chooser():
        apps = [app for app in children(atspi.get_desktop(0)) if app.get_process_id() == pid]
        for app in apps:
            for window in children(app):
                if window.get_name() == CHOOSER_TITLE and window.get_state_set().contains(atspi.StateType.SHOWING):
                    return window
        return None

    window = wait(chooser, 'the exact probe-owned GTK folder chooser was not visible')
    role = window.get_role_name()
    xinput = Input(0, 0)
    try:
        xinput.key('l', 'Control_L')

        def location_entry():
            todo = [chooser()]
            visited = 0
            while todo:
                node = todo.pop()
                if node is None:
                    continue
                visited += 1
                assert visited <= 512, 'unexpectedly large GTK chooser subtree'
                if (node.get_state_set().contains(atspi.StateType.FOCUSED)
                        and (editor := node.get_editable_text_iface()) is not None):
                    return editor
                todo.extend(children(node))
            return None

        editor = wait(location_entry, 'GTK chooser did not focus its path field')
        assert editor.set_text_contents(os.fspath(selected)), 'native GTK path field rejected the fixture'
        xinput.key('Return')

        def closed():
            try:
                return chooser() is None
            except GLib.Error:
                return False

        wait(closed, 'GTK chooser did not confirm the selected folder')
    finally:
        xinput.close()
    return {'title': CHOOSER_TITLE, 'role': role, 'probePid': pid, 'fixture': selected.name}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    args = parser.parse_args()
    if sys.platform != 'linux':
        parser.error('this proof uses Linux native GTK chooser and X11')
    atspi = start_accessibility()
    root = args.root.resolve()
    assert all(ch not in os.fspath(root) for ch in ('"', '\\', '\n', '$'))
    root.mkdir(parents=True, exist_ok=False)
    target = root / 'run'
    os_root = root / 'os-profile'
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(target), CUMULUS_LOCAL_DIRECTORY_SELECTION_PROOF='1',
               GTK_USE_PORTAL='0')
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR'):
        profile = os_root / key
        profile.mkdir(parents=True, mode=0o700)
        env[key] = str(profile)
    # Direct the native audio-dir default into the sandbox before any chooser
    # opens. Captures never need to inspect the real user's home/library.
    (Path(env['XDG_CONFIG_HOME']) / 'user-dirs.dirs').write_text(
        f'XDG_MUSIC_DIR="{target}"\n', encoding='utf-8')
    chooser = []
    source_before = []

    def source_hashes():
        return [(path.name, hashlib.sha256(path.read_bytes()).hexdigest())
                for path in sorted((target / 'media').iterdir())]

    def before_ack(stage, _, pid):
        if stage.stem == 'ux_01_picker_requested':
            source_before.extend(source_hashes())
            chooser.append(native_choose(atspi, pid, target / 'media'))

    with (root / 'host.log').open('x', encoding='utf-8') as log:
        status = launch(args.binary.resolve(strict=True), env, log, target, True, before_ack=before_ack)
    failed = target / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert status == 0, f'proof exited {status}'
    completed = json.loads((target / 'complete.json').read_text(encoding='utf-8'))
    assert completed['ok'] and completed['stages'] == len(STAGES) and len(chooser) == 1
    assert completed['realMusicRequests'] == completed['realAccountWrites'] == 0
    rows = [json.loads((target / f'ux_{i:02d}_{stage}.json').read_text(encoding='utf-8'))
            for i, stage in enumerate(STAGES, 1)]
    assert [row['count'] for row in rows] == [0, 0, 3, 3, 0, 3]
    assert [row['roots'] for row in rows] == [[], *[
        [{'checked': checked, 'paused': paused}]
        for checked, paused in [(False, False), (True, False), (True, True), (False, False), (True, False)]
    ]]
    assert all(row['stable'] for row in rows[3:])
    assert len(list(target.glob('ux_*.png'))) == len(STAGES)
    locale = json.loads((target / 'native-startup.json').read_text(encoding='utf-8'))
    assert locale['afterGuiInitialization'] and locale['numericLocale'] == 'C'
    assert locale['messageLocale'] == env['LC_ALL'] and env['LC_ALL'] not in ('C', 'POSIX')
    sources = sorted((target / 'media').iterdir())
    assert {path.name for path in sources} == {'album.cue', 'album.wav', '雨声.wav'}
    original = source_hashes()
    assert original == source_before, 'ordinary audio and CUE source bytes remain unchanged'
    result = dict(ok=True, actualWebView=True, nativeNullOutput=True, screens=len(STAGES),
                  counts=[row['count'] for row in rows], picker=chooser[0], locale=locale,
                  sources=original, pausedPlaybackRetained=True, realAccountWrites=0)
    (root / 'report.json').write_text(json.dumps(result, indent=2, ensure_ascii=False) + '\n', encoding='utf-8')
    print(json.dumps(result, ensure_ascii=False))


if __name__ == '__main__':
    main()
