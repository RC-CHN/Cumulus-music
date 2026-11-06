#!/usr/bin/env python3
"""Manual, sealed stock libmpv HTTPS decoder and first-PCM proof.

Requires Linux x86_64, a closed network/mount namespace with only loopback,
and a synthetic /etc/hosts entry. No production account or media API is used.
"""
import argparse
import ctypes as c
import hashlib
import http.server
import json
from pathlib import Path
import re
import socket
import ssl
import struct
import subprocess
import sys
import tempfile
import threading
import time
import zlib

HOST = "cumulus-tone-fixture.music.126.net"
RESOURCE = "/synthetic-tone.flac"
MPV_SHA256 = "2051bdd3bce70349ffa9513c5db4ac0d7fa4aad869004b243f39da93768e8bc5"
FLAC_SHA256 = "e2fe2ad3ecd24483db4ad23cade06bc2403f688dc13304f5c2ed8933486d0247"
PCM_SHA256 = "fd4410b1284111d2f4a39fdbd252230e1f2d25b85f5aae146a980f7417b23f63"
FRAMES, RATE = 88_207, 44_100
SEEK = 0.5
FRAME = int(RATE * SEEK)
PREROLL = 21_504


class Event(c.Structure):
    _fields_ = [("id", c.c_int), ("error", c.c_int), ("reply", c.c_uint64), ("data", c.c_void_p)]


class Log(c.Structure):
    _fields_ = [("prefix", c.c_char_p), ("level", c.c_char_p), ("text", c.c_char_p)]


class End(c.Structure):
    _fields_ = [("reason", c.c_int), ("error", c.c_int), ("entry", c.c_int64)]


class Player:
    def __init__(self, library, output, ca):
        self.lib = c.CDLL(str(library))
        self.log, self.frames, self.events = [], [], []
        p, s, i = c.c_void_p, c.c_char_p, c.c_int
        functions = dict(
            mpv_create=([], p),
            mpv_set_option_string=([p, s, s], i),
            mpv_initialize=([p], i),
            mpv_command=([p, c.POINTER(s)], i),
            mpv_wait_event=([p, c.c_double], c.POINTER(Event)),
            mpv_request_log_messages=([p, s], i),
            mpv_get_property_string=([p, s], p),
            mpv_free=([p], None),
            mpv_terminate_destroy=([p], None),
        )
        for name, (args, ret) in functions.items():
            fn = getattr(self.lib, name)
            fn.argtypes, fn.restype = args, ret
        self.mp = self.lib.mpv_create()
        assert self.mp
        for key, value in {
            "config": "no", "load-scripts": "no", "terminal": "no", "vo": "null",
            "audio-display": "no", "video": "no", "pause": "yes",
            "ao": "pcm", "ao-pcm-file": str(output), "ao-pcm-waveheader": "no",
            "af": "lavfi=[ashowinfo]", "audio-format": "s16",
            "tls-verify": "yes", "tls-ca-file": str(ca),
            "network-timeout": "4", "cache": "no", "demuxer-seekable-cache": "no",
            "demuxer-readahead-secs": "0.1", "stream-buffer-size": "8192",
            "volume": "100",
        }.items():
            assert self.lib.mpv_set_option_string(self.mp, key.encode(), value.encode()) >= 0, key
        assert self.lib.mpv_initialize(self.mp) == 0
        assert self.lib.mpv_request_log_messages(self.mp, b"v") == 0

    def command(self, *args):
        values = (c.c_char_p * (len(args) + 1))(*(arg.encode() for arg in args), None)
        assert self.lib.mpv_command(self.mp, values) >= 0, args[0]

    def prop(self, key):
        raw = self.lib.mpv_get_property_string(self.mp, key.encode())
        if not raw:
            return None
        try:
            return c.string_at(raw).decode()
        finally:
            self.lib.mpv_free(raw)

    def state(self, output):
        return {key: self.prop(key) for key in
                ("pause", "seeking", "time-pos", "audio-codec-name",
                 "audio-params/samplerate", "audio-params/channel-count",
                 "audio-params/channels", "audio-params/format",
                 "audio-out-params/samplerate", "audio-out-params/channels",
                 "audio-out-params/format", "current-ao")} | {
                     "frameLogCount": len(self.frames), "outputBytes": size(output)
                 }

    def pump(self, timeout=0.05):
        event = self.lib.mpv_wait_event(self.mp, timeout).contents
        if event.id == 2:
            message = c.cast(event.data, c.POINTER(Log)).contents
            txt = message.text.decode(errors="replace").strip()
            if len(self.log) < 2000:
                self.log.append(txt)
            row = re.search(
                r"n:(\d+) pts:(-?\d+) pts_time:([\d.\-]+).*?fmt:(\w+) channels:(\d+)"
                r" chlayout:(\S+) rate:(\d+) nb_samples:(\d+) checksum:([0-9A-F]+)", txt
            )
            if row:
                assert len(self.frames) < 512, "unbounded frame log"
                n, pts, time_, fmt, ch, layout, rate, length, sha = row.groups()
                self.frames.append(dict(n=int(n), pts=int(pts), time=float(time_), format=fmt,
                                        channels=int(ch), layout=layout, rate=int(rate),
                                        frames=int(length), adler32=sha))
        elif event.id == 6:
            self.events.append([6, c.cast(event.data, c.POINTER(c.c_int64)).contents.value])
        elif event.id == 7:
            data = c.cast(event.data, c.POINTER(End)).contents
            self.events.append([7, data.entry, data.reason, data.error])
        elif event.id in (8, 18, 20, 21):
            self.events.append([event.id])

    def until(self, description, ready):
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            self.pump()
            if ready():
                return
        raise RuntimeError((description, self.events, self.frames[-4:], self.log[-12:]))

    def close(self):
        self.lib.mpv_terminate_destroy(self.mp)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def size(path):
    return path.stat().st_size if path.exists() else 0


