#!/usr/bin/env python3
"""Offline Rust capture proof against an isolated PipeWire/Pulse null sink."""

import argparse
from contextlib import ExitStack, contextmanager
import json
import math
import os
from pathlib import Path
import select
import shutil
import struct
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
SINK = "cumulus_recognition_fixture"
DECOY = "cumulus_recognition_decoy"
NON_MONITOR = "cumulus_recognition_not_a_monitor.monitor"


def build(binary, log):
    records = []
    for line in log.read_text().splitlines():
        if line.startswith("{"):
            records.append(json.loads(line))
    artifacts = [
        Path(file)
        for item in records
        if item.get("reason") == "compiler-artifact"
        and item["target"]["name"] == "cumulus_core"
        for file in item["filenames"]
        if file.endswith(".rlib")
    ]
    assert len(artifacts) == 1 and artifacts[0].is_file(), "use a successful locked core build JSON log"
    artifact = artifacts[0]
    native = sorted({
        arg
        for item in records
        if item.get("reason") == "build-script-executed"
        for arg in item.get("linked_paths", [])
        if arg.startswith("native=")
    })
    command = [
        "rustc", "--edition", "2024", "-D", "warnings",
        str(ROOT / "desktop/probes/recognition-monitor/main.rs"),
        "--extern", f"cumulus_core={artifact}",
        "-L", f"dependency={artifact.parent / 'deps'}",
        "-o", str(binary),
    ]
    for arg in native:
        command += ["-L", arg]
    subprocess.run(command, cwd=ROOT, check=True, timeout=90)


def wait(description, predicate, seconds=8):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        if predicate():
            return
        time.sleep(.05)
    raise AssertionError(f"timed out waiting for {description}")


def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=3)


def pulse(args, env, check=True):
    result = subprocess.run(
        ["pactl", f"--server={env['PULSE_SERVER']}", *args],
        env=env, text=True, capture_output=True, timeout=3,
    )
    if check and result.returncode:
        raise RuntimeError(f"private Pulse command failed: {args[0]}: {result.stderr.strip()}")
    return result


@contextmanager
def private_audio(root):
    for name in ("pipewire", "pipewire-pulse", "wireplumber", "pactl", "pacat"):
        assert shutil.which(name), f"missing research executable: {name}"
    with ExitStack() as stack:
        runtime = Path(stack.enter_context(tempfile.TemporaryDirectory(
            prefix="cumulus-recognition-", dir="/tmp",
        )))
        env = {
            key: value for key, value in os.environ.items()
            if not key.startswith(("PIPEWIRE_", "PULSE_", "ALSA_", "WIREPLUMBER_"))
        }
        directories = {
            key: runtime / name for key, name in (
                ("XDG_CONFIG_HOME", "config"), ("XDG_STATE_HOME", "state"),
                ("XDG_DATA_HOME", "data"), ("XDG_CACHE_HOME", "cache"),
            )
        }
        for key, directory in directories.items():
            directory.mkdir()
            env[key] = str(directory)
        env.update(
            XDG_RUNTIME_DIR=str(runtime),
            PIPEWIRE_RUNTIME_DIR=str(runtime),
            PULSE_RUNTIME_PATH=str(runtime / "pulse"),
            PULSE_SERVER=f"unix:{runtime}/pulse/native",
            DBUS_SYSTEM_BUS_ADDRESS=f"unix:path={runtime}/no-system-bus",
            LC_ALL="C",
        )

        def spawn(name):
            log = stack.enter_context((root / f"{name}.log").open("xb"))
            child = subprocess.Popen([name], env=env, stdout=log, stderr=log)
            stack.callback(stop, child)
            return child

        server = spawn("pipewire")
        wait("private PipeWire socket", lambda: server.poll() is None and (runtime / "pipewire-0").is_socket())
        server = spawn("pipewire-pulse")
        policy = spawn("wireplumber")
        wait("private Pulse server", lambda: server.poll() is None and policy.poll() is None
             and (runtime / "pulse/native").is_socket() and pulse(["info"], env, False).returncode == 0)
        modules = {}
        for sink in (SINK, DECOY):
            modules[sink] = int(pulse([
                "load-module", "module-null-sink", f"sink_name={sink}",
                "rate=48000", "channels=2", "channel_map=front-left,front-right",
            ], env).stdout.strip())

        def decoy_is_default():
            response = pulse(["set-default-sink", DECOY], env, False)
            if response.returncode and response.stderr.strip() != "Failure: Not supported":
                raise RuntimeError("could not select the private decoy default")
            return pulse(["get-default-sink"], env, False).stdout.strip() == DECOY

        wait("private decoy default sink", decoy_is_default)
        modules[NON_MONITOR] = int(pulse([
            "load-module", "module-remap-source", f"master={SINK}.monitor",
            f"source_name={NON_MONITOR}",
        ], env).stdout.strip())
        sources = json.loads(pulse(["--format=json", "list", "sources"], env).stdout)
        names = {row["name"]: row for row in sources}
        assert {f"{SINK}.monitor", f"{DECOY}.monitor", NON_MONITOR} <= names.keys()
        yield runtime, server.pid, env, modules


