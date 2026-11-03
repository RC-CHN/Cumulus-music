#!/usr/bin/env python3
"""Linux synthetic proof: xvfb-run -a this script --binary PROBE --root NEW_ROOT.

Build PROBE with custom-protocol for the full run. Both phases use separate
namespaces, and neither phase reads a real profile or opens an egress route.
"""

import argparse
import json
import os
from pathlib import Path
import re
import sqlite3
import subprocess
import time


SOURCE = Path(__file__).resolve()


def require(condition, label):
    if not condition:
        raise RuntimeError(label)


def child(phase):
    # Run inside the same namespaces that will contain all application children.
    links = json.loads(subprocess.check_output(["ip", "-j", "link"]))
    require([link["ifname"] for link in links] == ["lo"], "non-loopback interface exposed")
    for version in ("-4", "-6"):
        routes = json.loads(subprocess.check_output(["ip", version, "-j", "route", "show", "table", "main"]))
        require(routes == [], "non-loopback route exposed")
    require(all(not Path(path).exists() for path in (
        "/run/user", "/run/dbus", "/home", "/var/run", "/relay", "/etc/resolv.conf",
    )), "unexpected host IPC, proxy, resolver or home mount")
    require(not any("proxy" in key.lower() for key in os.environ), "proxy environment exposed")
    require(os.environ.get("DBUS_SESSION_BUS_ADDRESS", "").startswith("unix:"), "private D-Bus missing")
    proof = Path("/proof")
    (proof / f"sandbox-{phase}.json").write_text(json.dumps({
        "loopbackOnly": True, "noMainRoutes": True, "noHostProxy": True, "privateBus": True,
    }), encoding="utf-8")
    # exec places the application at the exact checked boundary. There is no
    # proxy, fixture socket or route to enable before or after startup.
    os.execv("/probe", ["/probe", f"--{phase}", "/proof"])


def sandbox(binary, root, phase, screenshot):
    display = os.environ.get("DISPLAY", "")
    require(re.fullmatch(r":[0-9]+(?:\.0)?", display), "run under an independent Xvfb display")
    socket_file = "/tmp/.X11-unix/X" + display.split(":")[1].split(".")[0]
    require(Path(socket_file).is_socket(), "Xvfb socket missing")
    auth = Path(os.environ.get("XAUTHORITY", ""))
    require(auth.is_file(), "Xvfb authorization file missing")
    return [
        "bwrap", "--unshare-user", "--unshare-net", "--unshare-ipc", "--unshare-pid",
        "--die-with-parent", "--new-session", "--clearenv", "--ro-bind", "/usr", "/usr",
        "--symlink", "usr/lib", "/lib", "--symlink", "usr/lib64", "/lib64",
        "--symlink", "usr/bin", "/bin", "--dev", "/dev", "--proc", "/proc",
        "--tmpfs", "/tmp", "--dir", "/tmp/.X11-unix", "--ro-bind", socket_file, socket_file,
        "--ro-bind", str(auth.resolve()), "/xauth", "--dir", "/profile",
        "--perms", "0700", "--dir", "/runtime",
        "--ro-bind", str(root / "passwd"), "/etc/passwd",
        "--ro-bind", str(root / "group"), "/etc/group",
        "--ro-bind", "/etc/fonts", "/etc/fonts", "--ro-bind", "/etc/ld.so.cache", "/etc/ld.so.cache",
        "--ro-bind", "/etc/ssl/certs", "/etc/ssl/certs",
        "--bind", str(root), "/proof", "--ro-bind", str(binary), "/probe",
        "--ro-bind", str(SOURCE), "/worker.py",
        "--setenv", "PATH", "/usr/bin:/usr/sbin", "--setenv", "HOME", "/profile",
        "--setenv", "XDG_RUNTIME_DIR", "/runtime", "--setenv", "DISPLAY", display,
        "--setenv", "XAUTHORITY", "/xauth", "--setenv", "LANG", "en_US.UTF-8",
        "--setenv", "GDK_BACKEND", "x11", "--setenv", "LIBGL_ALWAYS_SOFTWARE", "1",
        *(["--setenv", "CUMULUS_COLD_SCREENSHOT", "1"] if screenshot else []),
        "--chdir", "/proof",
        "--", "/usr/bin/dbus-run-session", "--", "/usr/bin/python3", "/worker.py", "--child", phase,
    ]


def state_rows(root):
    # Compare logical SQLite state without writing it or emitting its values.
    output = {}
    for path in sorted((root / "state").rglob("core.sqlite3")):
        require(path.is_file() and not path.is_symlink(), "unexpected proof state path")
        connection = sqlite3.connect(path.resolve().as_uri() + "?mode=ro", uri=True)
        try:
            output[path.relative_to(root)] = connection.execute(
                "SELECT owner,kind,key,value FROM state ORDER BY owner,kind,key"
            ).fetchall()
        finally:
            connection.close()
    require(output, "seed state database missing")
    return output


