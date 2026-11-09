"""Closed synthetic checks of the actual Linux proof sandbox and bridge."""

import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch

from signed_window_sandbox import RECEIPT, SandboxError, _prepared_tree, _receipt, run


FIXTURE = r'''
#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

int main(int argc, char **argv) {
    if (argc != 2 || strcmp(argv[1], "--live") != 0) return 1;
    const char *proxy = getenv("CUMULUS_SIGNED_WINDOW_PROXY");
    if (!proxy || strcmp(proxy, "127.0.0.1:41389") != 0 || getenv("HTTPS_PROXY") ||
        getenv("CUMULUS_SANDBOX_HOST_SECRET") || getenv("CUMULUS_SANDBOX_OUTER_NETNS")) return 2;
    if (access("/home", F_OK) == 0 || access("/sys", F_OK) == 0 ||
        access("/etc/resolv.conf", F_OK) == 0 || access("/run/user", F_OK) == 0) return 3;
    char buffer[9] = {0};
    int fd = open("/prepared/parameters.json", O_RDONLY);
    if (fd < 0 || read(fd, buffer, 7) != 7 || memcmp(buffer, "fixture", 7)) return 4;
    close(fd);
    if (open("/prepared/parameters.json", O_WRONLY) >= 0 || errno != EROFS) return 5;
    struct sockaddr_in address = {.sin_family = AF_INET, .sin_port = htons(9)};
    if (inet_pton(AF_INET, "192.0.2.1", &address.sin_addr) != 1) return 6;
    fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0 || connect(fd, (struct sockaddr *)&address, sizeof(address)) == 0 || errno != ENETUNREACH)
        return 7;
    close(fd);
    address.sin_port = htons(41389);
    address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0 || connect(fd, (struct sockaddr *)&address, sizeof(address)) ||
        write(fd, "fixture", 7) != 7 || shutdown(fd, SHUT_WR) ||
        read(fd, buffer, 8) != 8 || memcmp(buffer, "accepted", 8)) return 8;
    close(fd);
    fd = open("/work/marker", O_CREAT | O_WRONLY | O_EXCL, 0600);
    if (fd < 0 || write(fd, "ready", 5) != 5) return 9;
    close(fd);
    execl("/usr/bin/xdpyinfo", "xdpyinfo", (char *)0);
    return 10;
}
'''

TIMEOUT_FIXTURE = r'''
#include <arpa/inet.h>
#include <sys/socket.h>
#include <unistd.h>

int main(void) {
    struct sockaddr_in address = {.sin_family = AF_INET, .sin_port = htons(41389)};
    address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0 || connect(fd, (struct sockaddr *)&address, sizeof(address)) || write(fd, "ready", 5) != 5)
        return 1;
    sleep(30);
    return 2;
}
'''


HAS_TOOLS = sys.platform == "linux" and all(shutil.which(tool) for tool in (
    "cc", "bwrap", "Xvfb", "xdpyinfo", "ip", "dbus-run-session",
))


def fixture(root, code):
    binary = root / "fixture"
    subprocess.run(["cc", "-x", "c", "-o", str(binary), "-"], input=code, text=True, check=True)
    binary.chmod(0o700)
    prepared, work, output, broker_dir = (root / name for name in ("prepared", "work", "proof", "broker"))
    for directory in (prepared, work, output, broker_dir):
        directory.mkdir(mode=0o700)
    params = prepared / "parameters.json"
    params.write_bytes(b"fixture")
    params.chmod(0o600)
    return binary, prepared, work, output, broker_dir / "socket"