def waveform(decoy=False):
    # Never touch a microphone or a media file. Both waveforms are synthetic.
    pcm = bytearray()
    for i in range(48_000 * 6):
        left = 1760 if decoy else 440
        right = 1760 if decoy else 880
        pcm.extend(struct.pack(
            "<hh", round(8000 * math.sin(math.tau * left * i / 48_000)),
            round(6000 * math.sin(math.tau * right * i / 48_000)),
        ))
    return pcm


@contextmanager
def play(pcm, sink, env):
    with os.fdopen(os.memfd_create("synthetic-stereo-pcm"), "w+b") as memory:
        memory.write(pcm)
        memory.seek(0)
        process = subprocess.Popen(
            ["pacat", f"--server={env['PULSE_SERVER']}", "--playback", "--raw", f"--device={sink}",
             f"--stream-name={sink}.tone", "--rate=48000", "--channels=2", "--format=s16le"],
            env=env, stdin=memory, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
        )
        try:
            yield process
        finally:
            stop(process)
            process.stderr.close()


def run_proof(args):
    root = args.root
    assert not Path("/dev/snd").exists()
    assert os.readlink("/proc/self/ns/net") != os.environ["CUMULUS_MONITOR_OUTER_NET"]
    with private_audio(root) as (runtime, pid, env, modules):
        # Deliberately give the probe a bad inherited server. Only its verified
        # explicit socket may be used. Playback commands keep the true endpoint.
        probe_env = {**env, "PULSE_SERVER": f"unix:{runtime}/missing-environment-socket"}
        prefix = [str(args.binary), str(runtime)]
        captured = []
        signal = waveform()
        decoy = waveform(True)

        def own_streams():
            rows = json.loads(pulse(["--format=json", "list", "source-outputs"], env).stdout)
            return [
                row for row in rows if row.get("properties", {}).get("application.name")
                == "Cumulus recognition monitor proof"
                or row.get("properties", {}).get("media.name") == "Cumulus private output capture"
            ]

        def no_streams():
            return not own_streams()

        sinks = {
            row["name"]: row["index"]
            for row in json.loads(pulse(["--format=json", "list", "sinks"], env).stdout)
        }

        def playing_both():
            rows = json.loads(pulse(["--format=json", "list", "sink-inputs"], env).stdout)
            return all(any(
                row.get("properties", {}).get("media.name") == f"{sink}.tone"
                and row["sink"] == sinks[sink] and not row.get("corked", True)
                for row in rows
            ) for sink in (SINK, DECOY))

        for fmt in ("i16", "f32"):
            with play(decoy, DECOY, env) as other, play(signal, SINK, env) as player:
                wait("two test tone streams on their explicit sinks", playing_both, 3)
                probe = subprocess.Popen(
                    [*prefix, str(pid), SINK, f"{SINK}.monitor", str(args.stock), fmt, "capture"],
                    env=probe_env, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                )
                try:
                    assert select.select([probe.stdout], [], [], 8)[0], "capture never became ready"
                    assert probe.stdout.readline().strip() == "monitor-ready"
                    wait("actual named record stream", lambda: len(own_streams()) == 1, 2)
                    out, err = probe.communicate(timeout=9)
                    assert probe.returncode == 0, err.strip()
                    assert player.poll() in (None, 0) and other.poll() in (None, 0)
                    result = json.loads(out)
                    assert result["status"] == "captured" and result["frames"] == 192000
                    assert result["produced"] >= 24_000
                    captured.append({"format": fmt, **result})
                    with (root / "captures.jsonl").open("a") as log:
                        log.write(json.dumps(captured[-1]) + "\n")
                finally:
                    stop(probe)
                    probe.stdout.close()
                    probe.stderr.close()
                wait("record stream removal", no_streams, 3)

        for selected, peer, reason in [
            (f"{DECOY}.monitor", pid, "not the expected private output monitor"),
            (NON_MONITOR, pid, "not the expected private output monitor"),
            ("cumulus_recognition_missing.monitor", pid, "private output monitor"),
            (f"{SINK}.monitor", pid + 1, "private Pulse server identity mismatch"),
        ]:
            failed = subprocess.run(
                [*prefix, str(peer), SINK, selected, str(args.stock), "i16", "capture"],
                env=probe_env, text=True, capture_output=True, timeout=8,
            )
            assert failed.returncode == 1 and not failed.stdout, failed.stderr
            assert reason in failed.stderr, failed.stderr
            wait("no record stream after rejection", no_streams, 3)

        start = time.monotonic()
        stopped = subprocess.run(
            [*prefix, str(pid), SINK, f"{SINK}.monitor", str(args.stock), "i16", "cancel"],
            env=probe_env, text=True, capture_output=True, timeout=6,
        )
        elapsed = time.monotonic() - start
        assert stopped.returncode == 0, stopped.stderr
        ready, result = stopped.stdout.splitlines()
        assert ready == "monitor-ready"
        result = json.loads(result)
        assert result["status"] == "cancelled" and result["produced"] < 24_000 and elapsed < 3
        wait("cancelled record stream removal", no_streams, 3)

        # Removing the selected sink must never turn a live monitor capture
        # into a capture of the still-playing, now only default decoy.
        pulse(["unload-module", str(modules[NON_MONITOR])], env)
        with play(decoy, DECOY, env):
            lost = subprocess.Popen(
                [*prefix, str(pid), SINK, f"{SINK}.monitor", str(args.stock), "i16", "capture"],
                env=probe_env, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            )
            try:
                assert select.select([lost.stdout], [], [], 8)[0]
                assert lost.stdout.readline().strip() == "monitor-ready"
                wait("record stream before output removal", lambda: len(own_streams()) == 1, 2)
                removal_start = time.monotonic()
                pulse(["unload-module", str(modules[SINK])], env)
                out, err = lost.communicate(timeout=8)
                removal_elapsed = time.monotonic() - removal_start
                assert lost.returncode == 1 and not out, err
                if "private monitor binding or format changed" in err:
                    removal = "binding_changed"
                elif "private capture timed out" in err:
                    removal = "timed_out"
                else:
                    raise AssertionError(err)
            finally:
                stop(lost)
                lost.stdout.close()
                lost.stderr.close()
            wait("removed source stream cleanup", no_streams, 3)

    report = {
        "captures": captured, "rejections": 4, "unisolatedRefused": True,
        "cancelled": True, "cancelElapsedMs": round(elapsed * 1000),
        "monitorRemovalRefusedFallback": True, "monitorRemovalResult": removal,
        "monitorRemovalElapsedMs": round(removal_elapsed * 1000),
    }
    (root / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--stock", type=Path, required=True)
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--core-build-log", type=Path)
    parser.add_argument("--inside", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()
    args.root = args.root.resolve()
    args.stock = args.stock.resolve(strict=True)
    if args.inside:
        run_proof(args)
        return

    args.root.mkdir(parents=True, mode=0o700, exist_ok=False)
    if args.binary is None:
        assert args.core_build_log, "provide a pinned cumulus-core cargo build JSON log or an existing probe"
        args.binary = args.root / "recognition-monitor-probe"
        build(args.binary, args.core_build_log)
    args.binary = args.binary.resolve(strict=True)
    outside = {key: value for key, value in os.environ.items()
               if key != "CUMULUS_MONITOR_OUTER_NET"}
    refused = subprocess.run(
        [str(args.binary), "/tmp/cumulus-recognition-not-running", "2",
         SINK, f"{SINK}.monitor", str(args.stock), "i16", "capture"],
        env=outside, text=True, capture_output=True, timeout=3,
    )
    assert refused.returncode == 1 and not refused.stdout
    assert "private network namespace required" in refused.stderr
    outer = os.readlink("/proc/self/ns/net")
    command = [
        "bwrap", "--unshare-user", "--unshare-net", "--die-with-parent",
        "--bind", "/", "/", "--dev", "/dev", "--proc", "/proc",
        "--setenv", "CUMULUS_MONITOR_OUTER_NET", outer,
        "--unsetenv", "DBUS_SESSION_BUS_ADDRESS",
        "--", "dbus-run-session", "--", sys.executable, str(Path(__file__).resolve()),
        "--inside", "--root", str(args.root), "--stock", str(args.stock),
        "--binary", str(args.binary),
    ]
    subprocess.run(command, check=True, timeout=65)


if __name__ == "__main__":
    main()
