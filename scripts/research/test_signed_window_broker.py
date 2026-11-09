import ipaddress
from pathlib import Path
import queue
import socket
import ssl
import tempfile
import unittest

from signed_window_broker import Broker, PC_HOST, Pin, public_ipv4
from signed_window_tls_fence import Denied, ECH, EARLY_DATA, check_server_name

PC_IP = "1.1.1.1"  # Never dialled: the test adapter returns a local socketpair.
CDN_IP = "8.8.8.8"
CDN = b"m1.music.126.net"


def hello(host):
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
    incoming, outgoing = ssl.MemoryBIO(), ssl.MemoryBIO()
    tls = context.wrap_bio(incoming, outgoing, server_hostname=host.decode("ascii"))
    try:
        tls.do_handshake()
    except ssl.SSLWantReadError:
        return outgoing.read()
    raise AssertionError("fixture unexpectedly completed TLS without I/O")


def read_exact(peer, length):
    data = bytearray()
    while len(data) < length:
        block = peer.recv(length - len(data))
        if not block:
            raise AssertionError("truncated local test stream")
        data.extend(block)
    return bytes(data)


class SignedWindowBrokerTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="csb-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.dials = queue.Queue()

    def dial(self, address, timeout):
        self.assertGreater(timeout, 0)
        bridge, server = socket.socketpair()
        bridge.settimeout(2)
        server.settimeout(2)
        self.addCleanup(server.close)
        self.dials.put((address, server))
        return bridge

    def broker(self, **kwargs):
        return Broker(self.root, Pin.pc(PC_IP), Pin.cdn(CDN.decode(), CDN_IP), dial=self.dial, **kwargs)

    def peer(self, broker):
        peer = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.addCleanup(peer.close)
        peer.settimeout(2)
        peer.connect(str(broker.path))
        return peer

    def connect(self, broker, host):
        peer = self.peer(broker)
        authority = host + b":443"
        peer.sendall(b"CONNECT " + authority + b" HTTP/1.1\r\nHost: " + authority + b"\r\n\r\n")
        return peer

    def send_hello(self, peer, host):
        established = b"HTTP/1.1 200 Connection Established\r\n\r\n"
        self.assertEqual(read_exact(peer, len(established)), established)
        original = hello(host)
        peer.sendall(original)
        return original

    def test_only_exact_pc_and_single_exact_media_dial_and_passthrough(self):
        with self.broker() as broker:
            with self.connect(broker, PC_HOST) as api:
                sent = self.send_hello(api, PC_HOST)
                address, server = self.dials.get(timeout=2)
                self.assertEqual(address, ipaddress.IPv4Address(PC_IP))
                self.assertEqual(read_exact(server, len(sent)), sent)
                server.sendall(b"opaque TLS fixture")
                self.assertEqual(read_exact(api, 18), b"opaque TLS fixture")
            with self.connect(broker, CDN) as media:
                sent = self.send_hello(media, CDN)
                address, server = self.dials.get(timeout=2)
                self.assertEqual(address, ipaddress.IPv4Address(CDN_IP))
                self.assertEqual(read_exact(server, len(sent)), sent)
            with self.connect(broker, CDN) as repeated:
                self.send_hello(repeated, CDN)
                self.assertEqual(repeated.recv(1), b"")
            broker.disarm()
            with self.peer(broker) as late:
                self.assertEqual(late.recv(1), b"")
        self.assertTrue(self.dials.empty())
        self.assertFalse(broker.path.exists())
        self.assertEqual(broker.summary(), {"pcDialAttempts": 1, "mediaDialAttempts": 1,
                                           "deniedConnections": 1, "disarmed": True})

    def test_other_authority_wrong_sni_and_proxy_header_never_dial(self):
        with self.broker() as broker:
            with self.connect(broker, b"m2.music.126.net") as wrong:
                self.assertEqual(wrong.recv(1), b"")
            with self.connect(broker, PC_HOST) as wrong_sni:
                self.send_hello(wrong_sni, CDN)
                self.assertEqual(wrong_sni.recv(1), b"")
            with self.peer(broker) as auth:
                auth.sendall(b"CONNECT " + PC_HOST + b":443 HTTP/1.1\r\nHost: " + PC_HOST
                             + b"\r\nProxy-Authorization: synthetic\r\n\r\n")
                self.assertEqual(auth.recv(1), b"")
        self.assertTrue(self.dials.empty())
        self.assertEqual(broker.summary()["deniedConnections"], 3)

    def test_partial_hello_expires_before_dial(self):
        with self.broker(seconds=.4) as broker:
            with self.connect(broker, PC_HOST) as peer:
                established = b"HTTP/1.1 200 Connection Established\r\n\r\n"
                self.assertEqual(read_exact(peer, len(established)), established)
                peer.sendall(b"\x16\x03")
                self.assertEqual(peer.recv(1), b"")
        self.assertTrue(self.dials.empty())

    def test_private_numeric_pins_and_non_plain_hello_are_refused(self):
        for address in ("127.0.0.1", "10.1.2.3", "169.254.4.4", "224.0.0.1", "fec0::1",
                        "::ffff:1.1.1.1", "www.music.163.com", "192.0.2.1", "0.0.0.0"):
            with self.assertRaises(Denied):
                public_ipv4(address)
        for host in (PC_HOST.decode(), "music.126.net", "m1.music.126.net.evil.test",
                     "M1.music.126.net", "m1.music.126.net.", "a" * 64 + ".music.126.net"):
            with self.assertRaises(Denied):
                Pin.cdn(host, CDN_IP)
        raw = hello(PC_HOST)
        handshake = raw[9:]
        # Locate extensions after the bounded session ID, cipher and compression vectors.
        i = 34
        i += 1 + handshake[i]
        i += 2 + int.from_bytes(handshake[i:i+2], "big")
        i += 1 + handshake[i]
        length = int.from_bytes(handshake[i:i+2], "big")
        self.assertEqual(len(handshake), i + 2 + length)
        for extension in (ECH, EARLY_DATA):
            added = handshake[:i] + (length + 4).to_bytes(2, "big") + handshake[i+2:]
            with self.assertRaises(Denied):
                check_server_name(added + extension.to_bytes(2, "big") + b"\0\0", PC_HOST)


if __name__ == "__main__":
    unittest.main()
