"""Two pinned TLS authorities for the signed desktop proof; no command-line egress."""

from dataclasses import dataclass
import ipaddress
import os
from pathlib import Path
import selectors
import socket
import socketserver
import ssl
import stat
import struct
import threading
import time
from types import MappingProxyType

from signed_window_tls_fence import Denied, HOST, read_client_hello, read_connect, require

PC_HOST = b"interfacepc.music.163.com"
CDN_SUFFIXES = (b".music.126.net", b".music.163.com", b".netease.com", b".vod.126.net")
PC_DIALS = 25  # 16 playlist pages + the nine other allowed reads
MAX_ACTIVE = 4
MAX_SESSION_SECONDS = 60
MIB = 1024 * 1024


class _Disarmed(Exception):
    pass


def public_ipv4(text):
    """The reviewable destination is a single literal public IPv4 address."""
    require(isinstance(text, str) and len(text) <= 15)
    try:
        address = ipaddress.IPv4Address(text)
    except ValueError:
        raise Denied("proof broker requires a public IPv4 pin") from None
    require(str(address) == text and address.is_global and not address.is_multicast
            and not address.is_reserved and not address.is_unspecified and not address.is_loopback
            and not address.is_link_local)
    return address


@dataclass(frozen=True, repr=False)
class Pin:
    host: bytes
    address: ipaddress.IPv4Address

    @classmethod
    def pc(cls, address):
        return cls(PC_HOST, public_ipv4(address))

    @classmethod
    def cdn(cls, host, address):
        require(isinstance(host, str) and host.isascii() and len(host) <= 253)
        name = host.encode("ascii")
        require(name != PC_HOST and HOST.fullmatch(name) and name.endswith(CDN_SUFFIXES)
                and all(0 < len(label) <= 63 for label in name.split(b".")))
        return cls(name, public_ipv4(address))


def numeric_dial(address, seconds):
    """Connect directly to the reviewed pin; no resolver or ambient proxy."""
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    try:
        sock.settimeout(seconds)
        sock.connect((str(address), 443))
        return sock
    except OSError:
        sock.close()
        raise


def verify_certificate(pin):
    """Optional host-side pin preflight: TLS certificate and SNI, no HTTP."""
    context = ssl.create_default_context(cafile="/etc/ssl/certs/ca-certificates.crt")
    with numeric_dial(pin.address, 3) as raw:
        with context.wrap_socket(raw, server_hostname=pin.host.decode("ascii")):
            pass


class _UnixServer(socketserver.ThreadingUnixStreamServer):
    request_queue_size = MAX_ACTIVE
    daemon_threads = False
    block_on_close = True

    def __init__(self, path, broker):
        self.broker = broker
        self.slots = threading.BoundedSemaphore(MAX_ACTIVE)
        super().__init__(str(path), _Handler)

    def process_request(self, request, client_address):
        if not self.slots.acquire(blocking=False):
            self.broker.note_denial()
            self.shutdown_request(request)
            return
        try:
            super().process_request(request, client_address)
        except BaseException:
            self.slots.release()
            raise

    def process_request_thread(self, request, client_address):
        try:
            super().process_request_thread(request, client_address)
        finally:
            self.slots.release()

    def handle_error(self, _request, _client_address):
        # No platform errors, peer information or TLS details enter evidence.
        self.broker.note_denial()


class _Handler(socketserver.BaseRequestHandler):
    def handle(self):
        broker = self.server.broker
        try:
            _, uid, _ = struct.unpack("3i", self.request.getsockopt(
                socket.SOL_SOCKET, socket.SO_PEERCRED, struct.calcsize("3i")))
            require(uid == os.geteuid())
            broker.tunnel(self.request)
        except _Disarmed:
            return
        except (Denied, OSError):
            broker.note_denial()