def source_pcm():
    values = bytearray()
    for i in range(FRAMES):
        left = (i * 97 + (i // 128) * 23) % 8192 - 4096
        values.extend(struct.pack("<hh", left, -int(left / 8)))
    assert sha(values) == PCM_SHA256
    return values


def start_server(data, cert, key):
    calls = []
    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_GET(self):
            assert self.path == RESOURCE and self.headers["Host"] == HOST
            assert not self.headers.get("Cookie") and not self.headers.get("Authorization")
            range_ = self.headers.get("Range")
            left, right = 0, len(data) - 1
            if range_:
                match = re.fullmatch(r"bytes=(\d+)-(\d*)", range_)
                assert match
                left = int(match[1])
                if match[2]:
                    right = min(right, int(match[2]))
            assert 0 <= left <= right < len(data) and len(calls) < 16
            calls.append([left, right, range_ is not None])
            self.send_response(206 if range_ else 200)
            self.send_header("Accept-Ranges", "bytes")
            self.send_header("Content-Type", "audio/flac")
            self.send_header("Content-Length", str(right - left + 1))
            if range_:
                self.send_header("Content-Range", f"bytes {left}-{right}/{len(data)}")
            self.end_headers()
            try:
                self.wfile.write(data[left:right + 1])
            except (BrokenPipeError, ConnectionResetError, ssl.SSLEOFError):
                pass

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 443), Handler)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(cert, key)
    server.socket = context.wrap_socket(server.socket, server_side=True)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server, calls


def prove(output, player, calls):
    snapshots = {}
    player.command("loadfile", "https://" + HOST + RESOURCE, "replace")
    player.until(
        "paused load",
        lambda: any(e[0] == 7 for e in player.events)
        or ([8] in player.events and [21] in player.events and bool(player.frames)),
    )
    assert [8] in player.events and [21] in player.events
    snapshots["loadedPaused"] = player.state(output)
    mark, range_mark, filter_mark = len(player.events), len(calls), len(player.frames)
    player.command("seek", str(SEEK), "absolute+exact")
    player.until(
        "paused same-entry seek",
        lambda: [20] in player.events[mark:] and [21] in player.events[mark:]
        and any(frame["n"] == 0 for frame in player.frames[filter_mark:]),
    )
    snapshots["soughtPaused"] = player.state(output)
    for snap, clock in [(snapshots["loadedPaused"], 0.0), (snapshots["soughtPaused"], SEEK)]:
        assert (snap["pause"], snap["seeking"], snap["current-ao"]) == ("yes", "no", "pcm")
        assert abs(float(snap["time-pos"]) - clock) <= 1 / RATE
        assert snap["audio-codec-name"] == "flac"
        for prefix in ("audio-params", "audio-out-params"):
            assert [snap[f"{prefix}/{key}"] for key in ("samplerate", "channels", "format")] == [
                "44100", "stereo", "s16"
            ]
        assert snap["audio-params/channel-count"] == "2" and snap["outputBytes"] == 0
    assert len(calls) > range_mark and any(left > 0 for left, _, _ in calls[range_mark:])
    player.command("set", "pause", "no")
    player.until("native EOF", lambda: any(event[0] == 7 for event in player.events))
    snapshots["eof"] = player.state(output)
    assert [event for event in player.events if event[0] == 6] == [[6, 1]]
    assert [event for event in player.events if event[0] == 7] == [[7, 1, 0, 0]]
    assert 2 <= len(calls) <= 16 and all(range_requested for _, _, range_requested in calls)
    return snapshots


