#!/usr/bin/env python3
"""Launch the signed Linux WebView once, with only its private HTTPS bridge.

The caller prepares minimal credentials and starts a separately guarded host
broker. This helper never opens the original account, dials an IP outside the
jail, or authorizes a target. No proof application output is copied to logs.
"""

from contextlib import contextmanager
import json
import math
import os
from pathlib import Path
import re
import secrets
import selectors
import signal
import socket
import socketserver
import stat
import struct
import subprocess
import sys
import tempfile
import threading
import time


BRIDGE_PORT = 41389
PROXY = f"127.0.0.1:{BRIDGE_PORT}"
RECEIPT = "isolation.json"
CERTS = Path("/etc/ssl/certs/ca-certificates.crt")
WORKER = Path(__file__).resolve()
PATH = "/usr/bin:/usr/sbin"
MAX_RECEIPT_BYTES = 1024
MAX_PREPARED_BYTES = 32 * 1024 * 1024
SESSION_BUS = b"""<busconfig>
  <type>session</type>
  <listen>unix:path=/runtime/bus</listen>
  <auth>EXTERNAL</auth>
  <standard_session_servicedirs/>
  <policy context="default">
    <allow send_destination="*"/>
    <allow receive_sender="*"/>
    <allow own="*"/>
  </policy>
</busconfig>
"""


class SandboxError(RuntimeError):
    """A sanitized setup or child refusal; does not contain paths or output."""


def _require(condition, message):
    if not condition:
        raise SandboxError(message)


def _canonical(path):
    path = Path(path)
    _require(path.is_absolute() and path == path.resolve(strict=True), "sandbox path is not canonical")
    return path


def _private_directory(path):
    info = path.stat(follow_symlinks=False)
    _require(stat.S_ISDIR(info.st_mode) and info.st_uid == os.getuid()
             and stat.S_IMODE(info.st_mode) == 0o700, "sandbox directory is not private")


def _prepared_tree(root):
    _require((root / "parameters.json").is_file(), "prepared parameters are absent")
    pending = [root]
    count = 0
    size = 0
    while pending:
        path = pending.pop()
        count += 1
        _require(count <= 16 and len(path.relative_to(root).parts) <= 4, "prepared tree exceeds bounds")
        info = path.stat(follow_symlinks=False)
        _require(info.st_uid == os.getuid() and info.st_mode & 0o077 == 0,
                 "prepared input is not private")
        if stat.S_ISDIR(info.st_mode):
            for child in path.iterdir():
                _require(count + len(pending) < 16, "prepared tree exceeds bounds")
                pending.append(child)
        else:
            _require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1
                     and not path.name.endswith(("-wal", "-shm", "-journal")),
                     "prepared input is not a closed regular file")
            size += info.st_size
            _require(size <= MAX_PREPARED_BYTES, "prepared input exceeds its size bound")


def _paths(binary, prepared, work, output, broker):
    binary, prepared, work, output, broker = map(_canonical, (binary, prepared, work, output, broker))
    for directory in (prepared, work, output, broker.parent):
        _private_directory(directory)
    roots = (prepared, work, output)
    _require(not any(a == b or a in b.parents or b in a.parents
                     for i, a in enumerate(roots) for b in roots[i + 1:])
             and not any(a in b.parents for a in roots for b in (binary, broker)), "sandbox mounts overlap")
    _require(not any(work.iterdir()) and not any(output.iterdir()), "sandbox output is not fresh")
    _prepared_tree(prepared)
    info = binary.stat(follow_symlinks=False)
    _require(stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid()
             and info.st_mode & 0o022 == 0 and info.st_mode & 0o100, "proof binary is not trusted")
    with binary.open("rb") as stream:
        _require(stream.read(4) == b"\x7fELF", "proof binary is not ELF")
    info = broker.stat(follow_symlinks=False)
    _require(stat.S_ISSOCK(info.st_mode) and info.st_uid == os.getuid()
             and stat.S_IMODE(info.st_mode) == 0o600, "sandbox broker is not a private socket")
    return binary, prepared, work, output, broker