class SandboxTests(unittest.TestCase):
    def test_prepared_tree_does_not_mount_aliases_or_live_sqlite_sidecars(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            entry = root / "parameters.json"
            entry.write_bytes(b"fixture")
            entry.chmod(0o600)
            _prepared_tree(root)
            alias = root / "alias"
            alias.symlink_to(entry)
            with self.assertRaises(SandboxError):
                _prepared_tree(root)
            alias.unlink()
            wal = root / "core.sqlite3-wal"
            wal.touch(mode=0o600)
            with self.assertRaises(SandboxError):
                _prepared_tree(root)

    def test_receipt_is_private_bounded_and_only_returns_fixed_facts(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            report = root / RECEIPT
            valid = {"loopbackOnly": True, "mainRoutes": 0, "privateSessionBus": True,
                     "privateDisplay": True, "privateBroker": True, "bridgeConnections": 1,
                     "probeExit": 0}
            report.write_text(json.dumps(valid), encoding="ascii")
            report.chmod(0o600)
            self.assertEqual(_receipt(root), valid)
            report.write_text(json.dumps(dict(valid, forbidden="synthetic-private-value")), encoding="ascii")
            with self.assertRaisesRegex(SandboxError, "invalid"):
                _receipt(root)
            report.write_text(json.dumps(dict(valid, mainRoutes=False)), encoding="ascii")
            with self.assertRaisesRegex(SandboxError, "invalid"):
                _receipt(root)
            report.write_text(" " * 1025, encoding="ascii")
            with self.assertRaisesRegex(SandboxError, "exceeds"):
                _receipt(root)

    @unittest.skipUnless(HAS_TOOLS, "requires local Linux sandbox, display and compiler tools")
    def test_actual_jail_allows_only_one_private_broker_and_uses_own_display(self):
        with tempfile.TemporaryDirectory(prefix="cumulus-sandbox-test-", dir="/tmp") as tmp:
            binary, prepared, work, output, broker = fixture(Path(tmp), FIXTURE)
            seen = []
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as listener:
                listener.bind(str(broker))
                broker.chmod(0o600)
                listener.listen(1)
                listener.settimeout(3)

                def serve():
                    try:
                        with listener.accept()[0] as conn:
                            conn.settimeout(2)
                            with conn.makefile("rb") as stream:
                                seen.append(stream.read(8))
                            conn.sendall(b"accepted")
                    except OSError:
                        pass

                worker = threading.Thread(target=serve)
                worker.start()
                try:
                    with patch.dict(os.environ, {"DISPLAY": ":99999", "HTTPS_PROXY": "http://127.0.0.1:9",
                                                 "CUMULUS_SANDBOX_HOST_SECRET": "fixture-must-not-enter"}):
                        summary = run(binary, prepared, work, output, broker, timeout=10)
                finally:
                    worker.join()
            self.assertEqual(seen, [b"fixture"])
            self.assertEqual(summary["bridgeConnections"], 1)
            self.assertTrue(summary["loopbackOnly"] and summary["privateDisplay"] and summary["privateSessionBus"])
            self.assertEqual((work / "marker").read_bytes(), b"ready")
            self.assertEqual((prepared / "parameters.json").read_bytes(), b"fixture")

    @unittest.skipUnless(HAS_TOOLS, "requires local Linux sandbox, display and compiler tools")
    def test_timeout_closes_running_bridge_without_writing_a_pass_receipt(self):
        with tempfile.TemporaryDirectory(prefix="cumulus-sandbox-timeout-", dir="/tmp") as tmp:
            paths = fixture(Path(tmp), TIMEOUT_FIXTURE)
            output, broker = paths[-2:]
            replies = []
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as listener:
                listener.bind(str(broker))
                broker.chmod(0o600)
                listener.listen(1)
                listener.settimeout(3)

                def serve():
                    try:
                        with listener.accept()[0] as conn, conn.makefile("rb") as stream:
                            conn.settimeout(7)
                            replies.extend((stream.read(5), stream.read(1)))
                    except OSError:
                        pass

                worker = threading.Thread(target=serve)
                worker.start()
                try:
                    with self.assertRaisesRegex(SandboxError, "timed out"):
                        run(*paths, timeout=5)
                finally:
                    worker.join()
            self.assertEqual(replies, [b"ready", b""])
            self.assertFalse((output / RECEIPT).exists())


if __name__ == "__main__":
    unittest.main()
