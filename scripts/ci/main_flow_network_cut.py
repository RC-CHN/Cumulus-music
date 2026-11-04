"""External, one-shot network barrier for the synthetic Linux desktop probe."""

from contextlib import contextmanager
import json
import os
from pathlib import Path
import socket
import struct
import subprocess
import threading


def loopback(up):
    links = json.loads(subprocess.check_output(['/usr/sbin/ip', '-j', 'link'], timeout=2))
    if [link['ifname'] for link in links] != ['lo'] or ('UP' in links[0]['flags']) != up:
        raise RuntimeError('unexpected isolated link state')
    for family in ('-4', '-6'):
        routes = subprocess.check_output(['/usr/sbin/ip', family, '-j', 'route', 'show', 'table', 'main'], timeout=2)
        if json.loads(routes):
            raise RuntimeError('unexpected isolated network route')


def unable_to_connect(port):
    try:
        connection = socket.create_connection(('127.0.0.1', port), timeout=.6)
    except OSError:
        return
    with connection:
        raise RuntimeError('a fresh TCP connection survived the isolated link cutoff')


def isolated_namespaces(source_pid):
    # The outer Python launcher remains an actual ancestor across sudo,
    # unshare and runuser. A caller-provided namespace string alone cannot
    # prove that this process is outside the invoking network namespace.
    ancestor = os.getppid()
    while ancestor > 1 and ancestor != source_pid:
        status = Path(f'/proc/{ancestor}/status').read_text(encoding='utf-8')
        ancestor = int(next(line.split(':', 1)[1] for line in status.splitlines() if line.startswith('PPid:')))
    if ancestor <= 1 or ancestor != source_pid:
        raise RuntimeError('network cutoff requires its actual outer launcher')
    source_namespace = Path(f'/proc/{source_pid}/ns/net').readlink().name
    current = Path('/proc/self/ns/net').readlink().name
    if current == source_namespace:
        raise RuntimeError('refusing to alter the invoking network namespace')
    return current, source_namespace


def cut(source_pid):
    current, source_namespace = isolated_namespaces(source_pid)
    loopback(up=True)
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        listener.listen(1)
        listener.settimeout(.6)
        port = listener.getsockname()[1]
        with socket.create_connection(('127.0.0.1', port), timeout=.6) as client:
            accepted, _ = listener.accept()
            with accepted:
                accepted.settimeout(.6)
                client.sendall(b'B')
                if accepted.recv(1) != b'B':
                    raise RuntimeError('established TCP witness failed before cutoff')
                # Retain both endpoints and the listener. The network itself
                # must stop traffic; merely closing a socket cannot pass.
                subprocess.run([
                    '/usr/bin/sudo', '-n', '/bin/sh', '-ec',
                    'test "$(/usr/bin/readlink /proc/self/ns/net)" = "$1"; '
                    'test "$(/usr/bin/readlink "/proc/$3/ns/net")" = "$2"; '
                    'test "$1" != "$2"; exec /usr/sbin/ip link set dev lo down',
                    'isolated-loopback-cut', current, source_namespace, str(source_pid),
                ], check=True, timeout=3)
                loopback(up=False)
                try:
                    client.sendall(b'A')
                    after = accepted.recv(1)
                except OSError:
                    after = b''
                if after:
                    raise RuntimeError('established TCP traffic survived the isolated link cutoff')
                unable_to_connect(port)
                unable_to_connect(443)
    return dict(loopbackDown=True, noIpv4OrIpv6Route=True,
                establishedTcpBlocked=True, newTcpBlocked=True, fixtureHttpsBlocked=True)


@contextmanager
def controller(path, source_pid):
    """Only the local capture driver can release the download-stage barrier."""
    isolated_namespaces(source_pid)
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as control:
        control.bind(str(path))
        control.listen(1)
        control.settimeout(.1)
        stop = threading.Event()
        result = {}
        failures = []

        def run():
            while not stop.is_set():
                try:
                    connection, _ = control.accept()
                except TimeoutError:
                    continue
                try:
                    with connection:
                        connection.settimeout(1)
                        size = struct.calcsize('3i')
                        peer, _, _ = struct.unpack('3i', connection.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, size))
                        if Path(f'/proc/{peer}/ns/net').readlink() != Path('/proc/self/ns/net').readlink():
                            raise RuntimeError('capture driver and fixture do not share the isolated network')
                        if connection.recv(1) != b'C':
                            raise RuntimeError('unexpected cutoff request')
                        result.update(cut(source_pid))
                        result['nativeHostAndFixtureSharedNamespace'] = True
                        connection.sendall(b'Y')
                except Exception as error:
                    failures.append(error)
                return

        worker = threading.Thread(target=run, daemon=True)
        worker.start()
        try:
            yield result
            if failures:
                raise RuntimeError('isolated network cutoff failed') from failures[0]
            if not result:
                raise RuntimeError('download checkpoint did not trigger isolated network cutoff')
            loopback(up=False)
        finally:
            stop.set()
            worker.join(timeout=8)
