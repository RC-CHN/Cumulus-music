"""Pure fixtures for the Windows package diagnostic's GitHub source gate."""
import copy
import unittest

from windows_installed_source import ARTIFACT, REPOSITORY, REQUIRED_JOBS, WORKFLOW, select_source

COMMIT = 'a' * 40
PREFIX = f'repos/{REPOSITORY}/actions'
RUN = f'{PREFIX}/runs/17'
ATTEMPT = f'{RUN}/attempts/2'
JOBS = f'{ATTEMPT}/jobs?per_page=100'
ARTIFACTS = f'{RUN}/artifacts?per_page=100'


class WindowsInstalledSource(unittest.TestCase):
    def setUp(self):
        repo = {'id': 31, 'full_name': REPOSITORY}
        run = dict(id=17, run_attempt=2, repository=repo, head_repository=repo,
                   path=WORKFLOW, workflow_id=29, event='push', head_branch='main',
                   head_sha=COMMIT, status='completed', conclusion='success')
        jobs = [
            dict(name=name, run_id=17, run_attempt=2, head_sha=COMMIT,
                 status='completed', conclusion='success') for name in sorted(REQUIRED_JOBS)
        ]
        artifact = dict(id=58, name=ARTIFACT, expired=False,
                        workflow_run=dict(id=17, repository_id=31, head_repository_id=31,
                                          head_branch='main', head_sha=COMMIT))
        self.response = {f'{PREFIX}/workflows/ci.yml': dict(id=29, path=WORKFLOW, state='active'),
                         RUN: copy.deepcopy(run), ATTEMPT: copy.deepcopy(run),
                         JOBS: dict(total_count=len(jobs), jobs=jobs),
                         ARTIFACTS: dict(total_count=1, artifacts=[artifact])}

    def select(self):
        return select_source(self.response.__getitem__, '17', '2', COMMIT)

    def test_picks_the_unique_exact_artifact_id(self):
        self.assertEqual(self.select(), '58')

    def test_rejects_old_attempt_failed_job_or_mismatched_artifact_source(self):
        for path, fields, message in (
            (RUN, {'run_attempt': 3}, 'rerun'),
            (ATTEMPT, {'head_sha': 'b' * 40}, 'exact commit'),
            (ATTEMPT, {'head_repository': {'id': 89, 'full_name': 'fork/other'}}, 'repository'),
            (ATTEMPT, {'workflow_id': 30}, 'wrong workflow'),
        ):
            with self.subTest(path=path, fields=fields):
                with self.assertRaisesRegex(AssertionError, message):
                    select_source((self.response | {path: self.response[path] | fields}).__getitem__,
                                  '17', '2', COMMIT)
        windows = next(job for job in self.response[JOBS]['jobs'] if job['name'] == 'Native (windows-x64)')
        windows['conclusion'] = 'failure'
        with self.assertRaisesRegex(AssertionError, 'source CI job mismatch'):
            self.select()
        windows['conclusion'] = 'success'
        self.response[ARTIFACTS]['artifacts'][0]['workflow_run']['head_repository_id'] = 92
        with self.assertRaisesRegex(AssertionError, 'artifact source mismatch'):
            self.select()

    def test_never_accepts_duplicate_artifact_or_rerun_after_selection(self):
        duplicate = self.response[ARTIFACTS]['artifacts'][0].copy() | {'id': 59}
        self.response[ARTIFACTS]['artifacts'].append(duplicate)
        self.response[ARTIFACTS]['total_count'] = 2
        with self.assertRaisesRegex(AssertionError, 'duplicate'):
            self.select()
        self.response[ARTIFACTS]['artifacts'].pop()
        self.response[ARTIFACTS]['total_count'] = 1
        previous = self.response[RUN]
        observed = iter((previous, previous | {'run_attempt': 3}))
        with self.assertRaisesRegex(AssertionError, 'rerun'):
            select_source(lambda path: next(observed) if path == RUN else self.response[path],
                          '17', '2', COMMIT)


if __name__ == '__main__':
    unittest.main()
