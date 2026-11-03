"""Archive only the allowlisted synthetic CI proofs, preserving unusual paths.

upload-artifact rejects colons even on Linux; our filter path-escaping fixtures
intentionally contain them. A tar preserves these names without renaming proof
inputs or losing the rest of a failed job's evidence.
"""
from pathlib import Path, PurePosixPath
import shlex
import tarfile


REPOSITORY = Path(__file__).resolve().parents[2]

# Only these non-UI outputs accompany the workflow's explicit proof roots.
# Platform and report names are closed sets; a neighboring research directory
# must not enter the archive merely because its name starts with "ci-".
FIXED_PATHS = (
    'package-stage/linux-x64/equalizer-proof',
    'package-stage/windows-x64/equalizer-proof',
    # Linux's run_auth_probe takes its proof root positionally, without --root.
    'ci-login', 'ci-login.log',
    'windows-media/build.log', 'windows-media/plan.json', 'windows-media/installed.json',
    'windows-media/player-probe.log', 'windows-media/player-probe.stdout.log', 'windows-media/player-probe.json', 'windows-media/equalizer-proof',
)
NATIVE_REPORTS = (
    'ci-native-audio-linux', 'ci-native-audio-windows',
    'ci-native-audio-loudness-linux', 'ci-native-audio-loudness-windows',
)


def workflow_roots():
    """Read literal --root arguments; never execute a workflow or expand globs.

    A proof root must be a single named child of .build. This intentionally does
    not accept variable substitutions, parent paths or arbitrary repository data.
    """
    roots = set()
    for workflow in sorted((REPOSITORY / '.github/workflows').glob('*.yml')):
        for line in workflow.read_text(encoding='utf-8').splitlines():
            if '--root' not in line:
                continue
            words = shlex.split(line, comments=True)
            for index, word in enumerate(words[:-1]):
                if word != '--root':
                    continue
                path = PurePosixPath(words[index + 1])
                if (len(path.parts) == 2 and path.parts[0] == '.build'
                        and path.name and all(char.isascii() and (char.isalnum() or char in '-_')
                                              for char in path.name)):
                    roots.add(path.name)
    return roots


def proof_paths(build):
    names = set(FIXED_PATHS)
    for root in workflow_roots():
        names.update((root, root + '.log'))
    for report in NATIVE_REPORTS:
        names.add(report + '-profile')
        names.update(report + suffix for suffix in
                     ('.json', '.log', '.failure.json', '.progress.jsonl', '.pcm'))
    for platform in ('linux-x64', 'windows-x64'):
        names.update(f'ci-{probe}-roundtrip-{platform}.log' for probe in ('engine', 'equalizer'))
    return sorted(build / name for name in names if (build / name).exists())


def archive(build: Path, destination: Path):
    paths = proof_paths(build)
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tarfile.open(destination, 'w:gz', dereference=False) as output:
        for path in paths:
            output.add(path, arcname=path.relative_to(build).as_posix())
    return len(paths)


if __name__ == '__main__':
    count = archive(Path('.build'), Path('.build/diagnostics/synthetic-proofs.tar.gz'))
    print(f'Archived {count} synthetic CI proof roots (no private research paths).')
