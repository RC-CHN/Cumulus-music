"""Archive only the allowlisted synthetic CI proofs, preserving unusual paths.

upload-artifact rejects colons even on Linux; our filter path-escaping fixtures
intentionally contain them. A tar preserves these names without renaming proof
inputs or losing the rest of a failed job's evidence.
"""
from pathlib import Path
import tarfile


PATTERNS = (
    'package-stage/*/equalizer-proof',
    'ci-login*', 'ci-native-audio*', 'ci-engine-roundtrip*.log', 'ci-equalizer-roundtrip*.log',
    'windows-media/build.log', 'windows-media/plan.json', 'windows-media/installed.json',
    'windows-media/player-probe.log', 'windows-media/player-probe.stdout.log', 'windows-media/player-probe.json', 'windows-media/equalizer-proof',
    'ci-scroll*', 'ci-graphics*', 'ci-ux*', 'ci-motion*', 'ci-accounts*',
    'ci-startup*', 'ci-output-settings*', 'ci-tray*', 'ci-mpris*', 'ci-catalog*', 'ci-likes*', 'ci-keyboard*',
    'ci-collections*', 'ci-history*', 'ci-settings*', 'ci-relay*',
    'ci-download-*', 'ci-playlist-*', 'ci-song-menu*', 'ci-search*',
    'ci-playback-speed*', 'ci-playback-quality*', 'ci-quality-trial-native*', 'ci-quality-rights-ui*', 'ci-volume-ui*', 'ci-mode-ui*',
    'ci-empty-fm-ui*', 'ci-shortcuts*', 'ci-system-media*', 'ci-mini*', 'ci-desktop-lyrics*', 'ci-fonts*',
)


def archive(build: Path, destination: Path):
    paths = sorted({path for pattern in PATTERNS for path in build.glob(pattern)})
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tarfile.open(destination, 'w:gz', dereference=False) as output:
        for path in paths:
            output.add(path, arcname=path.relative_to(build).as_posix())
    return len(paths)


if __name__ == '__main__':
    count = archive(Path('.build'), Path('.build/diagnostics/synthetic-proofs.tar.gz'))
    print(f'Archived {count} synthetic CI proof roots (no private research paths).')
