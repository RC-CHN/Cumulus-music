"""Independent libmpv C ABI probe, only on the synthetic Windows CI runner.

Compare one/two cores and synchronous/asynchronous device queries without Rust.
Unguarded async modes reproduce the bundled DLL fault; -mta modes hold a COM
usage lease through native teardown. Never load media or print device names.
"""
import ctypes as c
import json
import os
from pathlib import Path
import sys
import time


class Event(c.Structure):
    _fields_ = [('event_id', c.c_int), ('error', c.c_int),
                ('reply_userdata', c.c_uint64), ('data', c.c_void_p)]


class Value(c.Union):
    _fields_ = [('string', c.c_void_p), ('opaque', c.c_uint64)]


class Node(c.Structure):
    _fields_ = [('value', Value), ('format', c.c_int)]


class CommandReply(c.Structure):
    _fields_ = [('result', Node)]


def stage(name):
    print(name, flush=True)


def bind(lib, name, args, result):
    function = getattr(lib, name)
    function.argtypes = args
    function.restype = result
    return function


def run(runtime, mode):
    query_mode = mode.removesuffix('-mta')
    use_mta = mode.endswith('-mta')
    ole32 = c.WinDLL('ole32')
    increment = bind(ole32, 'CoIncrementMTAUsage', [c.POINTER(c.c_void_p)], c.c_int32)
    decrement = bind(ole32, 'CoDecrementMTAUsage', [c.c_void_p], c.c_int32)
    leases = {}
    lib = c.CDLL(str(runtime / 'libmpv-2.dll'))
    create = bind(lib, 'mpv_create', [], c.c_void_p)
    option = bind(lib, 'mpv_set_option_string', [c.c_void_p, c.c_char_p, c.c_char_p], c.c_int)
    initialize = bind(lib, 'mpv_initialize', [c.c_void_p], c.c_int)
    get = bind(lib, 'mpv_get_property_string', [c.c_void_p, c.c_char_p], c.c_void_p)
    free = bind(lib, 'mpv_free', [c.c_void_p], None)
    destroy = bind(lib, 'mpv_terminate_destroy', [c.c_void_p], None)
    query = bind(lib, 'mpv_get_property_async',
                 [c.c_void_p, c.c_uint64, c.c_char_p, c.c_int], c.c_int)
    command = bind(lib, 'mpv_command_async',
                   [c.c_void_p, c.c_uint64, c.POINTER(c.c_char_p)], c.c_int)
    wait = bind(lib, 'mpv_wait_event', [c.c_void_p, c.c_double], c.POINTER(Event))

    def open_core(null_output):
        cookie = c.c_void_p()
        if use_mta:
            assert increment(c.byref(cookie)) >= 0 and cookie.value
            stage(f'{mode}:mta_acquired')
        handle = create()
        assert handle
        if use_mta:
            leases[handle] = cookie
        for name, value in [
            (b'config', b'no'), (b'load-scripts', b'no'), (b'ytdl', b'no'),
            (b'terminal', b'no'), (b'msg-level', b'all=no'), (b'idle', b'yes'),
            (b'keep-open', b'no'), (b'pause', b'yes'), (b'audio-display', b'no'),
            (b'vo', b'null'), (b'network-timeout', b'15'), (b'cookies', b'no'),
            (b'user-agent', b'Cumulus/0.1'), (b'load-unsafe-playlists', b'no'),
            (b'volume', b'0'),
        ]:
            assert option(handle, name, value) >= 0
        if null_output:
            assert option(handle, b'ao', b'null') >= 0
        assert initialize(handle) >= 0
        return handle

    def close_core(handle):
        destroy(handle)
        if use_mta:
            assert decrement(leases.pop(handle)) >= 0
            stage(f'{mode}:mta_released')

    stage(f'{mode}:create')
    primary = open_core(True) if mode.startswith('pair-') else None
    handle = open_core(False)
    stage(f'{mode}:initialized')

    def reply(event_id, sequence):
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            event = wait(handle, .1).contents
            if event.event_id == event_id and event.reply_userdata == sequence:
                assert event.error >= 0
                return event
        raise TimeoutError('libmpv did not reply')

    sequence = 0
    for index in range(4):
        sequence += 1
        stage(f'{mode}:query_{index}')
        if query_mode.endswith('-command'):
            args = (c.c_char_p * 3)(b'expand-text', b'${=audio-device-list}', None)
            assert command(handle, sequence, args) >= 0
            event = reply(5, sequence)
            assert event.data
            result = c.cast(event.data, c.POINTER(CommandReply)).contents.result
            assert result.format == 1 and result.value.string
            # Copy the event-owned result before the next wait. Never free it.
            devices = json.loads(c.string_at(result.value.string))
            assert isinstance(devices, list) and 0 < len(devices) <= 1024
            assert all(isinstance(d['name'], str) and isinstance(d['description'], str)
                       for d in devices)
            assert any(d['name'] == 'auto' for d in devices)
        elif 'async' in mode:
            assert query(handle, sequence, b'audio-device-list', 6) >= 0
            reply(3, sequence)
            # Leave the MPV_FORMAT_NODE allocation untouched and mpv-owned.
        else:
            data = get(handle, b'audio-device-list')
            assert data
            free(data)
        stage(f'{mode}:query_{index}_returned')
        data = get(handle, b'audio-device')
        assert data
        free(data)
        if index == 1:
            sequence += 1
            args = (c.c_char_p * 4)(b'set', b'audio-device', b'auto', None)
            assert command(handle, sequence, args) >= 0
            reply(5, sequence)
            stage(f'{mode}:auto_selected')
    if mode.endswith('-drain'):
        for _ in range(100):
            if wait(handle, 0).contents.event_id == 0:
                break
        else:
            raise AssertionError('event queue did not drain')
        stage(f'{mode}:events_drained')
    stage(f'{mode}:destroy_enumerator')
    close_core(handle)
    stage(f'{mode}:enumerator_destroyed')
    if primary:
        close_core(primary)
        stage(f'{mode}:primary_destroyed')
    # ctypes retains the DLL normally. Explicit unload checks the same boundary
    # as Rust Library::drop after every native core is already destroyed.
    unload = bind(c.WinDLL('kernel32', use_last_error=True), 'FreeLibrary', [c.c_void_p], c.c_int)
    stage(f'{mode}:unload_library')
    assert unload(lib._handle)
    stage(f'{mode}:complete')


if __name__ == '__main__':
    assert sys.platform == 'win32'
    runtime = Path(sys.argv[1]).resolve()
    mode = sys.argv[2] if len(sys.argv) > 2 else 'single-sync'
    assert mode in ['single-sync', 'pair-sync', 'single-command', 'pair-command',
                    'single-async', 'pair-async', 'pair-async-drain',
                    'single-command-mta', 'pair-command-mta',
                    'single-async-mta', 'pair-async-mta']
    with os.add_dll_directory(str(runtime)):
        run(runtime, mode)
