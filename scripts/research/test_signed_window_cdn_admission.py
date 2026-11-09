"""One-shot host CDN admission using sealed synthetic authority messages."""

import json
import os
from pathlib import Path
import stat
import subprocess
import tempfile
import time
import unittest
from unittest.mock import patch

from signed_window_broker import Broker, Pin
from signed_window_cdn_admission import ACK, REQUEST, AdmissionRefused, CdnAdmission, resolve_ipv4
from signed_window_tls_fence import Denied

HOST = "m7.music.126.net"
ADDRESS = "8.8.8.8"


class CdnAdmissionTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.output = self.root / "proof"
        self.relay = self.root / "relay"
        for path in (self.output, self.relay):
            path.mkdir(mode=0o700)
        self.request = self.output / REQUEST
        self.ack = self.output / ACK
        self.lookup = []
        self.verified = []

    def resolve(self, host, seconds):
        self.lookup.append(host)
        self.assertGreater(seconds, 0)
        self.assertLessEqual(seconds, 3)
        return ADDRESS

    def verify(self, pin, seconds):
        self.verified.append(pin)
        self.assertGreater(seconds, 0)
        self.assertLessEqual(seconds, 5)

    def broker(self):
        def no_network(*_):
            self.fail("admission must not open a media tunnel")
        return Broker(self.relay, Pin.pc("1.1.1.1"), dynamic_cdn=True, dial=no_network)

    def admission(self, broker, verify=None):
        return CdnAdmission(self.output, broker, resolve=self.resolve, verify=verify or self.verify)

    def create(self, payload=None, *, mode=0o600, path=None):
        target = path or self.request
        target.write_bytes(payload if payload is not None else json.dumps({"authority": HOST}).encode())
        target.chmod(mode)

    def test_one_verified_exact_request_and_no_rearm_if_it_changes(self):
        with self.broker() as broker, self.admission(broker) as admission:
            deadline = time.monotonic() + 20
            admission.observe(deadline)
            self.assertFalse(self.ack.exists())
            self.create()
            admission.observe(deadline)
            admission.observe(deadline)
            self.assertEqual(json.loads(self.ack.read_bytes()), {"authority": HOST})
            self.assertEqual(stat.S_IMODE(self.ack.stat().st_mode), 0o600)
            self.assertEqual(self.ack.stat().st_nlink, 1)
            self.assertEqual(admission.summary(), {
                "requestAccepted": True, "dnsLookups": 1, "certificatePreflightAttempts": 1,
                "pinArmed": True, "ackPublished": True,
            })
            self.assertEqual(self.lookup, [HOST])
            self.assertEqual(self.verified, [Pin.cdn(HOST, ADDRESS)])
            self.create(b'{"authority":"m8.music.126.net"}')
            with self.assertRaises(AdmissionRefused):
                admission.observe(deadline)
            self.assertEqual(admission.summary()["failure"], "changed_record")
            self.assertTrue(broker.summary()["disarmed"])
            with self.assertRaises(AdmissionRefused):
                admission.observe(deadline)
            self.assertEqual(self.lookup, [HOST])
            self.assertEqual(broker.summary()["mediaDialAttempts"], 0)

    def test_malformed_unsafe_or_unexpected_input_never_reaches_dns(self):
        cases = (
            "public-file", "oversized", "duplicate-json-key", "raw-url",
            "unapproved-suffix", "ip-literal", "symlink", "hardlink", "preexisting-ack",
        )
        for case in cases:
            with self.subTest(case=case), self.broker() as broker, self.admission(broker) as admission:
                if case == "public-file":
                    self.create(mode=0o644)
                elif case == "oversized":
                    self.create(b" " * 257)
                elif case == "duplicate-json-key":
                    self.create(b'{"authority":"m7.music.126.net","authority":"m8.music.126.net"}')
                elif case == "raw-url":
                    self.create(b'{"authority":"https://m7.music.126.net/synthetic?query=private"}')
                elif case == "unapproved-suffix":
                    self.create(b'{"authority":"m7.music.126.net.example.com"}')
                elif case == "ip-literal":
                    self.create(b'{"authority":"8.8.8.8"}')
                elif case in ("symlink", "hardlink"):
                    external = self.root / "outside"
                    self.create(path=external)
                    if case == "symlink":
                        self.request.symlink_to(external)
                    else:
                        os.link(external, self.request)
                else:
                    self.create()
                    self.create(path=self.ack)
                with self.assertRaises(AdmissionRefused):
                    admission.observe(time.monotonic() + 20)
                self.assertEqual(admission.summary()["failure"], "request")
                self.assertEqual(self.lookup, [])
                self.assertEqual(self.verified, [])
                self.assertTrue(broker.summary()["disarmed"])
                self.request.unlink(missing_ok=True)
                self.ack.unlink(missing_ok=True)
                (self.root / "outside").unlink(missing_ok=True)

    def test_certificate_failure_and_changed_request_do_not_arm_or_ack(self):
        def reject(_pin, _seconds):
            raise OSError("synthetic confidential exception")
        def replace(_pin, _seconds):
            self.create(b'{"authority":"m8.music.126.net"}')
        for verify, category in ((reject, "certificate"), (replace, "arm")):
            with self.subTest(category=category), self.broker() as broker, self.admission(broker, verify) as admission:
                self.create()
                with self.assertRaisesRegex(AdmissionRefused, "private CDN admission stopped"):
                    admission.observe(time.monotonic() + 20)
                self.assertEqual(admission.summary()["failure"], category)
                self.assertEqual(admission.summary()["certificatePreflightAttempts"], 1)
                self.assertFalse(admission.summary()["pinArmed"] or self.ack.exists())
                self.assertTrue(broker.summary()["disarmed"])
                self.request.unlink()

    def test_bounded_resolver_keeps_one_public_pin_and_refuses_mixed_answers(self):
        answers = subprocess.CompletedProcess([], 0, b'["9.9.9.9", "8.8.8.8"]')
        with patch("signed_window_cdn_admission.subprocess.run", return_value=answers) as run:
            self.assertEqual(resolve_ipv4(HOST, 1.25), ADDRESS)
            self.assertEqual(run.call_args.kwargs["input"], HOST.encode())
            self.assertEqual(run.call_args.kwargs["timeout"], 1.25)
            self.assertNotIn(HOST, run.call_args.args[0])
            run.assert_called_once()
        mixed = subprocess.CompletedProcess([], 0, b'["8.8.8.8", "192.168.1.2"]')
        with patch("signed_window_cdn_admission.subprocess.run", return_value=mixed):
            with self.assertRaises(Denied):
                resolve_ipv4(HOST, 1)


if __name__ == "__main__":
    unittest.main()
