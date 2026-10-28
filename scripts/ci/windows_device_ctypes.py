"""Independent libmpv ABI probe on a synthetic Windows CI runner only.

No device is opened for playback and no track/account/network input is used.
Compare a single synchronous get with the Rust async event adapter. A crash
before event parsing distinguishes library/driver failures from our node copy.
"""
import ctypes as c
from pathlib import Path
import sys


def stage(name):
    print(name, flush=True)


if __name__ == '__main__':
    assert sys.platform == 'win32'
    runtime = Path(sys.argv[1]).resolve()
    import os
    with os.add_dll_directory(str(runtime)):
        lib = c.CDLL(str(runtime / 'libmpv-2.dll'))
        lib.mpv_create.argtypes = []
        lib.mpv_create.restype = c.c_void_p
        lib.mpv_set_option_string.argtypes = [c.c_void_p, c.c_char_p, c.c_char_p]
        lib.mpv_set_option_string.restype = c.c_int
        lib.mpv_initialize.argtypes = [c.c_void_p]
        lib.mpv_initialize.restype = c.c_int
        lib.mpv_get_property_string.argtypes = [c.c_void_p, c.c_char_p]
        lib.mpv_get_property_string.restype = c.c_void_p
        lib.mpv_free.argtypes = [c.c_void_p]
        lib.mpv_free.restype = None
        lib.mpv_terminate_destroy.argtypes = [c.c_void_p]
        lib.mpv_terminate_destroy.restype = None
        stage('ctypes:create')
        handle = lib.mpv_create()
        assert handle
        for name, value in [(b'config', b'no'), (b'vo', b'null'), (b'idle', b'yes'), (b'volume', b'0')]:
            assert lib.mpv_set_option_string(handle, name, value) >= 0
        stage('ctypes:initialize')
        assert lib.mpv_initialize(handle) >= 0
        stage('ctypes:query_audio_device_list')
        data = lib.mpv_get_property_string(handle, b'audio-device-list')
        stage('ctypes:query_returned')
        assert data
        # Deliberately do not parse nodes or expose device names.
        lib.mpv_free(data)
        stage('ctypes:destroy')
        lib.mpv_terminate_destroy(handle)
        stage('ctypes:complete')
