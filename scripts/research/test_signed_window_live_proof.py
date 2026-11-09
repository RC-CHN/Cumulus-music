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
                     "selectedSongFound": True, "selectedFromPreview": True, "songDetailRead": True,
                     "mediaAuthorized": True, "mediaConsumed": True,
                     "closedAfterFailure": False, "searchV3": True, "searchTabRead": True,
                     "playlistPages": 1, "playlistCount": 2, "likedCount": 12,
                     "attemptedReads": 9, "deniedBeforeDispatch": 4}}


def discovery_sample():
    report = sample()
    for key in ("pauseHeldSameEntry", "resumedSameEntry", "stopped", "fullTimeline",
                "observedLibmpvNullClock", "playbackClocksMs"):
        del report[key]
    return dict(report, discoveryOnly=True, mediaAuthority="m7.music.126.net",
                stages=proof.DISCOVERY_STAGES.copy(), mediaReads=0, anonymousPinnedLoads=0)


def launcher_args(directory, *, discover_cdn=False):
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
    payload = {"keyword": "synthetic", "songId": "41"}
    if not discover_cdn:
        payload["mediaHost"] = "m1.music.126.net"
    intent.write_text(json.dumps(payload), encoding="ascii")
    intent.chmod(0o600)
    source = path / "source"
    source.mkdir(mode=0o700)
    return SimpleNamespace(authorize_readonly=True, source_stopped=True,
                           assets_sha256=ASSETS, output=path / "output", intent=intent,
                           pc_ipv4="1.1.1.1", cdn_ipv4=None if discover_cdn else "8.8.8.8",
                           discover_cdn=discover_cdn, probe=binary, profile_helper=helper,
                           source=source, expected_uid_file=uid, derive_if_missing=True,
                           probe_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
                           profile_helper_sha256=hashlib.sha256(helper.read_bytes()).hexdigest())


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
        for field, replacement in (("expectedAccountVerified", False), ("selectedFromPreview", False), ("searchTabRead", False),
                                   ("playlistPages", 17), ("attemptedReads", 26),
                                   ("unfinishedRead", "source")):
            report = sample()
            report["gate"][field] = replacement
            with self.assertRaises(proof.Refused):
                proof.accepted_report(report, ASSETS)
        with self.assertRaises(proof.Refused):
            proof.accepted_report(original, "0" * 64)

    def test_discovery_receipt_retains_only_canonical_authority_and_zero_media(self):
        original = discovery_sample()
        report = dict(original, mediaUrl="https://synthetic.invalid/secret", playbackClocksMs={"private": "unused"})
        self.assertEqual(proof.accepted_report(report, ASSETS, discover_cdn=True), original)
        with self.assertRaises(proof.Refused):
            proof.accepted_report(original, ASSETS)
        with self.assertRaises(proof.Refused):
            proof.accepted_report(sample(), ASSETS, discover_cdn=True)
        for host in (None, "m7.music.126.net:443", "https://m7.music.126.net/signed",
                     "m7.music.126.net.evil.test", "M7.music.126.net", "m7.music.126.net.",
                     "interfacepc.music.163.com", "127.0.0.1", "music.126.net"):
            with self.subTest(host=host), self.assertRaises(proof.Refused):
                proof.accepted_report(dict(original, mediaAuthority=host), ASSETS, discover_cdn=True)
        for field, value in (("discoveryOnly", False), ("mediaReads", 1), ("anonymousPinnedLoads", 1),
                             ("auxiliaryAttempts", 1), ("otherMediaAttempts", 1), ("stages", proof.STAGES)):
            with self.subTest(field=field), self.assertRaises(proof.Refused):
                proof.accepted_report(dict(original, **{field: value}), ASSETS, discover_cdn=True)
        for field in ("mediaAuthorized", "mediaConsumed"):
            report = discovery_sample()
            report["gate"][field] = False
            with self.subTest(field=field), self.assertRaises(proof.Refused):
                proof.accepted_report(report, ASSETS, discover_cdn=True)

    def test_supervisor_uses_exact_artifacts_and_cleans_staging(self):
        with tempfile.TemporaryDirectory() as directory:
            args = launcher_args(directory)
            path, helper, binary, uid, intent = (args.output.parent, args.profile_helper, args.probe,
                                                  args.expected_uid_file, args.intent)
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

            args.output = path / "failed-window"

            def failed_window(*arguments):
                window(*arguments)
                destination = arguments[3] / "report.json"
                report = json.loads(destination.read_text())
                report.update(ok=False, stages=proof.STAGES[:2], mediaReads=0)
                report["gate"]["attemptedReads"] = "synthetic private sentinel"
                report["gate"]["firstPlanRejection"] = "headers"
                report["gate"]["unfinishedRead"] = "source"
                destination.write_text(json.dumps(report))
                raise RuntimeError("synthetic private exception")

            with patch.dict(sys.modules, {"signed_window_sandbox": SimpleNamespace(run=failed_window)}), \
                    patch.object(proof, "verify_certificate", side_effect=preflight), \
                    patch.object(proof.subprocess, "run", side_effect=prepare), \
                    patch.object(proof.Broker, "summary", return_value=summary):
                with self.assertRaises(proof.Refused):
                    proof.run(args)
            failed = args.output / "result.json"
            receipt = proof.read_private_json(failed, proof.MAX_REPLY)
            self.assertEqual((receipt["ok"], receipt["stage"]), (False, "isolated_window"))
            self.assertEqual(receipt["checkpoint"]["completedStages"], 2)
            self.assertEqual(receipt["checkpoint"]["mediaReads"], 0)
            self.assertNotIn("attemptedReads", receipt["checkpoint"]["gate"])
            self.assertEqual(receipt["checkpoint"]["gate"]["firstPlanRejection"], "headers")
            self.assertEqual(receipt["checkpoint"]["gate"]["unfinishedRead"], "source")
            self.assertNotIn("synthetic private", failed.read_text())
            self.assertFalse(scratch[-1].exists())

            args.output = path / "wrong-hash"
            args.probe_sha256 = "0" * 64
            with patch.dict(sys.modules, {"signed_window_sandbox": SimpleNamespace(run=window)}), \
                    patch.object(proof, "verify_certificate") as verify:
                with self.assertRaises(proof.Refused):
                    proof.run(args)
            verify.assert_not_called()
            self.assertEqual(proof.read_private_json(args.output / "result.json", proof.MAX_REPLY),
                             {"ok": False, "stage": "seal_binaries"})

    def test_rejected_checkpoint_drops_unknown_or_structured_diagnostics(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory).resolve()
            report = sample()
            report.update(ok=False, stages=proof.STAGES[:1])
            report["gate"].update(firstPlanRejection={"headers": "synthetic private sentinel"},
                                  unfinishedRead="https://synthetic.invalid/secret")
            path = output / "report.json"
            path.write_text(json.dumps(report), encoding="utf-8")
            path.chmod(0o600)
            checkpoint = proof.rejected_checkpoint(output)
            self.assertEqual(checkpoint["completedStages"], 1)
            self.assertNotIn("firstPlanRejection", checkpoint["gate"])
            self.assertNotIn("unfinishedRead", checkpoint["gate"])
            self.assertNotIn("synthetic", json.dumps(checkpoint))

    def test_discovery_supervisor_pins_only_pc_and_refuses_any_cdn_dial(self):
        for media_dials in (0, 1):
            with self.subTest(media_dials=media_dials), tempfile.TemporaryDirectory() as directory:
                args = launcher_args(directory, discover_cdn=True)
                original = proof.read_private_json(args.intent, 4096)
                scratch = []
                broker_class = proof.Broker

                def pc_only(directory, pc, cdn):
                    self.assertEqual(pc, proof.Pin.pc(args.pc_ipv4))
                    self.assertIsNone(cdn)
                    return broker_class(directory, pc)

                def prepare(command, **kwargs):
                    self.assertEqual(proof.read_private_json(command[4], 4096),
                                     dict(original, mediaHost=proof.DISCOVERY_PROFILE_HOST))
                    command[5].mkdir(mode=0o700)
                    return subprocess.CompletedProcess(command, 0, b'{"ok":true,"identityWasDerived":true}')

                def window(probe, prepared, work, output, broker, *, discover_cdn):
                    self.assertIs(discover_cdn, True)
                    scratch.append(prepared.parent)
                    report = discovery_sample()
                    report["mediaUrl"] = "https://synthetic.invalid/never-retain"
                    destination = output / "report.json"
                    destination.write_text(json.dumps(report), encoding="utf-8")
                    destination.chmod(0o600)
                    return {"loopbackOnly": True, "mainRoutes": 0, "privateSessionBus": True,
                            "privateDisplay": True, "privateBroker": True,
                            "probeExit": 0, "bridgeConnections": 1 + media_dials}

                summary = {"pcDialAttempts": 1, "mediaDialAttempts": media_dials,
                           "deniedConnections": 0, "disarmed": True}
                with patch.dict(sys.modules, {"signed_window_sandbox": SimpleNamespace(run=window)}), \
                        patch.object(broker_class, "summary", return_value=summary), \
                        patch.object(proof, "Broker", side_effect=pc_only), \
                        patch.object(proof, "verify_certificate") as verify, \
                        patch.object(proof.subprocess, "run", side_effect=prepare):
                    if media_dials:
                        with self.assertRaises(proof.Refused):
                            proof.run(args)
                    else:
                        proof.run(args)
                    verify.assert_called_once_with(proof.Pin.pc(args.pc_ipv4))
                self.assertFalse(scratch[0].exists())
                self.assertEqual(proof.read_private_json(args.intent, 4096), original)
                receipt = args.output / "result.json"
                evidence = proof.read_private_json(receipt, proof.MAX_REPLY)
                self.assertNotIn("never-retain", receipt.read_text())
                if media_dials:
                    self.assertEqual((evidence["ok"], evidence["stage"]), (False, "verify_evidence"))
                    self.assertNotIn("m7.music.126.net", receipt.read_text())
                else:
                    self.assertTrue(evidence["ok"] and evidence["discoveryOnly"])
                    self.assertEqual(evidence["mediaAuthority"], "m7.music.126.net")
                    self.assertEqual(evidence["broker"]["mediaDialAttempts"], 0)

    def test_mode_mismatch_is_refused_before_any_certificate_check(self):
        for discover_cdn, cdn_ip, injected_host in ((True, "8.8.8.8", False), (False, None, False),
                                                   (True, None, True)):
            with self.subTest(discover_cdn=discover_cdn, cdn_ip=cdn_ip, injected_host=injected_host), \
                    tempfile.TemporaryDirectory() as directory:
                args = launcher_args(directory, discover_cdn=discover_cdn)
                args.cdn_ipv4 = cdn_ip
                if injected_host:
                    intent = proof.read_private_json(args.intent, 4096)
                    args.intent.write_text(json.dumps(dict(intent, mediaHost="m7.music.126.net")))
                with patch.dict(sys.modules, {"signed_window_sandbox": SimpleNamespace(run=None)}), \
                        patch.object(proof, "verify_certificate") as verify:
                    with self.assertRaises(proof.Refused):
                        proof.run(args)
                    verify.assert_not_called()
                self.assertEqual(proof.read_private_json(args.output / "result.json", proof.MAX_REPLY),
                                 {"ok": False, "stage": "validate_intent"})


if __name__ == "__main__":
    unittest.main()