def capture_panel(root):
    # This runs on the host, on the same private Xvfb display the sandbox
    # already uses. The application gains only a one-file acknowledgement.
    os.environ["GDK_BACKEND"] = "x11"
    import gi
    gi.require_version("Gdk", "3.0")
    from gi.repository import Gdk

    display = Gdk.Display.open(os.environ["DISPLAY"])
    require(display is not None, "could not open the private Xvfb display")
    window = display.get_default_screen().get_root_window()
    require(window is not None, "private Xvfb root window unavailable")
    picture = Gdk.pixbuf_get_from_window(window, 0, 0, window.get_width(), window.get_height())
    require(picture is not None, "offline panel capture unavailable")
    require(picture.savev(str(root / "cold-offline-panel.png"), "png", [], []),
            "could not save synthetic screenshot")
    (root / "panel-captured").write_text("captured\n", encoding="utf-8")


def launch(command, log, deadline, capture_root):
    if capture_root is None:
        return subprocess.run(command, stdout=log, stderr=subprocess.STDOUT, timeout=deadline).returncode
    process = subprocess.Popen(command, stdout=log, stderr=subprocess.STDOUT)
    expires = time.monotonic() + deadline
    try:
        while not (capture_root / "panel-ready").is_file():
            if process.poll() is not None:
                return process.returncode
            require(time.monotonic() < expires, "cold panel capture milestone timed out")
            time.sleep(.04)
        capture_panel(capture_root)
        return process.wait(timeout=max(.1, expires - time.monotonic()))
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()


def run(binary, root, seed_only, screenshot):
    root.mkdir(mode=0o700)
    root = root.resolve()
    uid, gid = os.getuid(), os.getgid()
    (root / "passwd").write_text(f"probe:x:{uid}:{gid}:Synthetic proof:/profile:/bin/false\n", encoding="utf-8")
    (root / "group").write_text(f"probe:x:{gid}:\n", encoding="utf-8")
    phases = (("seed", 20),) if seed_only else (("seed", 20), ("cold", 45))
    seed_rows = None
    for phase, deadline in phases:
        with (root / f"{phase}.log").open("wb") as log:
            capture = screenshot and phase == "cold"
            result = launch(sandbox(binary, root, phase, capture), log, deadline, root if capture else None)
        require(result == 0, f"{phase} process failed; see {phase}.log and {phase}.json")
        namespace = json.loads((root / f"sandbox-{phase}.json").read_text(encoding="utf-8"))
        require(namespace == {
            "loopbackOnly": True, "noMainRoutes": True, "noHostProxy": True, "privateBus": True,
        }, "missing verified startup isolation")
        if phase == "seed":
            seed_rows = state_rows(root)
    seed = json.loads((root / "seed.json").read_text(encoding="utf-8"))
    require(seed["published"] is True and seed["mediaTransfers"] >= 1, "seed was not downloaded and published")
    if seed_only:
        print("cold-offline seed proof passed: production publication inside no-egress namespace")
        return
    cold = json.loads((root / "cold.json").read_text(encoding="utf-8"))
    require(cold["stage"] == "complete" and cold["accountUnknown"] is True, "cold mode changed account state")
    require(cold["publishedIdMatched"] and cold["unknownIdDenied"] and cold["onlineLibraryDenied"]
            and cold["stoppedAndClosed"], "cold scope or playback lifecycle proof missing")
    require(cold["output"] == "null" and cold["nativePosition"] > cold["nativeStartPosition"] >= .12,
            "native playback progress missing")
    require(cold["transport"]["accountAttempts"] >= 1
            and cold["transport"]["otherHttpAttempts"] == 0
            and cold["transport"]["mediaAttempts"] == 0, "unexpected cold network path")
    require(cold["transport"]["pinnedLoads"] == 1 and cold["transport"]["ordinaryLoads"] == 0
            and cold["transport"]["observedNullAudio"], "native pinned/null output evidence missing")
    require(state_rows(root) == seed_rows, "cold launch changed durable account/download/player state")
    (root / "storage-audit.json").write_text(json.dumps({
        "unchanged": True, "databases": len(seed_rows),
    }), encoding="utf-8")
    print("cold-offline proof passed: distinct seed and cold launches, local native playback, no egress")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--root", type=Path)
    parser.add_argument("--child", choices=("seed", "cold"))
    parser.add_argument("--seed-only", action="store_true")
    parser.add_argument("--screenshot", action="store_true", help="capture the synthetic panel on private Xvfb")
    args = parser.parse_args()
    if args.child:
        child(args.child)
    else:
        require(args.binary and args.binary.is_file() and args.root and not args.root.exists(),
                "require an existing probe binary and a new proof directory")
        run(args.binary.resolve(), args.root, args.seed_only, args.screenshot)
