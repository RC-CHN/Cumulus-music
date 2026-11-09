"""Pin the earlier diagnostic-only null package without treating it as a release."""
import json
import subprocess


REPOSITORY = 'RC-CHN/Cumulus-music'
REPO_ID = 1432308824
RUN_ID = 42667945952
JOB_ID = 129337379592
ARTIFACT_ID = 13699442499
COMMIT = 'ad461a94306ef0a36c5cef835dfb76241b87c97e'
BRANCH = 'validation/phase199-native-open'
BINARY_SHA256 = '7893c0f62c762bfb9e2f7b54d4a609d4f5c4133cbcbe6757353c0b76c6b1427c'


def api_get(resource):
    return json.loads(subprocess.check_output(
        ['gh', 'api', f'repos/{REPOSITORY}/actions/{resource}'],
        encoding='utf-8', timeout=30,
    ))


def require_fields(value, expected):
    assert all(value.get(key) == item for key, item in expected.items()), (
        'retained diagnostic producer identity or status mismatch'
    )


def verified_producer(get=api_get):
    run = get(f'runs/{RUN_ID}')
    require_fields(run, dict(id=RUN_ID, run_attempt=1, workflow_id=398042187,
                             path='.github/workflows/ci.yml', event='workflow_dispatch',
                             head_branch=BRANCH, head_sha=COMMIT, status='completed',
                             conclusion='failure'))
    assert run['repository']['id'] == run['head_repository']['id'] == REPO_ID
    package = get(f'jobs/{JOB_ID}')
    require_fields(package, dict(id=JOB_ID, run_id=RUN_ID, run_attempt=1, head_sha=COMMIT,
                                 name='Package and verify (windows-x64)', status='completed',
                                 conclusion='success'))
    artifact = get(f'artifacts/{ARTIFACT_ID}')
    require_fields(artifact, dict(
        id=ARTIFACT_ID, name='diagnostic-packages-windows-x64', expired=False,
        size_in_bytes=209791309,
        digest='sha256:de2021afa2fb13877b3b207806c1a622e234b5f9bee0c81305fb2fb94c37ceb1',
    ))
    require_fields(artifact['workflow_run'], dict(id=RUN_ID, repository_id=REPO_ID,
                                                 head_repository_id=REPO_ID,
                                                 head_branch=BRANCH, head_sha=COMMIT))
    return dict(commit=COMMIT, ciRun=str(RUN_ID), ciAttempt='1')
