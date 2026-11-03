#!/usr/bin/env python3
"""Exercise ordinary libmpv on verified synthetic ISO BMFF in a brokerless Linux namespace."""

import argparse
import hashlib
import http.client
import http.server
import json
import os
from pathlib import Path
import re
import ssl
import subprocess
import tempfile
import threading

SCRIPT = Path(__file__).resolve()
HOST = "cumulus-local-fixture.music.126.net"
ROUTE = "/synthetic/audio.m4a"
EXPECTED = "d0cb4a728564e1f14f6444bb9b50b14d73bdb3c6267453f7567843ee439463ee"
LENGTH = 6327


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def child():
    links = json.loads(subprocess.check_output(["ip", "-j", "link"]))
    require([item["ifname"] for item in links] == ["lo"], "unexpected network interface")
    for family in ("-4", "-6"):
        routes = subprocess.check_output(["ip", family, "-j", "route", "show", "table", "main"])
        require(json.loads(routes) == [], "unexpected network route")
    require(not any(Path(p).exists() for p in (
        "/home", "/etc/resolv.conf", "/run/user", "/run/dbus", "/var/run", "/relay",
    )), "unexpected host mounts")
    require(not any("proxy" in key.lower() for key in os.environ), "unexpected proxy environment")
    served = []
    denied = []
    with tempfile.TemporaryFile("w+b") as verified:
        with Path("/synthetic-input").open("rb") as source:
            payload = source.read(LENGTH + 1)
        require(len(payload) == LENGTH and hashlib.sha256(payload).hexdigest() == EXPECTED,
                "synthetic fixture integrity mismatch")
        verified.write(payload)
        verified.flush()
        del payload

        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass

            def send_error(self, *_args, **_kwargs):
                denied.append(True)
                self.send_response(403)
                self.send_header("Content-Length", "0")
                self.end_headers()

            def serve(self, send_body):
                if (self.path != ROUTE
                        or self.headers.get_all("Host") != [HOST]
                        or any(name in self.headers for name in ("Authorization", "Cookie"))):
                    self.send_error(403)
                    return
                range_headers = self.headers.get_all("Range", [])
                start, end = 0, LENGTH - 1
                if range_headers:
                    match = re.fullmatch(r"bytes=([0-9]{1,4})-([0-9]{0,4})", range_headers[0])
                    if len(range_headers) != 1 or match is None:
                        self.send_error(403)
                        return
                    start = int(match[1])
                    end = min(int(match[2]), end) if match[2] else end
                    if not start <= end:
                        self.send_error(403)
                        return
                self.send_response(206 if range_headers else 200)
                self.send_header("Accept-Ranges", "bytes")
                self.send_header("Content-Type", "audio/mp4")
                self.send_header("Content-Length", str(end - start + 1))
                if range_headers:
                    self.send_header("Content-Range", f"bytes {start}-{end}/{LENGTH}")
                self.end_headers()
                if send_body:
                    self.wfile.write(os.pread(verified.fileno(), end - start + 1, start))
                    served.append(True)

            def do_GET(self):
                self.serve(True)

            def do_HEAD(self):
                self.serve(False)

        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.load_cert_chain("/fixture.crt", "/fixture.key")
        with http.server.HTTPServer(("127.0.0.1", 443), Handler) as server:
            server.socket = context.wrap_socket(server.socket, server_side=True)
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            try:
                trust = ssl.create_default_context(cafile="/fixture.crt")
                for path, headers in (
                    (ROUTE + "?alternate=1", {}),
                    (ROUTE, {"Cookie": "synthetic=forbidden"}),
                    (ROUTE, {"Host": "other.invalid"}),
                    ("/different.m4a", {}),
                ):
                    connection = http.client.HTTPSConnection(HOST, context=trust, timeout=2)
                    try:
                        connection.request("GET", path, headers=headers)
                        response = connection.getresponse()
                        require(response.status == 403 and response.read() == b"",
                                "unexpected local TLS route admission")
                    finally:
                        connection.close()
                require(len(denied) == 4 and not served, "refused requests served media")
                denied.clear()
                run = subprocess.run(["/probe"], capture_output=True, text=True, timeout=15)
                require(run.returncode == 0 and run.stdout.strip() == "ordinary-url-aac-clock-ok",
                        f"ordinary native URL decode failed ({run.returncode}): {run.stderr}")
            finally:
                server.shutdown()
                thread.join(timeout=2)
    require(served and not denied, "fixture HTTP scope or byte serving was not verified")
    print(json.dumps({"syntheticOnly": True, "noEgress": True, "verifiedFixture": True,
                      "ordinaryUrlAacDecoded": True, "clockAdvancedAndStopped": True,
                      "localMediaGets": len(served)}))


def run(binary):
    require(binary.is_file(), "require an existing local TLS probe binary")
    fixture = SCRIPT.parents[2] / "core/tests/fixtures/tag_audio/seed.m4a"
    with tempfile.TemporaryDirectory(prefix="cumulus-local-tls-") as directory:
        root = Path(directory)
        cert, key = root / "cert", root / "key"
        subprocess.run([
            "openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
            "-subj", f"/CN={HOST}", "-addext", f"subjectAltName=DNS:{HOST}",
            "-keyout", key, "-out", cert,
        ], check=True, timeout=10, capture_output=True)
        hosts, nsswitch = root / "hosts", root / "nsswitch.conf"
        hosts.write_text(f"127.0.0.1 {HOST}\n", encoding="utf-8")
        nsswitch.write_text("hosts: files\n", encoding="utf-8")
        subprocess.run([
            # Set the port floor only inside the new network namespace. The
            # decoder runs after bwrap has dropped setup capabilities.
            "unshare", "--user", "--map-root-user", "--net", "--", "/bin/sh", "-ec",
            '/usr/sbin/ip link set lo up; '
            '/usr/sbin/sysctl -q -w net.ipv4.ip_unprivileged_port_start=443; exec "$@"',
            "local-tls-netns",
            "bwrap", "--unshare-user", "--unshare-ipc", "--unshare-pid",
            "--die-with-parent", "--new-session", "--clearenv",
            "--ro-bind", "/usr", "/usr", "--symlink", "usr/lib", "/lib",
            "--symlink", "usr/lib64", "/lib64", "--symlink", "usr/bin", "/bin",
            "--dev", "/dev", "--proc", "/proc", "--tmpfs", "/tmp",
            "--ro-bind", binary, "/probe", "--ro-bind", SCRIPT, "/proof-local-tls",
            "--ro-bind", fixture, "/synthetic-input", "--ro-bind", cert, "/fixture.crt",
            "--ro-bind", key, "/fixture.key", "--ro-bind", cert, "/etc/ssl/certs/ca-certificates.crt",
            "--ro-bind", hosts, "/etc/hosts", "--ro-bind", nsswitch, "/etc/nsswitch.conf",
            "--ro-bind", "/etc/ld.so.cache", "/etc/ld.so.cache",
            "--setenv", "SSL_CERT_FILE", "/fixture.crt", "--setenv", "G_TLS_CA_FILE", "/fixture.crt",
            "--setenv", "PATH", "/usr/bin:/usr/sbin", "--setenv", "HOME", "/tmp", "--chdir", "/",
            "--", "/usr/bin/python3", "/proof-local-tls", "--child",
        ], check=True, timeout=25)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--child", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.child:
        child()
    else:
        require(args.binary is not None, "--binary is required")
        run(args.binary.resolve())
