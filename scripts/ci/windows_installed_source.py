"""Read-only provenance for the optional Windows installed-package diagnostic."""
import argparse
import os
from pathlib import Path
import re

from publish import REQUIRED_JOBS, api, validate_manifest, validate_run
from release_guard import check_versions, git

REPOSITORY = 'RC-CHN/Cumulus-music'
WORKFLOW = '.github/workflows/ci.yml'
ARTIFACT = 'packages-windows-x64'
PLATFORM = 'windows-x64'
NUMBER = re.compile(r'[1-9][0-9]{0,18}')
COMMIT = re.compile(r'[0-9a-f]{40}')


def select_source(read, run_id, attempt, commit):
    assert NUMBER.fullmatch(run_id) and NUMBER.fullmatch(attempt), 'invalid source CI run/attempt'
    assert COMMIT.fullmatch(commit), 'expected full source commit SHA'
    run_id, attempt = int(run_id), int(attempt)
    prefix = f'repos/{REPOSITORY}/actions'
    workflow = read(f'{prefix}/workflows/ci.yml')
    assert workflow['path'] == WORKFLOW and workflow['state'] == 'active', 'wrong source workflow'
    latest = read(f'{prefix}/runs/{run_id}')
    source = read(f'{prefix}/runs/{run_id}/attempts/{attempt}')
    for run in (latest, source):
        validate_run(run, REPOSITORY, commit, run_id, workflow['id'])
        # Artifacts are listed by run, not attempt. A later retry invalidates
        # selection even when GitHub still returns the earlier attempt metadata.
        assert run['run_attempt'] == attempt, 'source CI run was rerun'
    repo_id = source['repository']['id']
    assert repo_id == source['head_repository']['id'], 'source repository ID mismatch'

    job_list = read(f'{prefix}/runs/{run_id}/attempts/{attempt}/jobs?per_page=100')
    jobs = job_list['jobs']
    assert job_list['total_count'] == len(jobs), 'incomplete source CI jobs'
    for name in REQUIRED_JOBS:
        matches = [job for job in jobs if job['name'] == name]
        assert len(matches) == 1, f'missing or duplicate source CI job: {name}'
        job, = matches
        assert (job['run_id'], job['run_attempt'], job['head_sha'], job['status'], job['conclusion']) == (
            run_id, attempt, commit, 'completed', 'success'), f'source CI job mismatch: {name}'

    artifact_list = read(f'{prefix}/runs/{run_id}/artifacts?per_page=100')
    artifacts = artifact_list['artifacts']
    assert artifact_list['total_count'] == len(artifacts), 'incomplete source artifact list'
    matches = [row for row in artifacts if row['name'] == ARTIFACT]
    assert len(matches) == 1 and matches[0]['expired'] is False, 'missing, duplicate or expired Windows package artifact'
    selected, = matches
    origin = selected['workflow_run']
    assert (origin['id'], origin['repository_id'], origin['head_repository_id'],
            origin['head_branch'], origin['head_sha']) == (
        run_id, repo_id, repo_id, 'main', commit), 'Windows package artifact source mismatch'
    artifact_id = str(selected['id'])
    assert NUMBER.fullmatch(artifact_id), 'invalid Windows package artifact ID'
    final = read(f'{prefix}/runs/{run_id}')
    validate_run(final, REPOSITORY, commit, run_id, workflow['id'])
    assert final['run_attempt'] == attempt, 'source CI run was rerun'
    return artifact_id


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--run-id', required=True)
    parser.add_argument('--attempt', required=True)
    parser.add_argument('--commit', required=True)
    sub = parser.add_subparsers(dest='action', required=True)
    sub.add_parser('select')
    confirm = sub.add_parser('confirm')
    confirm.add_argument('--artifact-id', required=True)
    confirm.add_argument('--source', required=True, type=Path)
    confirm.add_argument('--packages', required=True, type=Path)
    args = parser.parse_args()
    if not __debug__:
        raise RuntimeError('diagnostic provenance assertions must be enabled')
    assert os.environ['GITHUB_REPOSITORY'] == REPOSITORY, 'wrong diagnostic repository'
    assert os.environ['GITHUB_REF'] == 'refs/heads/main', 'diagnostic must run from main'
    assert git('-C', str(Path(__file__).resolve().parents[2]), 'rev-parse', 'HEAD') == os.environ[
        'GITHUB_SHA'], 'driver checkout differs from diagnostic workflow'
    artifact = select_source(lambda path: api(path, '--hostname', 'github.com'),
                             args.run_id, args.attempt, args.commit)
    if args.action == 'select':
        with Path(os.environ['GITHUB_OUTPUT']).open('a', encoding='utf-8') as output:
            # Every emitted value has a strict decimal or hex grammar.
            output.write(f'commit={args.commit}\nrun={args.run_id}\nattempt={args.attempt}\nartifact={artifact}\n')
    else:
        assert artifact == args.artifact_id, 'Windows package artifact changed during download'
        assert git('-C', str(args.source), 'rev-parse', 'HEAD') == args.commit, 'source checkout differs from CI'
        version = check_versions(args.source)
        validate_manifest(args.packages, version, args.commit, PLATFORM, args.run_id, args.attempt)
        print(f'Verified Windows package artifact {artifact} for exact source {args.commit}.')


if __name__ == '__main__':
    main()
