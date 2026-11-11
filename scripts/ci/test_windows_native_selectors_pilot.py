import copy
from pathlib import Path
import unittest

import windows_native_selectors_pilot as pilot


def fixture_plan():
    repo = Path.cwd().resolve()
    packages = []
    for package, folder in pilot.PACKAGES.items():
        def target(kind, name, **kwargs):
            return dict(kind=[kind], name=name, **kwargs)

        targets = [target('lib', package)]
        for shard in pilot.SHARDS:
            # Ensure selectors from every package remain in every test shard.
            name = next(f'suite_{shard}_{i}' for i in range(100)
                        if pilot.owner((folder, f'deps/suite_{shard}_{i}')) == shard)
            targets.append(target('test', name))
        if folder in ('core', 'desktop'):
            targets.append(target('test', 'watchman_collection'))
        if folder == 'desktop':
            targets.extend([target('example', 'feature-gated', test=False,
                                   **{'required-features': ['custom-protocol']}),
                            target('bin', 'desktop-app')])
        packages.append(dict(id=package, name=package, manifest_path=repo/folder/'Cargo.toml', targets=targets))
    meta = dict(packages=packages, workspace_members=list(pilot.PACKAGES))
    return pilot.plan_targets(meta, repo)


class NativeSelectorsPilotTest(unittest.TestCase):
    def setUp(self):
        self.ids, self.targets, self.assigned = fixture_plan()
        self.identity = dict(commit='a' * 40, ciRun='123', ciAttempt='1')

    def receipts(self, mode, identity=None):
        receipts = []
        for shard in pilot.SHARDS:
            keys = [key for key, part in self.assigned.items() if mode == 'reference' or part == shard]
            rows = [dict(target=list(key), args=[], status=0 if self.assigned[key] == shard else None,
                         elapsedMs=2 if self.assigned[key] == shard else None) for key in keys]
            artifacts = [dict(target=list(key), features=['custom-protocol'] if key[0] == 'desktop' else [],
                              profile=dict(test=True, opt_level='0')) for key in keys]
            receipts.append(dict(schema=pilot.SCHEMA, **(identity or self.identity), mode=mode, shard=shard,
                                 frontend='frontend-bytes', toolchain='pinned', media=dict(key='media-key'),
                                 dependencyCacheKey='rust-key', inventory=rows, artifacts=artifacts,
                                 compileMs=14, totalMs=20))
        return receipts

    def test_workspace_selector_groups_and_feature_gated_examples(self):
        libs = {key for key, selector in self.targets.items() if selector == ('--lib',)}
        self.assertEqual(len(libs), 3)
        self.assertEqual({self.assigned[key] for key in libs}, {'a'})
        duplicates = [('core', 'deps/watchman_collection'), ('desktop', 'deps/watchman_collection')]
        self.assertEqual(len({self.assigned[key] for key in duplicates}), 1)
        group = pilot.cargo_command(self.targets, self.assigned, 'selective', self.assigned[duplicates[0]])
        self.assertEqual(group.count('watchman_collection'), 1)
        for key, flag in ((('desktop', 'examples/feature-gated'), '--example'),
                          (('desktop', 'deps/desktop-app'), '--bin')):
            cmd = pilot.cargo_command(self.targets, self.assigned, 'selective', self.assigned[key])
            self.assertEqual(cmd[cmd.index(key[1].split('/')[1]) - 1], flag)
            self.assertNotIn('--all-targets', cmd)
        self.assertIn('--all-targets', pilot.cargo_command(self.targets, self.assigned, 'reference', 'a'))

    def test_compiler_artifact_accepts_test_harness_for_non_test_example(self):
        row = dict(package_id='cumulus-desktop', executable='feature-gated.exe',
                   target=dict(kind=['example'], name='feature-gated', test=False),
                   profile=dict(test=True, opt_level='0'), features=['custom-protocol'])
        key, facts = pilot.artifact_facts(row, self.ids, self.targets)
        self.assertEqual(key, ('desktop', 'examples/feature-gated'))
        self.assertEqual(facts['features'], ['custom-protocol'])
        row['profile']['test'] = False
        self.assertIsNone(pilot.artifact_facts(row, self.ids, self.targets))

    def test_both_modes_execute_same_complete_target_set(self):
        identity2 = {**self.identity, 'ciRun': '124'}
        reference = pilot.aggregate(self.receipts('reference'), self.identity, 'reference', self.assigned)
        selective = pilot.aggregate(self.receipts('selective', identity2), identity2, 'selective', self.assigned)
        self.assertEqual(reference['inventory'], selective['inventory'])
        self.assertEqual(reference['artifacts'], selective['artifacts'])
        self.assertEqual(pilot.compare(reference, selective),
                         dict(targets=len(self.targets), reference=dict(compileMs=14, testMs=6, totalMs=20),
                              selective=dict(compileMs=14, testMs=6, totalMs=20), sameDependencyCacheKeys=True))

    def test_receipt_must_prove_exact_execution_and_compile_inventory(self):
        def removed(receipt):
            receipt['inventory'].pop()

        def repeated(receipt):
            receipt['inventory'].append(receipt['inventory'][0])

        def missing_artifact(receipt):
            receipt['artifacts'].pop()

        cases = [(removed, 'runner inventory'), (repeated, 'runner inventory'),
                 (missing_artifact, 'compiled harness inventory'),
                 (lambda r: r['inventory'][0].update(args=['--skip', 't']), 'extra test arguments'),
                 (lambda r: r['inventory'][0].update(status=1), 'did not execute'),
                 (lambda r: r.update(ciAttempt='2'), 'wrong pilot receipt')]
        for mutate, reason in cases:
            with self.subTest(reason=reason):
                receipts = self.receipts('selective')
                mutate(receipts[0])
                with self.assertRaisesRegex(ValueError, reason):
                    pilot.aggregate(receipts, self.identity, 'selective', self.assigned)

    def test_comparison_rejects_configuration_changes_and_incomplete_execution(self):
        identity2 = {**self.identity, 'ciRun': '124'}
        reference = pilot.aggregate(self.receipts('reference'), self.identity, 'reference', self.assigned)
        selective = pilot.aggregate(self.receipts('selective', identity2), identity2, 'selective', self.assigned)
        changes = [(lambda r: r.update(ciRun=self.identity['ciRun']), 'separate reference'),
                   (lambda r: r.update(frontend='other-bytes'), 'differ in frontend'),
                   (lambda r: r['artifacts'][0]['profile'].update(opt_level='2'), 'differ in artifacts'),
                   (lambda r: r['inventory'].pop(), 'incomplete comparative target coverage')]
        for mutate, reason in changes:
            with self.subTest(reason=reason):
                altered = copy.deepcopy(selective)
                mutate(altered)
                with self.assertRaisesRegex(ValueError, reason):
                    pilot.compare(reference, altered)


if __name__ == '__main__':
    unittest.main()
