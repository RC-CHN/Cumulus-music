import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import signed_window_live_proof as proof

ASSETS = "f" * 64


def sample():
    return {"ok": True, "liveReadOnly": True, "productionWebView": True,
            "physicalAudioOutput": False, "isolatedNetwork": True, "searchAfterResources": True,
            "pauseHeldSameEntry": True, "resumedSameEntry": True, "stopped": True,
            "fullTimeline": True, "observedLibmpvNullClock": True,
            "stages": proof.STAGES.copy(), "mediaReads": 1, "anonymousPinnedLoads": 1,
            "playbackClocksMs": {"playing": 300, "paused": 510, "resumed": 760},
            "auxiliaryAttempts": 0, "otherMediaAttempts": 0,
            "assets": {"embedded": True, "entries": 31, "sha256": ASSETS},
            "gate": {"expectedAccountVerified": True, "comprehensiveRead": True,
                     "selectedSongFound": True, "songDetailRead": True,
                     "mediaAuthorized": True, "mediaConsumed": True,
                     "closedAfterFailure": False, "searchV3": True, "searchTabRead": True,
                     "playlistPages": 1, "playlistCount": 2, "likedCount": 12,
                     "attemptedReads": 9, "deniedBeforeDispatch": 4}}


class SignedWindowLauncherTests(unittest.TestCase):
    def test_missing_native_evidence_and_mismatched_assets_are_refused(self):
        original = sample()
        self.assertEqual(proof.accepted_report(original, ASSETS), original)
        for field, replacement in (("pauseHeldSameEntry", False), ("mediaReads", 2),
                                   ("otherMediaAttempts", 1), ("physicalAudioOutput", True),
                                   ("playbackClocksMs", {"playing": 300, "paused": 510, "resumed": 310})):
            report = sample()
            report[field] = replacement
            with self.assertRaises(proof.Refused):
                proof.accepted_report(report, ASSETS)
        for field, replacement in (("expectedAccountVerified", False), ("searchTabRead", False),
                                   ("playlistPages", 17), ("attemptedReads", 26)):
            report = sample()
            report["gate"][field] = replacement
            with self.assertRaises(proof.Refused):
                proof.accepted_report(report, ASSETS)
        with self.assertRaises(proof.Refused):
            proof.accepted_report(original, "0" * 64)

    def test_supervisor_uses_exact_artifacts_and_cleans_staging(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory).resolve()
            helper = path / "helper"
            binary = path / "probe"
            for file in (helper, binary):
                file.write_bytes(file.name.encode())
                file.chmod(0o700)
            uid = path / "uid"
            uid.write_text("123\n", encoding="ascii")
            uid.chmod(0o600)
            intent = path / "intent"
            intent.write_text(json.dumps({"keyword": "synthetic", "songId": "41",
                                          "mediaHost": "m1.music.126.net"}), encoding="ascii")
            intent.chmod(0o600)
            args = SimpleNamespace(authorize_readonly=True, source_stopped=True,
                                   assets_sha256=ASSETS, output=path / "output", intent=intent,
                                   pc_ipv4="1.1.1.1", cdn_ipv4="8.8.8.8", probe=binary,
                                   profile_helper=helper, source=path / "source",
                                   expected_uid_file=uid, derive_if_missing=True,
                                   probe_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
                                   profile_helper_sha256=hashlib.sha256(helper.read_bytes()).hexdigest())
            args.source.mkdir(mode=0o700)
            scratch = []
            order = []

            def preflight(_pin):
                order.append("tls")

            def prepare(command, **kwargs):
                order.append("prepare")
                self.assertEqual(command[1:4], ["--prepare", args.source, uid])
                self.assertNotEqual(command[4], intent)
                self.assertEqual(command[-1], "--derive-if-missing")
                self.assertEqual(command[0].read_bytes(), helper.read_bytes())
                self.assertNotIn("HTTPS_PROXY", kwargs["env"])
                command[5].mkdir(mode=0o700)
                return subprocess.CompletedProcess(command, 0, b'{"ok":true,"identityWasDerived":true}')

            def window(probe, prepared, work, output, broker):
                order.append("window")
                self.assertEqual(probe.read_bytes(), binary.read_bytes())
                self.assertTrue(prepared.is_dir() and work.is_dir())
                scratch.append(prepared.parent)
                self.assertTrue(broker.is_socket())  # Real broker, zero dials from this fixture.
                report = sample()
                report["notAReceiptField"] = "synthetic private sentinel"
                destination = output / "report.json"
                destination.write_text(json.dumps(report), encoding="utf-8")
                destination.chmod(0o600)
                return {"loopbackOnly": True, "mainRoutes": 0, "privateSessionBus": True,
                        "privateDisplay": True, "privateBroker": True,
                        "probeExit": 0, "bridgeConnections": 2}

            # Simulate independently measured dial counters; no real network is used.
            summary = {"pcDialAttempts": 1, "mediaDialAttempts": 1,
                       "deniedConnections": 0, "disarmed": True}
            with patch.dict(sys.modules, {"signed_window_sandbox": SimpleNamespace(run=window)}), \
                    patch.object(proof, "verify_certificate", side_effect=preflight), \
                    patch.object(proof.subprocess, "run", side_effect=prepare), \
                    patch.object(proof.Broker, "summary", return_value=summary):
                proof.run(args)
            self.assertEqual(order, ["tls", "tls", "prepare", "window"])
            self.assertFalse(scratch[0].exists())
            result = args.output / "result.json"
            evidence = proof.read_private_json(result, proof.MAX_REPLY)
            self.assertTrue(evidence["ok"] and evidence["temporaryProfileRemoved"] and evidence["identityWasDerived"])
            self.assertNotIn("synthetic private sentinel", result.read_text())
            self.assertEqual(evidence["broker"], summary)

            args.output = path / "wrong-hash"
            args.probe_sha256 = "0" * 64
            with patch.dict(sys.modules, {"signed_window_sandbox": SimpleNamespace(run=window)}), \
                    patch.object(proof, "verify_certificate") as verify:
                with self.assertRaises(proof.Refused):
                    proof.run(args)
            verify.assert_not_called()
            self.assertEqual(proof.read_private_json(args.output / "result.json", proof.MAX_REPLY),
                             {"ok": False, "stage": "seal_binaries"})


if __name__ == "__main__":
    unittest.main()
