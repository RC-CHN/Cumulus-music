#!/usr/bin/env python3
"""Opt-in Linux production WebView proof of ordinary HTTPS audio, without egress."""

import argparse
import hashlib
import http.client
import http.server
import ipaddress
import json
import os
from pathlib import Path
import pwd
import re
import ssl
import subprocess
import tempfile
import threading

SCRIPT = Path(__file__).resolve()
HOST = 'cumulus-local-fixture.music.126.net'
ROUTE = '/synthetic/audio.m4a'
SIZE = 6327
SHA256 = 'd0cb4a728564e1f14f6444bb9b50b14d73bdb3c6267453f7567843ee439463ee'
FIXTURE = SCRIPT.parents[2]/'core/tests/fixtures/tag_audio/seed.m4a'


def verify_isolation(directory):
    links = json.loads(subprocess.check_output(['ip', '-j', 'link']))
    assert [item['ifname'] for item in links] == ['lo']
    for family in ('-4', '-6'):
        assert json.loads(subprocess.check_output(['ip', family, '-j', 'route', 'show', 'table', 'main'])) == []
    assert Path('/etc/hosts').read_text() == f'127.0.0.1 {HOST}\n'
    assert Path('/etc/resolv.conf').read_bytes() == b''
    assert Path('/etc/nsswitch.conf').read_text() == 'passwd: files\ngroup: files\nhosts: files\n'
    assert Path('/etc/ssl/certs/ca-certificates.crt').read_bytes() == (directory/'cert').read_bytes()
    assert not any('proxy' in name.lower() for name in os.environ)


def ordinary_https(args, directory):
    assert os.geteuid() != 0, 'WebView must run as the unprivileged invoking user'
    verify_isolation(directory)
    with FIXTURE.open('rb') as source:
        payload = source.read(SIZE + 1)
    assert len(payload) == SIZE and hashlib.sha256(payload).hexdigest() == SHA256
    counters = dict(gets=0, rejected=0)

    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def send_error(self, *_args, **_kwargs):
            counters['rejected'] += 1
            self.send_response(403)
            self.send_header('Content-Length', '0')
            self.end_headers()

        def serve(self, body):
            if (
                self.path != ROUTE
                or self.headers.get_all('Host') != [HOST]
                or not ipaddress.ip_address(self.client_address[0]).is_loopback
                or any(name in self.headers for name in ('Cookie', 'Authorization', 'Proxy-Authorization'))
                or (body and counters['gets'])
            ):
                return self.send_error(403)
            ranges = self.headers.get_all('Range', [])
            # A short, fast-start file fits one whole GET. FFmpeg may express
            # that exact same request as bytes=0-.
            if ranges and (len(ranges) != 1 or not re.fullmatch(r'bytes=0-(?:6326)?', ranges[0])):
                return self.send_error(403)
            self.send_response(206 if ranges else 200)
            self.send_header('Accept-Ranges', 'bytes')
            self.send_header('Content-Type', 'audio/mp4')
            self.send_header('Content-Length', str(SIZE))
            if ranges:
                self.send_header('Content-Range', f'bytes 0-{SIZE - 1}/{SIZE}')
            self.end_headers()
            if body:
                self.wfile.write(payload)
                counters['gets'] += 1

        def do_GET(self):
            self.serve(True)

        def do_HEAD(self):
            self.serve(False)

    tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    tls.load_cert_chain(directory/'cert', directory/'key')
    with http.server.HTTPServer(('127.0.0.1', 443), Handler) as server:
        server.socket = tls.wrap_socket(server.socket, server_side=True)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            trust = ssl.create_default_context(cafile=directory/'cert')
            for route, headers in (
                ('/wrong.m4a', {}),
                (ROUTE + '?different=1', {}),
                (ROUTE, {'Cookie': 'synthetic=denied'}),
                (ROUTE, {'Host': 'other.invalid'}),
                (ROUTE, {'Range': 'bytes=1-'}),
            ):
                connection = http.client.HTTPSConnection(HOST, context=trust, timeout=2)
                try:
                    connection.request('GET', route, headers=headers)
                    denied = connection.getresponse()
                    assert denied.status == 403 and denied.read() == b''
                finally:
                    connection.close()
            assert counters == dict(gets=0, rejected=5)
            counters['rejected'] = 0
            if args.preflight:
                # Exercise the permitted HTTPS byte path without a desktop build.
                connection = http.client.HTTPSConnection(HOST, context=trust, timeout=2)
                try:
                    connection.request('GET', ROUTE, headers={'Range': 'bytes=0-'})
                    loaded = connection.getresponse()
                    assert loaded.status == 206 and loaded.read() == payload
                finally:
                    connection.close()
            else:
                subprocess.run([
                    'xvfb-run', '-a', 'dbus-run-session', '--', '/usr/bin/python3',
                    str(SCRIPT.with_name('main_flow_smoke.py')), '--ordinary-https',
                    '--binary', str(args.binary), '--root', str(args.root),
                    *(['--screenshots'] if args.screenshots else []),
                ], check=True, timeout=360)
        finally:
            server.shutdown()
            thread.join(timeout=2)
    assert counters == dict(gets=1, rejected=0)
    report = dict(syntheticOnly=True, loopbackOnly=True, verifiedFixture=True,
                  exactRouteRejected=5, ordinaryHttpsWebView=not args.preflight, localMediaGets=1)
    if not args.preflight:
        (args.root/'https-receipt.json').write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps(report))


