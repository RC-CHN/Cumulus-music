"""Exact CONNECT and first TLS ClientHello admission, from the older proof relay."""

import re
import time

MAX_CONNECT = 2048
MAX_HELLO = 65536
MAX_RECORD = 16384
MAX_RECORDS = 16
ECH = 0xFE0D
EARLY_DATA = 42
HEADER = re.compile(rb"[a-zA-Z0-9-]+")
HOST = re.compile(rb"[a-z0-9]+(?:[a-z0-9-]*[a-z0-9])?(?:\.[a-z0-9]+(?:[a-z0-9-]*[a-z0-9])?)+")


class Denied(ValueError):
    pass


def require(value):
    if not value:
        raise Denied("proof relay scope denied")


class Deadline:
    def __init__(self, sock, seconds):
        self.sock = sock
        self.end = time.monotonic() + seconds

    def read(self, length):
        result = bytearray()
        while len(result) < length:
            remaining = self.end - time.monotonic()
            require(remaining > 0)
            self.sock.settimeout(remaining)
            chunk = self.sock.recv(length - len(result))
            require(chunk)
            result.extend(chunk)
        return bytes(result)


def read_connect(sock, authorities, seconds=4):
    """Parse the exact approved lowercase HTTPS authority. No upstream I/O."""
    deadline = Deadline(sock, seconds)
    request = bytearray()
    while not request.endswith(b"\r\n\r\n"):
        require(len(request) < MAX_CONNECT)
        request.extend(deadline.read(1))
    start, *headers = bytes(request[:-4]).split(b"\r\n")
    parts = start.split(b" ")
    require(len(parts) == 3 and parts[0] == b"CONNECT" and parts[1] in authorities
            and parts[2] in (b"HTTP/1.0", b"HTTP/1.1"))
    authority = parts[1]
    host = authority.removesuffix(b":443")
    require(authority == host + b":443" and len(host) <= 253 and HOST.fullmatch(host))
    parsed = {}
    for line in headers:
        key, separator, value = line.partition(b":")
        key = key.lower()
        require(separator and HEADER.fullmatch(key) and key not in parsed
                and key in (b"host", b"user-agent", b"proxy-connection", b"connection",
                            b"accept-encoding", b"accept-language")
                and all(0x20 <= byte < 0x7F or byte == 9 for byte in value))
        parsed[key] = value.strip(b" \t")
    if parts[2] == b"HTTP/1.1" or b"host" in parsed:
        require(parsed.get(b"host") in (authority, host))  # :443 may be omitted
    return host


class Reader:
    def __init__(self, data):
        self.data = data
        self.offset = 0

    def take(self, length):
        end = self.offset + length
        require(end <= len(self.data))
        result = self.data[self.offset:end]
        self.offset = end
        return result

    def integer(self, width):
        return int.from_bytes(self.take(width), "big")

    def vector(self, width):
        return self.take(self.integer(width))

    def more(self):
        return self.offset < len(self.data)


def check_server_name(message, expected):
    hello = Reader(message)
    require(hello.take(2) in (b"\x03\x01", b"\x03\x02", b"\x03\x03"))
    hello.take(32)  # random
    require(len(hello.vector(1)) <= 32)
    ciphers = hello.vector(2)
    require(len(ciphers) >= 2 and len(ciphers) % 2 == 0)
    compression = hello.vector(1)
    require(b"\x00" in compression)
    extensions = Reader(hello.vector(2))
    require(not hello.more())
    seen = set()
    matched = False
    while extensions.more():
        kind = extensions.integer(2)
        body = extensions.vector(2)
        require(kind not in seen and kind not in (ECH, EARLY_DATA))
        seen.add(kind)
        if kind == 0:
            name_list = Reader(body)
            names = Reader(name_list.vector(2))
            require(not name_list.more() and names.integer(1) == 0)
            require(names.vector(2) == expected and not names.more())
            matched = True
    require(matched)


def read_client_hello(sock, expected, seconds=5):
    """Return untouched TLS record bytes only after exact visible SNI checks."""
    deadline = Deadline(sock, seconds)
    handshake = bytearray()
    wire = bytearray()
    for _ in range(MAX_RECORDS):
        header = deadline.read(5)
        length = int.from_bytes(header[3:5], "big")
        require(header[0] == 22 and header[1] == 3 and 1 <= header[2] <= 3
                and 0 < length <= MAX_RECORD and len(wire) + 5 + length <= MAX_HELLO)
        fragment = deadline.read(length)
        wire.extend(header)
        wire.extend(fragment)
        handshake.extend(fragment)
        if len(handshake) < 4:
            continue
        require(handshake[0] == 1)
        size = int.from_bytes(handshake[1:4], "big")
        require(0 < size <= MAX_HELLO - 4 and len(handshake) <= size + 4)
        if len(handshake) == size + 4:
            check_server_name(bytes(handshake[4:]), expected)
            return bytes(wire)
    raise Denied("proof relay ClientHello limit reached")
