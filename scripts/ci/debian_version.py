"""Keep the Debian upgrade order while leaving application SemVer unchanged."""
import gzip
import io
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tempfile

from release_guard import VERSION


TAURI_MEMBERS = ["debian-binary", "control.tar.gz", "data.tar.gz"]


def debian_version(version):
    assert VERSION.fullmatch(version), "invalid package version"
    # The last dash is a Debian revision separator, so a literal -rc.N
    # would sort after the final release. Tilde sorts before end-of-version.
    return version.replace("-rc.", "~rc.")


def require_control_version(package, expected):
    actual = subprocess.check_output(["dpkg-deb", "--field", package, "Version"],
                                     text=True, encoding="utf-8").strip()
    assert actual == expected, f"Debian Version is {actual!r}, expected {expected!r}"


def verify_debian_version(package, version):
    expected = debian_version(version)
    require_control_version(package, expected)
    return expected


def rewrite_control_tar(source, destination, version, target):
    with tarfile.open(source, "r:gz") as original:
        members = original.getmembers()
        controls = [member for member in members if member.name in {"control", "./control"}]
        assert len(controls) == 1 and controls[0].isreg(), "expected one Debian control file"
        control = controls[0]
        with original.extractfile(control) as stream:
            data = stream.read()
        fields = list(re.finditer(rb"^Version:[^\n]*$", data, re.MULTILINE | re.IGNORECASE))
        assert len(fields) == 1 and fields[0][0] == f"Version: {version}".encode("ascii"), (
            "unexpected generated Debian Version field")
        field = fields[0]
        replacement = f"Version: {target}".encode("ascii")
        data = data[:field.start()] + replacement + data[field.end():]
        assert len(data) == control.size  # -rc.N and ~rc.N have the same byte length.
        with destination.open("xb") as compressed, gzip.GzipFile(
            filename="", mode="wb", fileobj=compressed, mtime=0,
        ) as stream, tarfile.open(fileobj=stream, mode="w") as updated:
            for member in members:
                if member is control:
                    updated.addfile(member, io.BytesIO(data))
                elif member.isreg():
                    with original.extractfile(member) as content:
                        updated.addfile(member, content)
                else:
                    updated.addfile(member)


def stage_debian_installer(source, destination, version):
    target = debian_version(version)
    require_control_version(source, version)
    if target == version:
        shutil.copy2(source, destination)
    else:
        # Tauri CLI 2.12.4 emits exactly these three members. Replace only the
        # control tar: the original compressed payload (including executable,
        # permissions and native runtime) and its md5sums remain unchanged.
        source = source.resolve()
        members = subprocess.check_output(["ar", "t", source], text=True, encoding="ascii").splitlines()
        assert members == TAURI_MEMBERS, "unexpected Tauri Debian archive layout"
        with tempfile.TemporaryDirectory(prefix="cumulus-debian-", dir=destination.parent) as temporary:
            staging = Path(temporary)
            subprocess.run(["ar", "x", source], cwd=staging, check=True)
            control = staging / "control.tar.gz"
            original = staging / "original-control.tar.gz"
            control.rename(original)
            rewrite_control_tar(original, control, version, target)
            package = staging / "normalized.deb"
            subprocess.run(["ar", "rcD", package, *members], cwd=staging, check=True)
            require_control_version(package, target)
            package.replace(destination)
    verify_debian_version(destination, version)