def _terminate(process, *, graceful=False):
    if graceful and process.poll() is None:
        try:
            process.terminate()
            process.wait(timeout=1)
        except (ProcessLookupError, subprocess.TimeoutExpired):
            pass
    if process.poll() is None:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    process.wait()


def _private_write(path, data):
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC
    with os.fdopen(os.open(path, flags, 0o600), "wb") as stream:
        stream.write(data)


@contextmanager
def _display(root, deadline):
    authority = root / "xauth"
    # A fresh wildcard Xauthority record inside this private directory. Xvfb
    # supplies its own free display number, so no ambient X11 server is used.
    fields = (b"", b"", b"MIT-MAGIC-COOKIE-1", secrets.token_bytes(16))
    _private_write(authority, b"\xff\xff" + b"".join(struct.pack("!H", len(v)) + v for v in fields))
    read_fd, write_fd = os.pipe()
    try:
        with subprocess.Popen(
            ["/usr/bin/Xvfb", "-displayfd", str(write_fd), "-screen", "0", "1280x800x24",
             "-nolisten", "tcp", "-noreset", "-auth", str(authority)],
            pass_fds=(write_fd,), env={"PATH": PATH, "HOME": str(root)},
            start_new_session=True, stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        ) as display:
            os.close(write_fd)
            write_fd = None
            try:
                with selectors.DefaultSelector() as selector:
                    selector.register(read_fd, selectors.EVENT_READ)
                    _require(selector.select(max(0, min(4, deadline - time.monotonic()))),
                             "private X display timed out")
                    number = os.read(read_fd, 16)
                _require(re.fullmatch(rb"[0-9]{1,5}\n", number), "private X display did not start")
                number = number[:-1].decode("ascii")
                display_socket = Path("/tmp/.X11-unix") / f"X{number}"
                _require(display_socket.is_socket() and display.poll() is None, "private X socket is missing")
                yield f":{number}", display_socket, authority
            finally:
                _terminate(display, graceful=True)
    finally:
        os.close(read_fd)
        if write_fd is not None:
            os.close(write_fd)


def _command(binary, prepared, work, output, broker, config, display, netns, discover_cdn=False):
    name, display_socket, authority = display
    return [
        "/usr/bin/bwrap", "--unshare-user", "--unshare-net", "--unshare-ipc", "--unshare-pid",
        "--unshare-uts", "--die-with-parent", "--new-session", "--clearenv", "--cap-drop", "ALL",
        "--ro-bind", "/usr", "/usr", "--symlink", "usr/lib", "/lib", "--symlink", "usr/lib64", "/lib64",
        "--symlink", "usr/bin", "/bin", "--dev", "/dev", "--proc", "/proc", "--remount-ro", "/proc",
        "--tmpfs", "/tmp", "--dir", "/tmp/.X11-unix", "--ro-bind", str(display_socket), str(display_socket),
        "--ro-bind", str(authority), "/xauth", "--dir", "/profile", "--perms", "0700", "--dir", "/runtime",
        "--ro-bind", str(config / "passwd"), "/etc/passwd", "--ro-bind", str(config / "group"), "/etc/group",
        "--ro-bind", str(config / "session.conf"), "/session.conf",
        "--ro-bind", "/etc/fonts", "/etc/fonts", "--ro-bind", "/etc/ld.so.cache", "/etc/ld.so.cache",
        "--ro-bind", str(CERTS), str(CERTS), "--ro-bind", str(WORKER), "/sandbox.py",
        "--ro-bind", str(binary), "/probe", "--ro-bind", str(prepared), "/prepared",
        "--bind", str(work), "/work", "--bind", str(output), "/proof", "--ro-bind", str(broker), "/relay",
        "--setenv", "PATH", PATH, "--setenv", "HOME", "/profile", "--setenv", "XDG_RUNTIME_DIR", "/runtime",
        "--setenv", "DISPLAY", name, "--setenv", "XAUTHORITY", "/xauth", "--setenv", "LANG", "en_US.UTF-8",
        "--setenv", "GDK_BACKEND", "x11", "--setenv", "LIBGL_ALWAYS_SOFTWARE", "1",
        "--setenv", "WEBKIT_FORCE_DMABUF_RENDERER", "1", "--setenv", "WEBKIT_DMABUF_RENDERER_FORCE_SHM", "1",
        "--setenv", "CUMULUS_SANDBOX_OUTER_NETNS", netns, "--chdir", "/work",
        "--", "/usr/bin/dbus-run-session", "--dbus-daemon=/usr/bin/dbus-daemon", "--config-file=/session.conf",
        "--", "/usr/bin/python3", "-I", "/sandbox.py", "--worker",
        *(["--discover-cdn"] if discover_cdn else []),
    ]


