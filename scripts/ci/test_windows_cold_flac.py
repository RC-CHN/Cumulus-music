import hashlib
import io
import json
from pathlib import Path
from subprocess import CompletedProcess
import tempfile
import unittest
from unittest.mock import patch

import windows_cold_flac as proof


class WindowsColdFlacTests(unittest.TestCase):
    def test_cargo_builds_the_standalone_core_library_test_harness(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            (repo / 'core.exe').write_bytes(b'core libtest')
            artifacts = [
                {'reason': 'compiler-artifact', 'target': {'name': 'cumulus_core'},
                 'executable': 'not-a-test.exe', 'profile': {'test': False}},
                {'reason': 'compiler-artifact', 'target': {'name': 'other'},
                 'executable': 'dependency-test.exe', 'profile': {'test': True}},
                {'reason': 'compiler-artifact', 'target': {'name': 'cumulus_core'},
                 'executable': 'core.exe', 'profile': {'test': True}},
            ]
            output = '\n'.join(json.dumps(item) for item in artifacts)
            with patch.object(proof.subprocess, 'run', return_value=CompletedProcess(
                [], 0, output, 'cargo diagnostic\n',
            )) as run:
                log = io.StringIO()
                self.assertEqual(proof.test_executable(log, repo=repo), (repo / 'core.exe').resolve())
            self.assertEqual(log.getvalue(), 'cargo diagnostic\n')
            self.assertEqual(run.call_args.args[0], [
                'cargo', 'test', '--locked', '-p', 'cumulus-core', '--lib', '--no-run',
                '--message-format=json',
            ])
            self.assertEqual(run.call_args.kwargs['cwd'], repo)

    def test_ci_requires_the_packaged_dll_before_any_proof_fallback(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(proof, 'ensure_flac') as fallback:
            root = Path(directory)
            package = root / 'package'
            package.mkdir()
            for name in {proof.MPV, *proof.FLAC_IMPORTS}:
                (package / name).write_bytes(b'synthetic DLL')
            with self.assertRaisesRegex(ValueError, 'packaged libFLAC is required'):
                proof.stage_runtime(package, root, root, require_package_flac=True)
            fallback.assert_not_called()

    def test_rejects_an_unverified_dll_already_in_the_package(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(proof, 'download') as download:
            root = Path(directory)
            target = root / proof.FLAC
            target.write_bytes(b'incidental DLL')
            with self.assertRaisesRegex(ValueError, 'package-stage libFLAC'):
                proof.ensure_flac(root, root, root)
            download.assert_not_called()
            self.assertEqual(target.read_bytes(), b'incidental DLL')

    def test_reuses_installed_ucrt_only_when_the_binary_hash_matches(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            runtime = root / 'runtime'
            runtime.mkdir()
            installed = root / 'ucrt64/bin' / proof.FLAC
            installed.parent.mkdir(parents=True)
            installed.write_bytes(b'audited binary')
            digest = hashlib.sha256(installed.read_bytes()).hexdigest()
            with patch.object(proof, 'FLAC_DLL_SHA256', digest), patch.object(proof, 'download') as download:
                self.assertEqual(proof.ensure_flac(runtime, root, root), 'msys2-ucrt64-installed')
                self.assertEqual((runtime / proof.FLAC).read_bytes(), installed.read_bytes())
                download.assert_not_called()

    def test_mismatched_installed_dll_extracts_only_the_locked_archive_member(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            runtime = root / 'runtime'
            runtime.mkdir()
            installed = root / 'ucrt64/bin' / proof.FLAC
            installed.parent.mkdir(parents=True)
            installed.write_bytes(b'not the audited DLL')
            pinned = b'audited binary'
            digest = hashlib.sha256(pinned).hexdigest()
            with patch.object(proof, 'FLAC_DLL_SHA256', digest), patch.object(proof, 'download') as download, \
                    patch.object(proof.subprocess, 'check_output', side_effect=['/proof/package\n', pinned]) as execute:
                self.assertEqual(proof.ensure_flac(runtime, root, root), 'pinned-msys2-archive')
            download.assert_called_once_with(proof.FLAC_URL, root / proof.FLAC_ARCHIVE, proof.FLAC_ARCHIVE_SHA256)
            self.assertEqual(execute.call_args.args[0][1:], ['-xOf', '/proof/package', f'ucrt64/bin/{proof.FLAC}'])
            self.assertEqual((runtime / proof.FLAC).read_bytes(), pinned)
            self.assertEqual(installed.read_bytes(), b'not the audited DLL')

    def test_requires_exact_test_result_and_native_identity_receipt(self):
        name = proof.IDENTITY
        missing = CompletedProcess([], 0, 'test result: ok. 0 passed; 0 failed; 0 ignored')
        with self.assertRaisesRegex(RuntimeError, 'did not pass exactly once'):
            proof.case_result(missing, name)
        success = f'test {name} ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored'
        with self.assertRaisesRegex(ValueError, 'identity receipt is missing'):
            proof.case_result(CompletedProcess([], 0, success), name)
        identity = {'libflacVersion': '1.5.0', 'relativeLookupRejected': True}
        stdout = f'{proof.RECEIPT}{json.dumps(identity)}\n{success}'
        self.assertEqual(proof.case_result(CompletedProcess([], 0, stdout), name), identity)

    def test_gui_runs_before_temporary_runtime_is_removed_with_explicit_verified_paths(self):
        with tempfile.TemporaryDirectory() as directory:
            outer = Path(directory)
            gui_runs = []

            def staged(package, msys, working, *, require_package_flac):
                self.assertFalse(require_package_flac)
                runtime = working / 'runtime'
                runtime.mkdir()
                for name in (proof.FLAC, proof.MPV):
                    (runtime / name).write_bytes(b'synthetic verified runtime')
                dlls = {name: proof.sha256(runtime / name) for name in (proof.FLAC, proof.MPV)}
                return runtime, 'synthetic', dlls, []

            def execute(command, **kwargs):
                if command[0] == proof.sys.executable:
                    runtime = Path(command[command.index('--runtime') + 1])
                    flac = Path(command[command.index('--stereo-pcm-flac-library') + 1])
                    mpv = Path(command[command.index('--mpv-library') + 1])
                    self.assertTrue(flac.is_file() and mpv.is_file())
                    self.assertEqual(flac, runtime / proof.FLAC)
                    self.assertEqual(mpv, runtime / proof.MPV)
                    self.assertNotIn(str(runtime), kwargs['env']['PATH'])
                    report = Path(command[command.index('--report') + 1])
                    report.write_text(json.dumps({'nativeRuntime': {
                        'kind': 'explicit_windows_runtime', 'libmpvModule': str(mpv)}}))
                    gui_runs.append(runtime)
                return CompletedProcess(command, 0, '')

            with patch.object(proof, 'stage_runtime', side_effect=staged), \
                    patch.object(proof, 'test_executable', return_value=outer / 'tests.exe'), \
                    patch.object(proof, 'case_result', return_value=None), \
                    patch.object(proof.subprocess, 'run', side_effect=execute), \
                    patch.dict(proof.os.environ, {'SystemRoot': str(outer / 'Windows')}):
                proof.verify(outer / 'proof', outer, outer, outer / 'native_audio_probe.exe')
            self.assertEqual(len(gui_runs), 1)
            self.assertFalse(gui_runs[0].exists())
            report = json.loads((outer / 'proof/report.json').read_text())
            self.assertTrue(report['ok'] and report['stereoPcmGui']['ok'])
            self.assertEqual(report['coreTestBinary'], 'local-cargo')
            self.assertIn(f'{proof.COLD}::stereo::cold_stereo_unknown_length_native_pause_controls_seek_tail_and_revocation', report['passed'])

    def test_prebuilt_harness_runs_every_exact_case_without_a_cargo_build(self):
        with tempfile.TemporaryDirectory() as directory:
            outer = Path(directory)
            harness = outer / 'prebuilt.exe'
            harness.write_bytes(b'synthetic verified test harness')

            def staged(package, msys, working, *, require_package_flac):
                self.assertTrue(require_package_flac)
                return working, 'package-stage', {}, []

            with patch.object(proof, 'stage_runtime', side_effect=staged), \
                    patch.object(proof, 'test_executable') as cargo, \
                    patch.object(proof, 'case_result', return_value=None), \
                    patch.object(proof.subprocess, 'run', return_value=CompletedProcess([], 0, '')) as run, \
                    patch.dict(proof.os.environ, {'SystemRoot': str(outer / 'Windows')}):
                proof.verify(outer / 'proof', outer, outer, core_test_binary=harness,
                             require_package_flac=True)
            cargo.assert_not_called()
            self.assertEqual([call.args[0] for call in run.call_args_list], [
                [harness, '--exact', case, '--include-ignored', '--nocapture', '--color', 'never']
                for case in proof.CASES
            ])
            rooted_wav, = [call for call in run.call_args_list
                           if call.args[0][2] == proof.ROOTED_WAV_EQUALIZER]
            self.assertEqual(rooted_wav.kwargs['env']['CUMULUS_LIMITER_PROBE_MPV'],
                             str(rooted_wav.kwargs['cwd'] / proof.MPV))
            report = json.loads((outer / 'proof/report.json').read_text())
            self.assertEqual(report['coreTestBinary'], 'prebuilt')
            self.assertTrue(report['libflacPackaged'])
            self.assertEqual(report['passed'], list(proof.CASES))

    def test_report_rejects_a_loaded_dll_from_outside_the_runtime(self):
        with tempfile.TemporaryDirectory() as directory:
            runtime = Path(directory) / 'runtime'
            runtime.mkdir()
            flac, mpv = runtime / proof.FLAC, runtime / proof.MPV
            other = runtime.parent / proof.FLAC
            for path in (flac, mpv, other):
                path.write_bytes(b'identical contents, distinct file identities')
            receipt = {'libflacVersion': '1.5.0', 'libmpvApi': 0x20005, 'libmpvVersion': 'mpv 0.41.0',
                       'relativeLookupRejected': True, 'libflacModule': str(flac), 'libmpvModule': str(mpv)}
            proof.check_identity(receipt, runtime)
            receipt['libflacModule'] = str(other)
            with self.assertRaisesRegex(ValueError, 'does not identify'):
                proof.check_identity(receipt, runtime)


if __name__ == '__main__':
    unittest.main()
