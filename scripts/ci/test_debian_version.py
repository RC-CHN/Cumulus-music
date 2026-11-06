import io
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest

from debian_version import stage_debian_installer, verify_debian_version


def add_file(archive, name, data, *, mode=0o644):
    item = tarfile.TarInfo(name)
    item.size = len(data)
    item.mode = mode
    item.mtime = 42
    archive.addfile(item, io.BytesIO(data))


def tauri_deb(directory, version, *, extra_member=False):
    (directory / "debian-binary").write_bytes(b"2.0\n")
    with tarfile.open(directory / "control.tar.gz", "w:gz") as archive:
        add_file(archive, "./control", (
            f"Package: cumulus\nVersion: {version}\nArchitecture: amd64\n"
            "Maintainer: Cumulus Music\nDescription: synthetic package\n"
            " Version: descriptive continuation, not a field\n"
        ).encode("ascii"))
        add_file(archive, "./postinst", b"#!/bin/sh\nexit 0\n", mode=0o755)
    with tarfile.open(directory / "data.tar.gz", "w:gz") as archive:
        add_file(archive, "./usr/bin/cumulus", b"fixture executable", mode=0o755)
    package = directory / "tauri.deb"
    members = ["debian-binary", "control.tar.gz", "data.tar.gz"]
    if extra_member:
        (directory / "unexpected").write_bytes(b"unexpected archive member")
        members.append("unexpected")
    subprocess.run(["ar", "rcD", package, *members], cwd=directory, check=True)
    return package


def member(package, name):
    return subprocess.check_output(["ar", "p", package, name])


@unittest.skipUnless(all(shutil.which(tool) for tool in ("ar", "dpkg", "dpkg-deb")),
                     "requires Debian package tools and GNU ar")
class DebianVersionTest(unittest.TestCase):
    def test_rc_sorts_before_final_and_preserves_the_tauri_payload(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            original = tauri_deb(directory, "0.2.0-rc.1")
            target = directory / "Cumulus-0.2.0-rc.1-linux-x64.deb"
            with self.assertRaisesRegex(AssertionError, "expected '0.2.0~rc.1'"):
                verify_debian_version(original, "0.2.0-rc.1")
            stage_debian_installer(original, target, "0.2.0-rc.1")
            version = verify_debian_version(target, "0.2.0-rc.1")
            subprocess.run(["dpkg", "--compare-versions", "0.1.0", "lt", version], check=True)
            subprocess.run(["dpkg", "--compare-versions", version, "lt", "0.2.0"], check=True)
            self.assertEqual(member(original, "debian-binary"), member(target, "debian-binary"))
            self.assertEqual(member(original, "data.tar.gz"), member(target, "data.tar.gz"))
            with tarfile.open(fileobj=io.BytesIO(member(target, "control.tar.gz")), mode="r:gz") as archive:
                self.assertEqual(archive.getmember("./postinst").mode, 0o755)
                self.assertEqual(archive.getmember("./postinst").mtime, 42)
                self.assertEqual(archive.extractfile("./postinst").read(), b"#!/bin/sh\nexit 0\n")
                self.assertIn(b"\n Version: descriptive continuation, not a field\n",
                              archive.extractfile("./control").read())

    def test_stable_archive_is_copied_without_rewriting(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            original = tauri_deb(directory, "0.2.0")
            target = directory / "stable.deb"
            stage_debian_installer(original, target, "0.2.0")
            self.assertEqual(original.read_bytes(), target.read_bytes())

    def test_rejects_wrong_input_version_and_unexpected_archive_shape(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            original = tauri_deb(directory, "0.2.0-rc.2", extra_member=True)
            target = directory / "release.deb"
            with self.assertRaisesRegex(AssertionError, "expected '0.2.0-rc.1'"):
                stage_debian_installer(original, target, "0.2.0-rc.1")
            with self.assertRaisesRegex(AssertionError, "unexpected Tauri Debian archive layout"):
                stage_debian_installer(original, target, "0.2.0-rc.2")
            self.assertFalse(target.exists())