class _Bridge(socketserver.ThreadingTCPServer):
    allow_reuse_address = False
    request_queue_size = 4

    def __init__(self):
        self.slots = threading.BoundedSemaphore(4)
        self.stopped = threading.Event()
        self.lock = threading.Lock()
        self.connections = 0
        super().__init__(("127.0.0.1", BRIDGE_PORT), _Forward)

    def process_request(self, request, address):
        if not self.slots.acquire(blocking=False):
            self.shutdown_request(request)
            return
        try:
            super().process_request(request, address)
        except BaseException:
            self.slots.release()
            raise

    def process_request_thread(self, request, address):
        try:
            super().process_request_thread(request, address)
        finally:
            self.slots.release()


class _Forward(socketserver.BaseRequestHandler):
    def handle(self):
        try:
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as link:
                self.request.settimeout(2)
                link.settimeout(2)
                link.connect("/relay")
                with self.server.lock:
                    self.server.connections += 1
                with selectors.DefaultSelector() as selector:
                    selector.register(self.request, selectors.EVENT_READ, link)
                    selector.register(link, selectors.EVENT_READ, self.request)
                    deadline = time.monotonic() + 20
                    while selector.get_map() and not self.server.stopped.is_set() and time.monotonic() < deadline:
                        for key, _ in selector.select(timeout=.15):
                            data = key.fileobj.recv(16384)
                            if data:
                                key.data.sendall(data)
                            else:
                                selector.unregister(key.fileobj)
                                key.data.shutdown(socket.SHUT_WR)
        except OSError:
            pass


