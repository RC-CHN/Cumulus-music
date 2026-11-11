from copy import deepcopy
import json
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

from windows_native_plan import (
    FEATURE, WORKSPACE_PACKAGES, all_targets, cargo_argv, compare_legacy, derive_plan,
    load_plan, require_checkout, verify_selected,
)
from windows_native_shard import SHARDS, owner


def target(name, kind, tested=True, required=None):
    row = dict(name=name, kind=kind, test=tested)
    if required is not None:
        row['required-features'] = required
    return row


def metadata(workspace):
    packages = [
        dict(id=name, name=name, manifest_path=str(workspace / path / 'Cargo.toml'),
             features={'custom-protocol': ['tauri/custom-protocol']} if path == 'desktop' else {},
             targets=[target(name.replace('-', '_'), ['cdylib', 'rlib'] if path.endswith('audio-filter') else ['lib'])])
        for path, name in WORKSPACE_PACKAGES.items()
    ]
    packages[0]['targets'].extend((
        target('build-script-build', ['custom-build'], False),
        target('client_player', ['test']), target('history', ['test']),
        target('watchman_collection', ['test']),
    ))
    packages[2]['targets'].extend((
        target('cumulus', ['bin']), target('watchman_collection', ['test']),
        target('ux_ui_probe', ['example'], False, ['custom-protocol']),
    ))
    for index, (folder, package) in enumerate(zip(WORKSPACE_PACKAGES, packages, strict=True)):
        for shard in SHARDS:
            name = next(f'package_{index}_{shard}_{n}' for n in range(200)
                        if owner((folder, f'deps/package_{index}_{shard}_{n}')) == shard)
            package['targets'].append(target(name, ['test']))
    return dict(workspace_root=str(workspace), workspace_members=[p['id'] for p in packages],
                packages=packages)


