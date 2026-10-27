"""Build Linux/Windows packages and exercise their actual media dependency set.

No account, recovery archive or live music request is used. Run from repo root.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tarfile
import zipfile

from release_guard import check_versions, git

ROOT = Path(__file__).resolve().parents[2]


def run(args, **kwargs):
    print("+", " ".join(map(str, args)), flush=True)
    return subprocess.run(list(map(str, args)), check=True, **kwargs)


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def windows_media(destination, msys):
    """Copy the PE import closure, not the whole compiler/MSYS2 installation."""
    binary = msys / "ucrt64/bin"
    candidates = {p.name.lower(): p for pattern in ("*.dll", "ffprobe.exe") for p in binary.glob(pattern)}
    assert "libmpv-2.dll" in candidates, "MSYS2 UCRT64 libmpv is missing"
    assert "ffprobe.exe" in candidates, "MSYS2 UCRT64 ffprobe is missing"
    pending, copied = ["libmpv-2.dll", "ffprobe.exe"], set()
    while pending:
        name = pending.pop()
        if name in copied:
            continue
        source = candidates.get(name)
        if source is None:
            assert name.startswith(("api-ms-", "ext-ms-")) or (Path(os.environ["SystemRoot"]) / "System32" / name).is_file(), f"unresolved Windows DLL: {name}"
            continue
        shutil.copy2(source, destination / source.name)
        copied.add(name)
        imports = subprocess.check_output([str(binary / "objdump.exe"), "-p", str(source)], text=True, encoding="utf-8", errors="replace")
        pending.extend(name.lower() for name in re.findall(r"DLL Name:\s*(\S+)", imports))
    licenses = msys / "ucrt64/share/licenses"
    assert licenses.is_dir(), "MSYS2 license directory missing"
    shutil.copytree(licenses, destination / "third-party/licenses")
    packages = subprocess.check_output([str(msys / "usr/bin/pacman.exe"), "-Q"], text=True, encoding="utf-8")
    (destination / "third-party/msys2-packages.txt").write_text(packages, encoding="utf-8")
    (destination / "third-party/SOURCES.txt").write_text(
        "Runtime DLLs are from MSYS2 UCRT64, with exact installed versions in msys2-packages.txt.\n"
        "Package metadata, source archives and recipe history: https://packages.msys2.org/\n"
        "Source recipes: https://github.com/msys2/MINGW-packages\n"
        "libmpv: https://github.com/mpv-player/mpv ; FFmpeg: https://ffmpeg.org/\n"
        "The included licenses retain their upstream terms.\n", encoding="utf-8")
    return sorted(name for name in copied if name.endswith(".dll"))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platform", choices=["linux-x64", "windows-x64"], required=True)
    args = parser.parse_args()
    os.chdir(ROOT)
    windows = args.platform == "windows-x64"
    assert windows == (sys.platform == "win32"), "build on the target platform"
    version = check_versions()
    sha = git("rev-parse", "HEAD")
    assert not git("status", "--porcelain", "--untracked-files=normal"), "commit source changes before building release packages"
    target = Path(os.environ.get("CARGO_TARGET_DIR", ".build/target")).resolve()
    stage = ROOT / ".build/package-stage" / args.platform
    output = ROOT / ".build/packages" / args.platform
    # Never silently replace a package/proof directory from another invocation.
    stage.mkdir(parents=True, exist_ok=False)
    output.mkdir(parents=True, exist_ok=False)
    native = stage / "native"
    native.mkdir()
    suffix = ".exe" if windows else ""
    filter_name = "cumulus_audio_filter.dll" if windows else "libcumulus_audio_filter.so"
    run(["cargo", "build", "--release", "--locked", "-p", "cumulus-core", "-p", "cumulus-audio-filter", "--lib", "--example", "engine_roundtrip", "--example", "player_probe"])
    shutil.copy2(target / "release" / filter_name, native / filter_name)
    dlls = windows_media(native, Path(os.environ["CUMULUS_MSYS2_ROOT"])) if windows else []
    config = {"build": {"beforeBuildCommand": ""}, "bundle": {"active": True,
        "resources": {str(native): "" if windows else "native", str(ROOT / "LICENSE"): "LICENSE", str(ROOT / f"docs/releases/{version}.txt"): "README.txt"},
        "shortDescription": "Community desktop music client",
        "longDescription": "An independent Tauri and Rust community music client. See the included release notes for supported platforms and known limitations.",
        "publisher": "Cumulus Music",
        "licenseFile": str(ROOT / "LICENSE"),
        "copyright": "Cumulus Music contributors",
        "category": "Music",
        "linux": {"deb": {"depends": ["libmpv2", "ffmpeg"]}},
        "windows": {"nsis": {"installMode": "currentUser", "languages": ["SimpChinese", "English"]}}}}
    config_path = stage / "bundle-config.json"
    config_path.write_text(json.dumps(config, indent=2), encoding="utf-8")
    run(["node", ROOT / "frontend/node_modules/@tauri-apps/cli/tauri.js", "build", "--ci", "--bundles", "nsis" if windows else "deb", "--config", config_path, "--", "--locked"], cwd=ROOT / "desktop")

    portable = stage / f"Cumulus-{version}-{args.platform}"
    portable.mkdir()
    shutil.copytree(native, portable, dirs_exist_ok=True)
    shutil.copy2(target / "release" / f"cumulus{suffix}", portable)
    shutil.copy2(ROOT / "LICENSE", portable)
    shutil.copy2(ROOT / f"docs/releases/{version}.txt", portable / "README.txt")
    # Test against the exact portable dependency directory. On Windows remove
    # MSYS2 from PATH, so a missing bundled DLL cannot be masked by the runner.
    env = os.environ.copy()
    if windows:
        system = Path(os.environ["SystemRoot"])
        env["PATH"] = os.pathsep.join(map(str, [portable, system / "System32", system]))
    probe = portable / f"engine_roundtrip{suffix}"
    shutil.copy2(target / "release/examples" / probe.name, probe)
    media = run([probe], env=env, stdout=subprocess.PIPE, text=True, encoding="utf-8")
    media_report = json.loads(media.stdout)
    assert media_report["ok"] and media_report["remote_requests"] == 0
    probe.unlink()
    # The Linux distribution FFmpeg includes LADSPA. MSYS2's FFmpeg currently
    # does not; do not advertise a working Windows live DSP chain from unit tests.
    eq_report = {"tested": False, "reason": "MSYS2 FFmpeg does not enable LADSPA"}
    if not windows:
        eq = run([target / "release/examples/player_probe", portable / filter_name, stage / "equalizer-proof"], stdout=subprocess.PIPE, text=True, encoding="utf-8")
        eq_report = json.loads(eq.stdout)
        assert eq_report["ok"] and eq_report["remote_requests"] == 0
    (portable / "runtime-manifest.json").write_text(json.dumps({
        "version": version, "commit": sha, "platform": args.platform, "dlls": dlls,
        "nativePlayback": media_report, "equalizerPlayback": eq_report,
    }, indent=2), encoding="utf-8")
    archive_base = f"Cumulus-{version}-{args.platform}"
    if windows:
        with zipfile.ZipFile(output / f"{archive_base}.zip", "w", zipfile.ZIP_DEFLATED) as archive:
            for file in sorted(portable.rglob("*")):
                if file.is_file():
                    archive.write(file, file.relative_to(portable.parent))
    else:
        with tarfile.open(output / f"{archive_base}.tar.gz", "w:gz") as archive:
            archive.add(portable, arcname=portable.name)
    installers = list((target / "release/bundle" / ("nsis" if windows else "deb")).glob("*.exe" if windows else "*.deb"))
    assert len(installers) == 1, f"expected one installer, found {len(installers)}"
    shutil.copy2(installers[0], output / f"{archive_base}-setup.exe" if windows else output / f"{archive_base}.deb")
    assert git("rev-parse", "HEAD") == sha and not git("status", "--porcelain", "--untracked-files=normal"), "source changed during packaging"
    manifest = {"version": version, "commit": sha, "platform": args.platform,
        "ciRun": os.environ.get("GITHUB_RUN_ID"), "ciAttempt": os.environ.get("GITHUB_RUN_ATTEMPT"),
        "nativePlayback": media_report, "equalizerPlayback": eq_report,
        "assets": [{"name": p.name, "bytes": p.stat().st_size, "sha256": digest(p)} for p in sorted(output.iterdir())]}
    (output / f"manifest-{args.platform}.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"ok": True, "version": version, "platform": args.platform, "commit": sha}))


if __name__ == "__main__":
    main()
