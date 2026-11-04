import json
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

from windows_system_master_volume_runtime import categorical_report, collect_sample


class CategoricalReportTests(unittest.TestCase):
    def test_accepts_only_supported_category_for_stage_and_hresult(self):
        for expected, stage, code in [
            ("endpoint_readable", None, None),
            ("default_endpoint_absent", "find_default_endpoint", "0x80070490"),
            ("backend_unknown", "find_default_endpoint", "0x88890010"),
            ("backend_unknown", "initialize_com", "0x80070490"),
            ("backend_unknown", "create_enumerator", "0x80070490"),
            ("endpoint_unreadable", "activate_volume", "0x80070490"),
            ("endpoint_unreadable", "get_mute", "0x88890004"),
            ("endpoint_unreadable", "invalid_scalar", None),
        ]:
            with self.subTest(stage=stage, code=code):
                sample = dict(kind=expected, stage=stage, hresult=code)
                self.assertEqual(categorical_report(json.dumps(sample)), sample)
                with self.assertRaises(ValueError):
                    categorical_report(json.dumps({**sample, "kind": "another_result"}))

    def test_rejects_raw_values_duplicates_and_invalid_code(self):
        for output in [
            '{"kind":"endpoint_readable","stage":null,"hresult":null,"scalar":0.375}',
            '{"kind":"endpoint_readable","stage":null,"hresult":null,"muted":false}',
            '{"kind":"endpoint_readable","stage":null,"hresult":null,"endpointId":"test-only-id"}',
            '{"kind":"endpoint_readable","kind":"endpoint_readable","stage":null,"hresult":null}',
            '{"kind":"backend_unknown","stage":"find_default_endpoint","hresult":"0x00000000"}',
            '{"kind":"backend_unknown","stage":"probe_timeout","hresult":null}',
            '{"kind":"backend_unknown","stage":{},"hresult":null}',
            " " * 257,
        ]:
            with self.subTest(output=output), self.assertRaises(ValueError):
                categorical_report(output)

    def test_timeout_is_not_an_endpoint_observation(self):
        with patch(
            "windows_system_master_volume_runtime.run_local_probe",
            side_effect=subprocess.TimeoutExpired("sealed_probe", timeout=10, output=b"private"),
        ):
            self.assertEqual(collect_sample(Path("sealed_probe")), {
                "kind": "backend_unknown", "stage": "probe_timeout",
                "hresult": None, "probeReturned": False,
            })

    def test_unexpected_child_diagnostics_are_not_used(self):
        with patch(
            "windows_system_master_volume_runtime.run_local_probe",
            return_value=subprocess.CompletedProcess(["sealed_probe"], 0, "", "private"),
        ), self.assertRaisesRegex(ValueError, "^unexpected probe process result$"):
            collect_sample(Path("sealed_probe"))