def validate_frames(frames, pcm):
    starts = [row for row in frames if row["n"] == 0]
    assert [row["pts"] for row in starts] == [0, PREROLL]
    assert [row["frames"] for row in starts] == [1024, 1024]
    assert PREROLL < FRAME < PREROLL + starts[1]["frames"]
    for row in frames:
        assert (row["format"], row["channels"], row["layout"], row["rate"]) == (
            "s16", 2, "stereo", RATE
        )
        start, end = row["pts"], row["pts"] + row["frames"]
        assert 0 <= start < end <= FRAMES
        assert row["adler32"] == f"{zlib.adler32(pcm[start * 4:end * 4], 0):08X}"
    assert frames[-1]["pts"] + frames[-1]["frames"] == FRAMES
    return starts


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--mpv", required=True, type=Path)
    cli.add_argument("--source", required=True, type=Path)
    cli.add_argument("--output", required=True, type=Path)
    args = cli.parse_args()
    if not __debug__ or sys.platform != "linux":
        cli.error("run unoptimized Python on Linux x86_64")
    assert {name for _, name in socket.if_nameindex()} == {"lo"}
    assert socket.gethostbyname(HOST) == "127.0.0.1"
    assert args.mpv.stat().st_size == 3_153_472
    assert args.source.stat().st_size == 187_478
    assert sha(args.mpv.read_bytes()) == MPV_SHA256
    data = args.source.read_bytes()
    assert sha(data) == FLAC_SHA256
    root = args.output.resolve()
    root.mkdir()
    output = root / "output.s16"
    with tempfile.TemporaryDirectory(dir=root) as tls_dir:
        cert, key = Path(tls_dir) / "cert.pem", Path(tls_dir) / "key.pem"
        subprocess.run(
            ["openssl", "req", "-x509", "-newkey", "rsa:2048", "-noenc", "-days", "1",
             "-subj", f"/CN={HOST}", "-addext", f"subjectAltName=DNS:{HOST}",
             "-out", str(cert), "-keyout", str(key)],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True,
        )
        server, calls = start_server(data, cert, key)
        try:
            player = Player(args.mpv, output, cert)
            try:
                snapshots = prove(output, player, calls)
            finally:
                player.close()
                (root / "native.log").write_text("\n".join(player.log), encoding="utf-8")
        finally:
            server.shutdown()
            server.server_close()
    pcm = source_pcm()
    expect = pcm[FRAME * 4:]
    assert size(output) == len(expect)
    actual = output.read_bytes()
    assert actual == expect, (len(actual), len(expect), sha(actual), sha(expect))
    starts = validate_frames(player.frames, pcm)
    report = dict(
        kind="cumulus_stock_https_prefilter_pcm_v1", ok=True, libmpvSha256=MPV_SHA256,
        fixtureSha256=FLAC_SHA256, directLibmpvOnly=True, productionActor=False,
        toneTransform=False, physicalOutput=False, externalNetwork=False, nativeEntry=1,
        seekSeconds=SEEK, firstDecodedAfterSeek=starts[1], presentationStartFrame=FRAME,
        preTargetFrames=FRAME - PREROLL, outputFrames=len(actual) // 4, outputSha256=sha(actual),
        firstPresentedPcmHex=actual[:16].hex(), rangeRequests=calls, snapshots=snapshots,
        events=player.events, frames=player.frames,
    )
    (root / "report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print("Sealed stock HTTPS/native frame/AO proof:",
          report["outputFrames"], "exact frames", len(calls), "HTTP requests")


if __name__ == "__main__":
    main()