class WindowsNativePlanTest(unittest.TestCase):
    current_run = ['synthetic-sha', '421', '3']
    required_key = ('desktop', 'examples/ux_ui_probe')

    def setUp(self):
        root = TemporaryDirectory()
        self.addCleanup(root.cleanup)
        self.root = Path(root.name)
        self.metadata = metadata(self.root)
        self.plan = derive_plan(self.metadata, self.root)

    def selected_rows(self, reference=None):
        return {
            shard: [[list(key), shard, 0 if key in item.targets else None, self.current_run.copy()]
                    for key in sorted(all_targets(self.plan) if reference == shard else item.targets)]
            for shard, item in self.plan.items()
        }

    def receipts(self, rows):
        files = [self.root / f'{shard}.jsonl' for shard in SHARDS]
        for shard, path in zip(SHARDS, files, strict=True):
            path.write_text(''.join(json.dumps(line) + '\n' for line in rows[shard]), encoding='utf-8')
        return files

    def test_literal_selectors_cover_required_examples_libraries_and_package_collision(self):
        a, b = (self.plan[shard] for shard in ('a', 'b'))
        self.assertEqual(a.selectors.count('--lib'), 1)
        self.assertTrue({('core', 'deps/cumulus_core'), ('core/audio-filter', 'deps/cumulus_audio_filter'),
                         ('desktop', 'deps/cumulus_desktop')} <= set(a.targets))
        self.assertIn(self.required_key, b.targets)
        collection = {('core', 'deps/watchman_collection'), ('desktop', 'deps/watchman_collection')}
        owners = [item for item in self.plan.values() if collection <= set(item.targets)]
        self.assertEqual(len(owners), 1)
        self.assertEqual(owners[0].selectors.count('watchman_collection'), 1)
        argv = cargo_argv(('cargo', '+1.96.0'), b)
        self.assertEqual(argv[:8], ('cargo', '+1.96.0', 'test', '--no-fail-fast', '--workspace',
                                    '--features', FEATURE, '--locked'))
        self.assertEqual(argv[argv.index('ux_ui_probe') - 1], '--example')
        self.assertNotIn('--all-targets', argv)
        self.assertNotIn(('core', 'deps/build-script-build'), all_targets(self.plan))
        self.assertEqual(len(all_targets(self.plan)), 18)

    def test_reads_only_pinned_offline_workspace_metadata(self):
        (self.root / 'rust-toolchain.toml').write_text('[toolchain]\nchannel = "1.96.0"\n', encoding='utf-8')
        with patch('windows_native_plan.subprocess.check_output', return_value=json.dumps(self.metadata)) as run:
            cargo, result = load_plan(self.root)
        self.assertEqual(result, self.plan)
        self.assertEqual(cargo, ('cargo', '+1.96.0'))
        run.assert_called_once_with([*cargo, 'metadata', '--offline', '--locked', '--no-deps',
                                     '--format-version', '1', '--features', FEATURE],
                                    cwd=self.root.resolve(), encoding='utf-8')

    def test_planner_rejects_new_or_missing_feature_semantics_and_unknown_targets(self):
        def unsupported_requirements(packages):
            next(row for row in packages[2]['targets'] if row['name'] == 'ux_ui_probe')[
                'required-features'] = ['signed-window-read-proof']

        def disabled_integration(packages):
            packages[0]['targets'][-1]['test'] = False

        for edit, message in (
            (unsupported_requirements, 'required features'),
            (lambda packages: packages[2]['features'].clear(), 'feature defaults'),
            (lambda packages: packages[0]['features'].update(default=['unknown']), 'feature defaults'),
            (lambda packages: packages[0]['targets'].append(target('speed', ['bench'])), 'target kind'),
            (disabled_integration, 'disabled integration test'),
            (lambda packages: packages[2]['targets'].append(target('cumulus', ['test'])), 'duplicate'),
        ):
            with self.subTest(edit=message), self.assertRaisesRegex(ValueError, message):
                changed = deepcopy(self.metadata)
                edit(changed['packages'])
                derive_plan(changed, self.root)

    def test_exact_selected_coverage_and_full_reference_migration_pass(self):
        for reference in (None, 'a'):
            with self.subTest(reference=reference):
                files = self.receipts(self.selected_rows(reference))
                counts = verify_selected(self.plan, files, self.current_run, reference)
                self.assertEqual(sum(counts.values()), 18)

    def test_required_verification_rejects_a_different_checked_out_commit(self):
        with patch('windows_native_plan.subprocess.check_output', return_value='different-sha\n'):
            with self.assertRaisesRegex(ValueError, 'checkout does not match'):
                require_checkout(self.root, self.current_run)

    def test_receipt_mutations_cannot_satisfy_the_exact_gate(self):
        def find_example(rows):
            return next(row for row in rows['b'] if tuple(row[0]) == self.required_key)

        for label, edit in (
            ('drop required example', lambda rows: rows['b'].remove(find_example(rows))),
            ('skip assigned example', lambda rows: find_example(rows).__setitem__(2, None)),
            ('wrong shard', lambda rows: rows['c'].append(rows['b'].pop())),
            ('extra example', lambda rows: rows['b'].append([['desktop', 'examples/extra'], 'b', 0, self.current_run])),
            ('duplicate', lambda rows: rows['a'].append(rows['a'][0])),
            ('failed example', lambda rows: find_example(rows).__setitem__(2, 1)),
            ('boolean zero', lambda rows: find_example(rows).__setitem__(2, False)),
            ('previous attempt', lambda rows: find_example(rows)[3].__setitem__(2, '2')),
            ('other commit', lambda rows: find_example(rows)[3].__setitem__(0, 'other-sha')),
        ):
            with self.subTest(label=label), self.assertRaises(ValueError):
                rows = self.selected_rows()
                edit(rows)
                verify_selected(self.plan, self.receipts(rows), self.current_run)

    def test_legacy_reference_must_contain_even_unowned_examples(self):
        rows = self.selected_rows(reference='a')
        reference_example = next(row for row in rows['a'] if tuple(row[0]) == self.required_key)
        self.assertIsNone(reference_example[2])
        rows['a'].remove(reference_example)
        with self.assertRaisesRegex(ValueError, 'inventory differs on a'):
            verify_selected(self.plan, self.receipts(rows), self.current_run, reference_shard='a')

    def test_legacy_comparison_uses_exact_key_set_independent_of_historical_owners(self):
        rows = self.selected_rows()
        rows['b'] = [[list(key), 'b', 0 if owner(key) == 'a' else None, self.current_run]
                     for key in sorted(all_targets(self.plan))]
        path = self.receipts(rows)[1]
        self.assertEqual(compare_legacy(self.plan, path, 'b', self.current_run), 18)
        rows['b'].pop()
        with self.assertRaisesRegex(ValueError, 'inventory differs on b'):
            compare_legacy(self.plan, self.receipts(rows)[1], 'b', self.current_run)
        rows['b'] = [[list(key), 'b', None, self.current_run] for key in sorted(all_targets(self.plan))]
        with self.assertRaisesRegex(ValueError, 'did not execute any targets'):
            compare_legacy(self.plan, self.receipts(rows)[1], 'b', self.current_run)


if __name__ == '__main__':
    unittest.main()
