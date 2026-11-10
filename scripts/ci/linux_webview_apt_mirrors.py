"""Bound package acquisition on the known GitHub Ubuntu 24.04 mirror preset."""

from contextlib import contextmanager
from pathlib import Path
import stat
import subprocess


MIRRORS = Path('/etc/apt/apt-mirrors.txt')
SOURCES = Path('/etc/apt/sources.list.d/ubuntu.sources')
UPDATE_DEADLINE = 75
DOWNLOAD_DEADLINE = 150
KILL_GRACE = 5
AZURE = b'http://azure.archive.ubuntu.com/ubuntu/'
CANONICAL = b'https://archive.ubuntu.com/ubuntu/'
SECURITY = b'https://security.ubuntu.com/ubuntu/'
AZURE_FIRST = AZURE + b'\tpriority:1\n' + CANONICAL + b'\tpriority:2\n' + SECURITY + b'\tpriority:3\n'
CANONICAL_FIRST = AZURE + b'\tpriority:2\n' + CANONICAL + b'\tpriority:1\n' + SECURITY + b'\tpriority:3\n'


class AptDeadline(RuntimeError):
    def __init__(self, stage, seconds):
        super().__init__(f'APT {stage} exceeded {seconds}s without touching dpkg')


def is_signed_noble_source(source):
    mirror_uri = f'mirror+file:{MIRRORS}'
    suites = set()
    for stanza in source.split('\n\n'):
        fields = {name.lower(): value.strip() for name, value in (
            line.split(':', 1) for line in stanza.splitlines()
            if line and not line.startswith(('#', ' ', '\t')) and ':' in line)}
        if fields.get('enabled', '').lower() == 'no' or mirror_uri not in fields.get('uris', '').split():
            continue
        if (fields['uris'].split() != [mirror_uri]
                or fields.get('signed-by') != '/usr/share/keyrings/ubuntu-archive-keyring.gpg'
                or fields.keys() & {'trusted', 'allow-insecure', 'allow-downgrade-to-insecure'}):
            return False
        suites.update(fields.get('suites', '').split())
    return {'noble', 'noble-security'} <= suites <= {'noble', 'noble-updates', 'noble-backports', 'noble-security'}


def supports_fallback(env):
    if env.get('GITHUB_ACTIONS') != 'true' or env.get('RUNNER_OS') != 'Linux':
        return False
    try:
        info = MIRRORS.lstat()
        if not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_size != len(AZURE_FIRST):
            return False
        return MIRRORS.read_bytes() == AZURE_FIRST and is_signed_noble_source(SOURCES.read_text())
    except (OSError, UnicodeError):
        return False


@contextmanager
def canonical_priority(workspace):
    """Switch only the two existing priorities on this dedicated runner."""
    if MIRRORS.read_bytes() != AZURE_FIRST:
        raise ValueError('runner APT mirror preset changed during acquisition')
    backup = workspace / 'apt-mirrors.original'
    alternative = workspace / 'apt-mirrors.canonical-first'
    alternative.write_bytes(CANONICAL_FIRST)
    subprocess.run(['sudo', 'cp', '-p', '--', MIRRORS, backup], check=True)
    try:
        subprocess.run(['sudo', 'cp', '--', alternative, MIRRORS], check=True)
        yield
    finally:
        subprocess.run(['sudo', 'cp', '-p', '--', backup, MIRRORS], check=True)


def deadline_command(args, limit):
    # GNU timeout runs *inside* sudo so that its process group contains and
    # can terminate APT's root children. Killing an unprivileged sudo parent
    # from Python could leave an old acquisition holding locks in the runner.
    if (list(args[:2]) != ['sudo', 'apt-get']
            or args[-1] != 'update' and '--download-only' not in args):
        raise ValueError('APT deadline only applies to update or download-only')
    return ['sudo', 'timeout', '--signal=TERM', f'--kill-after={KILL_GRACE}s', f'{limit}s', *args[1:]]


@contextmanager
def acquire(update, prepare, download, reset, fallback):
    """The only retry occurs before installation, on a real deadline."""
    if fallback is None:
        update(None)
        prepare()
        yield False
        return
    try:
        update(UPDATE_DEADLINE)
        prepare()
        download(DOWNLOAD_DEADLINE)
    except AptDeadline as error:
        print(f'{error}; retry with the existing Canonical mirror first', flush=True)
        # The next signed index can select different archives. Discard both
        # partial and finalized first-plan bytes and independently admit any
        # candidate artifact again against the new plan.
        reset()
        with fallback():
            update(UPDATE_DEADLINE)
            prepare()
            download(DOWNLOAD_DEADLINE)
            yield True
    else:
        yield True
