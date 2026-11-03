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
import ssl
import subprocess
import tempfile
import threading

SCRIPT = Path(__file__).resolve()
HOST = 'cumulus-local-fixture.music.126.net'
ROUTE = '/synthetic/audio.m4a'
RETRY_ROUTE = ROUTE + '?proof-retry=2'
NEXT_ROUTE = '/synthetic/next.m4a'
SIZE = 39106
SHA256 = '883d4ab2f3e6900f47a892c4941e3a6ae8aab577258aeca6fc7a1fe1d001d520'
FIXTURE = SCRIPT.parents[2]/'core/tests/fixtures/tag_audio/playback-12s.m4a'


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
    counters = dict(gets=0, failedGets=0, rejected=0)
    eof_gets = []
    routes = (ROUTE, NEXT_ROUTE) if args.eof_next else (ROUTE, RETRY_ROUTE) if args.retry_once else (ROUTE,)

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
                self.path not in routes
                or self.headers.get_all('Host') != [HOST]
                or not ipaddress.ip_address(self.client_address[0]).is_loopback
                or any(name in self.headers for name in ('Cookie', 'Authorization', 'Proxy-Authorization'))
                or (body and not args.eof_next and counters['gets'])
            ):
                return self.send_error(403)
            ranges = self.headers.get_all('Range', [])
            # A short, fast-start file fits one whole GET. FFmpeg may express
            # that exact same request as bytes=0-.
            if ranges and ranges not in (['bytes=0-'], [f'bytes=0-{SIZE - 1}']):
                return self.send_error(403)
            if args.eof_next and body:
                expected = ROUTE if not eof_gets else NEXT_ROUTE if len(eof_gets) == 1 else None
                if self.path != expected:
                    return self.send_error(403)
                if self.path == NEXT_ROUTE and not args.preflight:
                    # Never serve second-track media before the actual first
                    # playing checkpoint, even if authorization prefetched it.
                    if not (args.root/'run/first/ux_https_before_eof.json').is_file():
                        return self.send_error(403)
            if args.retry_once and self.path == ROUTE:
                if counters['failedGets'] or not body:
                    return self.send_error(403)
                counters['failedGets'] += 1
                self.send_response(503)
                self.send_header('Content-Length', '0')
                self.end_headers()
                return
            if args.retry_once and counters['failedGets'] != 1:
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
                if args.eof_next:
                    eof_gets.append(self.path)

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
            refused_routes = [
                ('/wrong.m4a', {}),
                (ROUTE + '?different=1', {}),
                (ROUTE, {'Cookie': 'synthetic=denied'}),
                (ROUTE, {'Host': 'other.invalid'}),
                (ROUTE, {'Range': 'bytes=1-'}),
            ]
            if args.eof_next:
                refused_routes.append((NEXT_ROUTE, {}))
            for route, headers in refused_routes:
                connection = http.client.HTTPSConnection(HOST, context=trust, timeout=2)
                try:
                    connection.request('GET', route, headers=headers)
                    denied = connection.getresponse()
                    assert denied.status == 403 and denied.read() == b''
                finally:
                    connection.close()
            assert counters == dict(gets=0, failedGets=0, rejected=len(refused_routes))
            counters['rejected'] = 0
            if args.preflight:
                # Exercise the permitted HTTPS byte path without a desktop build.
                if args.retry_once:
                    cases = [(ROUTE, 503, b''), (RETRY_ROUTE, 206, payload)]
                elif args.eof_next:
                    cases = [(ROUTE, 206, payload), (NEXT_ROUTE, 206, payload)]
                else:
                    cases = [(ROUTE, 206, payload)]
                for route, status, expected in cases:
                    connection = http.client.HTTPSConnection(HOST, context=trust, timeout=2)
                    try:
                        connection.request('GET', route, headers={'Range': 'bytes=0-'})
                        loaded = connection.getresponse()
                        assert loaded.status == status and loaded.read() == expected
                    finally:
                        connection.close()
            else:
                subprocess.run([
                    'xvfb-run', '-a', 'dbus-run-session', '--', '/usr/bin/python3',
                    str(SCRIPT.with_name('main_flow_smoke.py')), '--ordinary-https',
                    '--binary', str(args.binary), '--root', str(args.root),
                    *(['--https-retry-once'] if args.retry_once else []),
                    *(['--https-eof-next'] if args.eof_next else []),
                    *(['--screenshots'] if args.screenshots else []),
                ], check=True, timeout=360)
        finally:
            server.shutdown()
            thread.join(timeout=2)
            if not args.preflight and args.root.exists():
                observed = {**counters, 'orderedTracks': [1 if route == ROUTE else 2 for route in eof_gets]} if args.eof_next else counters
                (args.root/'https-requests.json').write_text(json.dumps(observed)+'\n')
    assert counters == dict(gets=2 if args.eof_next else 1, failedGets=int(args.retry_once), rejected=0)
    if args.eof_next:
        assert eof_gets == [ROUTE, NEXT_ROUTE]
    report = dict(syntheticOnly=True, loopbackOnly=True, verifiedFixture=True,
                  exactRouteRejected=len(refused_routes), ordinaryHttpsWebView=not args.preflight, localMediaGets=counters['gets'],
                  localMediaFailedGets=counters['failedGets'], explicitRetryMode=args.retry_once)
    if args.eof_next:
        report['orderedTrackGets'] = [1, 2]
    if not args.preflight:
        (args.root/'https-receipt.json').write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps(report))


def outer(args):
    assert os.geteuid() != 0, 'invoke as the normal desktop user (sudo is used only for namespaces)'
    child_path = '/usr/bin:/usr/sbin:/bin'
    if args.openbox_directory:
        if args.preflight or ':' in str(args.openbox_directory) or not (args.openbox_directory/'openbox').is_file():
            raise ValueError('Openbox directory must contain the executable and is only used for desktop runs')
        child_path += ':' + str(args.openbox_directory)
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
            '/usr/bin/env', '-i', f'PATH={child_path}', f'HOME={directory}',
            'LC_ALL=en_US.UTF-8', 'GDK_BACKEND=x11', 'WEBKIT_DISABLE_DMABUF_RENDERER=1',
            'LIBGL_ALWAYS_SOFTWARE=1', 'CUMULUS_TEST_WINDOW_MANAGER=openbox',
            f'SSL_CERT_FILE={directory}/cert', f'G_TLS_CA_FILE={directory}/cert',
            '/usr/bin/python3', str(SCRIPT), '--isolated', str(directory),
            *(['--preflight'] if args.preflight else ['--binary', str(args.binary), '--root', str(args.root)]),
            *(['--retry-once'] if args.retry_once else []),
            *(['--eof-next'] if args.eof_next else []),
            *(['--screenshots'] if args.screenshots else []),
        ], check=True, timeout=390)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=lambda path: Path(path).resolve())
    parser.add_argument('--root', type=lambda path: Path(path).resolve())
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--openbox-directory', type=lambda path: Path(path).resolve())
    special = parser.add_mutually_exclusive_group()
    special.add_argument('--retry-once', action='store_true', help='fail the first authorized HTTPS GET and require an explicit UI retry')
    special.add_argument('--eof-next', action='store_true', help='exercise real EOF into a second authorized HTTPS track')
    parser.add_argument('--preflight', action='store_true', help='verify only TLS and namespace policy')
    parser.add_argument('--isolated', type=Path, help=argparse.SUPPRESS)
    arguments = parser.parse_args()
    if arguments.isolated:
        ordinary_https(arguments, arguments.isolated)
    else:
        outer(arguments)