def _worker(discover_cdn=False):
    outer = os.environ.pop("CUMULUS_SANDBOX_OUTER_NETNS", "")
    _require(re.fullmatch(r"net:\[[0-9]+\]", outer) and os.readlink("/proc/self/ns/net") != outer,
             "proof does not have a fresh network namespace")
    links = json.loads(subprocess.check_output(["/usr/sbin/ip", "-j", "link"]))
    _require([item["ifname"] for item in links] == ["lo"] and "UP" in links[0]["flags"],
             "proof network is not loopback only")
    for family in ("-4", "-6"):
        routes = subprocess.check_output(["/usr/sbin/ip", family, "-j", "route", "show", "table", "main"])
        _require(json.loads(routes) == [], "proof has an external network route")
    _require(not any(Path(path).exists() for path in (
        "/home", "/sys", "/run/user", "/run/dbus", "/var/run", "/etc/resolv.conf", "/etc/hosts",
    )), "proof sees an unexpected host mount")
    bus = os.environ.get("DBUS_SESSION_BUS_ADDRESS", "")
    _require(re.fullmatch(r"unix:path=/runtime/bus(?:,guid=[0-9a-f]+)?", bus)
             and Path("/runtime/bus").is_socket(), "proof private session bus is missing")
    _require(Path("/relay").is_socket() and not any("proxy" in key.lower() for key in os.environ),
             "proof proxy environment is invalid")
    with _Bridge() as bridge:
        server = threading.Thread(target=bridge.serve_forever, kwargs={"poll_interval": .15})
        server.start()
        try:
            env = dict(os.environ, CUMULUS_SIGNED_WINDOW_PROXY=PROXY)
            command = ["/probe", "--live"] + (["--discover-cdn"] if discover_cdn else [])
            status = subprocess.run(command, env=env, stdin=subprocess.DEVNULL,
                                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode
        finally:
            bridge.stopped.set()
            bridge.shutdown()
            server.join()
    _require(status == 0, "signed proof process did not finish successfully")
    _require(1 <= bridge.connections <= 26, "signed proof bridge use is outside scope")
    summary = {"loopbackOnly": True, "mainRoutes": 0, "privateSessionBus": True,
               "privateDisplay": True, "privateBroker": True, "bridgeConnections": bridge.connections,
               "probeExit": 0}
    _private_write(Path("/proof") / RECEIPT, json.dumps(summary, separators=(",", ":")).encode("ascii") + b"\n")


def _receipt(output):
    flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC
    with os.fdopen(os.open(output / RECEIPT, flags), "rb") as stream:
        info = os.fstat(stream.fileno())
        _require(stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid()
                 and stat.S_IMODE(info.st_mode) == 0o600 and info.st_nlink == 1,
                 "proof isolation receipt is not private")
        data = stream.read(MAX_RECEIPT_BYTES + 1)
    _require(len(data) <= MAX_RECEIPT_BYTES, "proof isolation receipt exceeds its bound")
    result = json.loads(data)
    fixed = {"loopbackOnly": True, "mainRoutes": 0, "privateSessionBus": True,
             "privateDisplay": True, "privateBroker": True, "probeExit": 0}
    _require(type(result) is dict and set(result) == set(fixed) | {"bridgeConnections"}
             and all(type(result[key]) is type(value) and result[key] == value for key, value in fixed.items())
             and type(result["bridgeConnections"]) is int and 1 <= result["bridgeConnections"] <= 26,
             "proof isolation receipt is invalid")
    return result


def run(binary, prepared, work, output, broker, *, timeout=55, discover_cdn=False):
    """Run the exact live or discovery probe in an isolated scratch namespace.

    `prepared` contains the closed input tree and `parameters.json`; it is
    mounted read-only. `work` and `output` start empty, owner-only, and writable.
    `broker` is the caller's already running owner-only AF_UNIX listener.
    Only the fixed, verified isolation receipt is returned.
    """
    try:
        _require(sys.platform == "linux" and type(timeout) in (int, float)
                 and math.isfinite(timeout) and 5 <= timeout <= 60
                 and type(discover_cdn) is bool, "unsupported sandbox timeout, mode or platform")
        binary, prepared, work, output, broker = _paths(binary, prepared, work, output, broker)
        deadline = time.monotonic() + timeout
        with tempfile.TemporaryDirectory(prefix="cumulus-signed-window-", dir="/tmp") as raw_config:
            config = Path(raw_config)
            uid, gid = os.getuid(), os.getgid()
            _private_write(config / "passwd", f"proof:x:{uid}:{gid}:Private proof:/profile:/bin/false\n".encode())
            _private_write(config / "group", f"proof:x:{gid}:\n".encode())
            _private_write(config / "session.conf", SESSION_BUS)
            with _display(config, deadline) as display:
                command = _command(binary, prepared, work, output, broker, config, display,
                                   os.readlink("/proc/self/ns/net"), discover_cdn)
                with subprocess.Popen(command, env={"PATH": PATH}, start_new_session=True,
                                      stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                      stderr=subprocess.DEVNULL) as child:
                    try:
                        status = child.wait(timeout=max(0, deadline - time.monotonic()))
                    except subprocess.TimeoutExpired:
                        raise SandboxError("signed proof sandbox timed out") from None
                    finally:
                        _terminate(child)
        _require(status == 0, "signed proof sandbox refused or failed")
        return _receipt(output)
    except (OSError, ValueError, subprocess.SubprocessError):
        raise SandboxError("signed proof sandbox setup or receipt failed") from None


if __name__ == "__main__":
    if sys.argv[1:] not in (["--worker"], ["--worker", "--discover-cdn"]):
        raise SystemExit(2)
    try:
        _worker(discover_cdn=len(sys.argv) == 3)
    except (SandboxError, OSError, ValueError, subprocess.SubprocessError):
        raise SystemExit(1) from None