def outer(args):
    assert os.geteuid() != 0, 'invoke as the normal desktop user (sudo is used only for namespaces)'
    if not args.preflight:
        assert args.binary and args.binary.is_file() and args.root and not args.root.exists()
    elif args.binary or args.root or args.screenshots:
        raise ValueError('preflight does not accept desktop options')
    with tempfile.TemporaryDirectory(prefix='cumulus-main-flow-https-') as temporary:
        directory = Path(temporary)
        subprocess.run([
            'openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '1',
            '-subj', f'/CN={HOST}', '-addext', f'subjectAltName=DNS:{HOST}',
            '-keyout', directory/'key', '-out', directory/'cert',
        ], check=True, capture_output=True, timeout=10)
        (directory/'hosts').write_text(f'127.0.0.1 {HOST}\n')
        (directory/'resolv').touch()
        (directory/'nsswitch').write_text('passwd: files\ngroup: files\nhosts: files\n')
        user = pwd.getpwuid(os.geteuid())
        setup = '''
            /usr/sbin/ip link set lo up
            /usr/sbin/sysctl -q -w net.ipv4.ip_unprivileged_port_start=443
            /usr/bin/mount --bind "$1/hosts" /etc/hosts
            /usr/bin/mount --bind "$1/resolv" /etc/resolv.conf
            /usr/bin/mount --bind "$1/nsswitch" /etc/nsswitch.conf
            /usr/bin/mount --bind "$1/cert" /etc/ssl/certs/ca-certificates.crt
            user="$2"
            shift 2
            exec /usr/sbin/runuser -u "$user" -- "$@"
        '''
        subprocess.run([
            'sudo', '-n', 'unshare', '--mount', '--propagation', 'private', '--net', '--fork',
            '--kill-child', '--', '/bin/sh', '-ec', setup, 'main-flow-https-namespace',
            str(directory), user.pw_name,
            '/usr/bin/env', '-i', 'PATH=/usr/bin:/usr/sbin:/bin', f'HOME={directory}',
            'LC_ALL=en_US.UTF-8', 'GDK_BACKEND=x11', 'WEBKIT_DISABLE_DMABUF_RENDERER=1',
            'LIBGL_ALWAYS_SOFTWARE=1', 'CUMULUS_TEST_WINDOW_MANAGER=openbox',
            f'SSL_CERT_FILE={directory}/cert', f'G_TLS_CA_FILE={directory}/cert',
            '/usr/bin/python3', str(SCRIPT), '--isolated', str(directory),
            *(['--preflight'] if args.preflight else ['--binary', str(args.binary), '--root', str(args.root)]),
            *(['--screenshots'] if args.screenshots else []),
        ], check=True, timeout=390)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=lambda path: Path(path).resolve())
    parser.add_argument('--root', type=lambda path: Path(path).resolve())
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--preflight', action='store_true', help='verify only TLS and namespace policy')
    parser.add_argument('--isolated', type=Path, help=argparse.SUPPRESS)
    arguments = parser.parse_args()
    if arguments.isolated:
        ordinary_https(arguments, arguments.isolated)
    else:
        outer(arguments)
