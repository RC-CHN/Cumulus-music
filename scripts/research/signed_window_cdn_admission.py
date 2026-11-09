"""Host-only, one-shot CDN admission from the sealed native probe."""

import ctypes
import json
import os
import stat
import subprocess
import sys
import time

from signed_window_broker import Pin, cdn_authority, public_ipv4, verify_certificate
from signed_window_tls_fence import require

REQUEST = "cdn-authority-request.json"
ACK = "cdn-authority-armed.json"
MAX_MESSAGE = 256
DNS_SECONDS = 3
CERTIFICATE_SECONDS = 5
MAX_ANSWERS = 16
# Only the exact absolute hostname is queried. The helper cannot dial HTTPS or
# access the scratch proof; it returns bounded IPv4 address strings over stdout.
DNS_HELPER = """
import json, socket, sys
host = sys.stdin.buffer.read(254).decode("ascii")
addresses = sorted({row[4][0] for row in socket.getaddrinfo(
    host + ".", 443, socket.AF_INET, socket.SOCK_STREAM, socket.IPPROTO_TCP)})
if not 1 <= len(addresses) <= 16:
    raise SystemExit(1)
print(json.dumps(addresses))
"""


class AdmissionRefused(ValueError):
    pass


def resolve_ipv4(host, seconds):
    """Resolve once in a time-bounded child; never try a fallback address."""
    result = subprocess.run(
        [sys.executable, "-I", "-S", "-c", DNS_HELPER],
        input=cdn_authority(host), stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
        env={"PATH": "/usr/bin:/bin", "LC_ALL": "C"}, timeout=seconds, check=False,
    )
    require(result.returncode == 0 and len(result.stdout) <= 512)
    answers = json.loads(result.stdout)
    require(type(answers) is list and 1 <= len(answers) <= MAX_ANSWERS)
    # Reject mixed public/private answers, and choose one deterministic pin.
    return str(min(public_ipv4(value) for value in answers))


def signature(info):
    return (info.st_dev, info.st_ino, info.st_uid, info.st_mode, info.st_nlink,
            info.st_size, info.st_mtime_ns, info.st_ctime_ns)


def record_at(directory, name):
    try:
        fd = os.open(name, os.O_RDONLY | os.O_NONBLOCK | os.O_CLOEXEC | os.O_NOFOLLOW,
                     dir_fd=directory)
    except FileNotFoundError:
        return None
    with os.fdopen(fd, "rb") as stream:
        info = os.fstat(stream.fileno())
        require(stat.S_ISREG(info.st_mode) and info.st_uid == os.geteuid()
                and stat.S_IMODE(info.st_mode) == 0o600 and info.st_nlink == 1
                and 0 < info.st_size <= MAX_MESSAGE)
        data = stream.read(MAX_MESSAGE + 1)
        pinned = signature(info)
        require(len(data) == info.st_size
                and pinned == signature(os.fstat(stream.fileno()))
                and pinned == signature(os.stat(name, dir_fd=directory, follow_symlinks=False)))
    return data, pinned


def parse_request(data):
    # Count object fields before conversion so duplicate keys are refused.
    obj = json.loads(data.decode("ascii"), object_pairs_hook=tuple)
    require(type(obj) is tuple and len(obj) == 1 and obj[0][0] == "authority")
    host = obj[0][1]
    cdn_authority(host)
    return host


def publish_ack(directory, host):
    temporary = ".cdn-authority-armed.tmp"
    fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_CLOEXEC | os.O_NOFOLLOW,
                 0o600, dir_fd=directory)
    try:
        with os.fdopen(fd, "wb") as stream:
            os.fchmod(stream.fileno(), 0o600)
            data = json.dumps({"authority": host}, separators=(",", ":")).encode("ascii") + b"\n"
            require(len(data) <= MAX_MESSAGE)
            stream.write(data)
        rename = ctypes.CDLL(None, use_errno=True).renameat2
        rename.argtypes = (ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p,
                           ctypes.c_uint)
        rename.restype = ctypes.c_int
        # RENAME_NOREPLACE: the final entry appears already singly linked.
        if rename(directory, os.fsencode(temporary), directory, os.fsencode(ACK), 1) != 0:
            raise OSError(ctypes.get_errno(), "private CDN acknowledgement refused")
    finally:
        try:
            os.unlink(temporary, dir_fd=directory)
        except FileNotFoundError:
            pass


class CdnAdmission:
    def __init__(self, output, broker, *, resolve=resolve_ipv4, verify=verify_certificate):
        self.output = output
        self.broker = broker
        self.resolve = resolve
        self.verify = verify
        self.request = None
        self.ack = None
        self.authority = None
        self.resolutions = 0
        self.preflight_attempts = 0
        self.armed = False
        self.failed = None

    def __enter__(self):
        self.directory = os.open(self.output, os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC | os.O_NOFOLLOW)
        try:
            info = os.fstat(self.directory)
            require(info.st_uid == os.geteuid() and stat.S_IMODE(info.st_mode) == 0o700
                    and not os.listdir(self.directory))
            return self
        except BaseException:
            os.close(self.directory)
            raise

    def __exit__(self, *_exc):
        os.close(self.directory)

    def summary(self):
        result = {"requestAccepted": self.request is not None, "dnsLookups": self.resolutions,
                  "certificatePreflightAttempts": self.preflight_attempts,
                  "pinArmed": self.armed, "ackPublished": self.ack is not None}
        if self.failed is not None:
            result["failure"] = self.failed
        return result

    def check_unchanged(self):
        if self.ack is None:
            return
        try:
            require(record_at(self.directory, REQUEST) == self.request
                    and record_at(self.directory, ACK) == self.ack)
        except (OSError, ValueError):
            self.failed = "changed_record"
            self.broker.disarm()
            raise AdmissionRefused("private CDN admission stopped") from None

    def available(self, deadline, cap):
        broker_seconds = self.broker.media_admission_seconds()
        seconds = min(broker_seconds, deadline - time.monotonic(), cap)
        require(seconds > 0)
        return seconds

    def observe(self, deadline):
        if self.failed is not None:
            raise AdmissionRefused("private CDN admission stopped")
        if self.ack is not None:
            self.check_unchanged()
            return
        category = "request"
        try:
            require(record_at(self.directory, ACK) is None)
            request = record_at(self.directory, REQUEST)
            if request is None:
                return
            host = parse_request(request[0])
            self.request = request
            category = "resolve"
            seconds = self.available(deadline, DNS_SECONDS)
            self.resolutions += 1
            pin = Pin.cdn(host, self.resolve(host, seconds))
            category = "certificate"
            seconds = self.available(deadline, CERTIFICATE_SECONDS)
            self.preflight_attempts += 1
            self.verify(pin, seconds)
            category = "arm"
            self.available(deadline, CERTIFICATE_SECONDS)
            require(record_at(self.directory, REQUEST) == request
                    and record_at(self.directory, ACK) is None)
            self.broker.arm_media(pin, deadline=deadline)
            self.armed = True
            category = "acknowledge"
            require(deadline > time.monotonic())
            publish_ack(self.directory, host)
            self.ack = record_at(self.directory, ACK)
            require(self.ack is not None and parse_request(self.ack[0]) == host)
            self.authority = host
        except (AttributeError, OSError, ValueError, RecursionError, subprocess.SubprocessError):
            self.failed = category
            self.broker.disarm()
            raise AdmissionRefused("private CDN admission stopped") from None