class Broker:
    def __init__(self, directory, pc, cdn=None, *, seconds=MAX_SESSION_SECONDS, dial=numeric_dial):
        path = Path(directory).absolute()
        info = path.lstat()
        require(path.resolve(strict=True) == path and stat.S_ISDIR(info.st_mode)
                and info.st_uid == os.geteuid() and info.st_mode & 0o077 == 0)
        require(pc == Pin.pc(str(pc.address)) and 0 < seconds <= MAX_SESSION_SECONDS)
        if cdn is not None:
            require(cdn == Pin.cdn(cdn.host.decode("ascii"), str(cdn.address)))
        self.path = path / "broker.sock"
        self._pins = MappingProxyType({pin.host: pin.address for pin in (pc, cdn) if pin})
        self._authorities = frozenset(host + b":443" for host in self._pins)
        self._seconds = seconds
        self._dial = dial
        self._lock = threading.Lock()
        self._stopped = threading.Event()
        self._attempts = {host: 0 for host in self._pins}
        self._relayed_bytes = {host: 0 for host in self._pins}
        self._denied = 0
        self._server = None

    def __enter__(self):
        require(self._server is None and not self._stopped.is_set() and not self.path.exists())
        self._deadline = time.monotonic() + self._seconds
        self._server = _UnixServer(self.path, self)
        try:
            self.path.chmod(0o600)
            self._thread = threading.Thread(target=self._server.serve_forever,
                                            kwargs={"poll_interval": 0.1}, daemon=True)
            self._thread.start()
        except BaseException:
            self._server.server_close()
            self.path.unlink(missing_ok=True)
            raise
        return self

    def __exit__(self, *_exc):
        self.disarm()
        self._server.shutdown()
        self._server.server_close()
        self._thread.join()
        self.path.unlink(missing_ok=True)

    def disarm(self):
        # Synchronize with the last bounded dial so no new dial follows return.
        with self._lock:
            self._stopped.set()

    def note_denial(self):
        with self._lock:
            self._denied += 1

    def summary(self):
        with self._lock:
            return {"pcDialAttempts": self._attempts[PC_HOST],
                    "mediaDialAttempts": sum(v for host, v in self._attempts.items() if host != PC_HOST),
                    "deniedConnections": self._denied, "disarmed": self._stopped.is_set()}

    def remaining(self):
        if self._stopped.is_set():
            raise _Disarmed
        remaining = self._deadline - time.monotonic()
        require(remaining > 0)
        return remaining

    def _account_bytes(self, host, size):
        # Called with the relay lock held.
        maximum = PC_DIALS * 17 * MIB if host == PC_HOST else 17 * MIB
        self.remaining()
        require(self._relayed_bytes[host] + size <= maximum)
        self._relayed_bytes[host] += size

    def account_bytes(self, host, size):
        with self._lock:
            self._account_bytes(host, size)

    def tunnel(self, peer):
        host = read_connect(peer, self._authorities, min(4, self.remaining()))
        peer.sendall(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        hello = read_client_hello(peer, host, min(5, self.remaining()))
        with self._lock:
            remaining = self.remaining()
            require(self._attempts[host] < (PC_DIALS if host == PC_HOST else 1))
            self._account_bytes(host, len(hello))
            self._attempts[host] += 1  # a failed dial also consumes its allowance
            destination = self._dial(self._pins[host], min(3, remaining))
        with destination:
            self.remaining()
            destination.sendall(hello)
            self.relay(host, peer, destination)

    def relay(self, host, left, right):
        with selectors.DefaultSelector() as poll:
            poll.register(left, selectors.EVENT_READ, right)
            poll.register(right, selectors.EVENT_READ, left)
            while not self._stopped.is_set() and time.monotonic() < self._deadline:
                for key, _ in poll.select(min(.2, self.remaining())):
                    key.fileobj.settimeout(min(.5, self.remaining()))
                    data = key.fileobj.recv(16384)
                    if not data:
                        return
                    self.account_bytes(host, len(data))
                    key.data.settimeout(min(.5, self.remaining()))
                    key.data.sendall(data)
