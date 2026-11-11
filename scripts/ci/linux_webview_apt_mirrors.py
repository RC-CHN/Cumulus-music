"""Bound signed Ubuntu 24.04 archive acquisition without changing host sources."""

from concurrent.futures import ThreadPoolExecutor
from contextlib import contextmanager
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from email.utils import parsedate_to_datetime
from pathlib import Path
import re
import stat
import subprocess
import time
from urllib.parse import urlsplit


MIRRORS = Path('/etc/apt/apt-mirrors.txt')
SOURCES = Path('/etc/apt/sources.list.d/ubuntu.sources')
KEYRING = '/usr/share/keyrings/ubuntu-archive-keyring.gpg'
UPDATE_DEADLINE = 75
DOWNLOAD_DEADLINE = 150
# On run 43046670469, each signed direct route completed its index update,
# but the 208–223 MB cold plans needed up to about seven minutes at the
# observed archive rate. Keep a hard bound without cutting a moving fetch.
DIRECT_DOWNLOAD_DEADLINE = 600
KILL_GRACE = 5
PROBE_TIMEOUT = 6
MAX_RELEASE = 512 * 1024
MAX_POCKET_AGE = timedelta(days=7)
SUITES = ('noble', 'noble-updates', 'noble-backports', 'noble-security')
MUTABLE = ('noble-security', 'noble-updates')
AZURE_FIRST = (
    b'http://azure.archive.ubuntu.com/ubuntu/\tpriority:1\n'
    b'https://archive.ubuntu.com/ubuntu/\tpriority:2\n'
    b'https://security.ubuntu.com/ubuntu/\tpriority:3\n'
)
DIRECT_MIRRORS = (
    ('Azure HTTPS', 'https://azure.archive.ubuntu.com/ubuntu/'),
    ('Canonical', 'https://archive.ubuntu.com/ubuntu/'),
    ('Security', 'https://security.ubuntu.com/ubuntu/'),
    ('MIT', 'https://mirrors.mit.edu/ubuntu/'),
)


class AptDeadline(RuntimeError):
    def __init__(self, stage, seconds):
        super().__init__(f'APT {stage} exceeded {seconds}s without touching dpkg')


@dataclass(frozen=True)
class Mirror:
    name: str
    uri: str
    dates: dict
    seconds: float


def signed_noble_suites(source):
    """Recognize the runner's Ubuntu source before replacing its URI lines."""
    uri = f'mirror+file:{MIRRORS}'
    suites = set()
    for stanza in re.split(r'\n[ \t]*\n', source):
        if not any(line.startswith('URIs:') and uri in line.split()[1:] for line in stanza.splitlines()):
            continue
        fields = {}
        for line in stanza.splitlines():
            if not line or line.startswith('#'):
                continue
            name, separator, value = line.partition(':')
            name = name.lower()
            if not separator or not re.fullmatch(r'[a-z][a-z0-9-]*', name) or name in fields:
                return ()
            fields[name] = value.strip()
        found = fields['suites'].split() if 'suites' in fields else []
        if (fields.get('uris') != uri or fields.get('types') != 'deb'
                or fields.get('signed-by') != KEYRING
                or fields.get('enabled', 'yes').lower() != 'yes'
                or fields.keys() & {'trusted', 'allow-insecure', 'allow-downgrade-to-insecure',
                                    'allow-weak', 'check-valid-until', 'check-date', 'date-max-future',
                                    'valid-until-min', 'valid-until-max', 'snapshot'}
                or not found or len(found) != len(set(found)) or suites.intersection(found)):
            return ()
        suites.update(found)
    return SUITES if suites == set(SUITES) else ()


def configured_source(env):
    if env.get('GITHUB_ACTIONS') != 'true' or env.get('RUNNER_OS') != 'Linux':
        return None
    try:
        for path, limit in ((MIRRORS, len(AZURE_FIRST)), (SOURCES, 32768)):
            info = path.lstat()
            if not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or not 0 < info.st_size <= limit:
                return None
        source = SOURCES.read_text(encoding='utf-8')
        return source if MIRRORS.read_bytes() == AZURE_FIRST and signed_noble_suites(source) else None
    except (OSError, UnicodeError):
        return None


def direct_sources(source, directory, uri):
    """Copy the Ubuntu stanza with only its URI changed, leaving /etc intact."""
    if not signed_noble_suites(source) or uri not in {value for _, value in DIRECT_MIRRORS}:
        raise ValueError('cannot replace an unrecognized Ubuntu mirror configuration')
    directory.mkdir(mode=0o755)
    original = f'mirror+file:{MIRRORS}'
    replacement = re.sub(rf'(?m)^(URIs:[ \t]*){re.escape(original)}(?=[ \t]*$)',
                         lambda match: match.group(1) + uri, source)
    if replacement == source or original in [
            line.split(':', 1)[1].strip() for line in replacement.splitlines()
            if line.startswith('URIs:')]:
        raise ValueError('could not replace all recognized Ubuntu URI lines')
    for path in sorted(SOURCES.parent.iterdir()):
        if path == SOURCES:
            continue
        if path.suffix in ('.list', '.sources') and re.fullmatch(r'[a-zA-Z0-9_.-]+', path.name):
            (directory / path.name).symlink_to(path)
    (directory / SOURCES.name).write_text(replacement, encoding='utf-8')


def release_date(data, suite, now):
    """Validate the signed payload rather than interpreting unverified headers."""
    if not 0 < len(data) <= MAX_RELEASE:
        raise ValueError('unexpected InRelease size')
    payload = subprocess.run(['gpgv', '--keyring', KEYRING, '--output', '-', '-'],
                             input=data, capture_output=True, timeout=2, check=True).stdout
    keys = ('Origin', 'Suite', 'Codename', 'Date', 'Valid-Until')
    fields = {}
    for line in payload.decode('utf-8').splitlines():
        name, separator, value = line.partition(': ')
        if separator and name in keys:
            if name in fields:
                raise ValueError('duplicate signed Release header')
            fields[name] = value
    if (fields.get('Origin'), fields.get('Suite'), fields.get('Codename')) != ('Ubuntu', suite, 'noble'):
        raise ValueError('unexpected signed Origin, Suite or Codename')
    date = parsedate_to_datetime(fields.get('Date', ''))
    if date.tzinfo is None or date > now + timedelta(seconds=10):
        raise ValueError('future or timezone-less signed Release')
    if suite in MUTABLE and date < now - MAX_POCKET_AGE:
        raise ValueError('stale signed Ubuntu update/security pocket')
    if 'Valid-Until' in fields:
        expiry = parsedate_to_datetime(fields['Valid-Until'])
        if expiry.tzinfo is None or expiry < now:
            raise ValueError('expired or timezone-less signed Release')
    return date


def probe_suite(uri, suite, now):
    """Select a transport only. APT independently downloads and verifies it."""
    start = time.monotonic()
    try:
        result = subprocess.run([
            'curl', '--disable', '--proto', '=https', '--silent', '--show-error',
            '--connect-timeout', '4', '--max-time', str(PROBE_TIMEOUT),
            '--max-filesize', str(MAX_RELEASE), '--write-out', '\n%{http_code}',
            f'{uri}dists/{suite}/InRelease',
        ], capture_output=True, timeout=PROBE_TIMEOUT + 1, check=True)
        body, _, status = result.stdout.rpartition(b'\n')
        if status != b'200':
            raise ValueError(f'HTTP {status.decode("ascii", errors="replace")[:3]}')
        return release_date(body, suite, now), time.monotonic() - start, None
    except subprocess.CalledProcessError as error:
        reason = 'bad archive signature' if error.cmd[0] == 'gpgv' else f'curl exit {error.returncode}'
    except subprocess.TimeoutExpired:
        reason = 'read/signature deadline'
    except (OSError, ValueError) as error:
        reason = str(error)
    return None, time.monotonic() - start, reason


def preflight():
    """Probe all fixed HTTPS suites concurrently; choose no more than two routes."""
    start = time.monotonic()
    now = datetime.now(timezone.utc)
    with ThreadPoolExecutor(max_workers=len(DIRECT_MIRRORS) * len(SUITES)) as pool:
        tasks = {name: {suite: pool.submit(probe_suite, uri, suite, now) for suite in SUITES}
                 for name, uri in DIRECT_MIRRORS}
    ready = []
    for name, uri in DIRECT_MIRRORS:
        dates = {}
        errors = []
        seconds = 0
        for suite, future in tasks[name].items():
            date, elapsed, reason = future.result()
            if reason is None:
                dates[suite] = date
            else:
                errors.append(f'{suite}: {reason}')
            seconds = max(elapsed, seconds)
        if errors:
            print(f'APT preflight {name}: unavailable ({", ".join(errors)})', flush=True)
        else:
            print(f'APT preflight {name}: {seconds:.3f}s; signed security {dates[MUTABLE[0]].isoformat()}, '
                  f'updates {dates[MUTABLE[1]].isoformat()}', flush=True)
            ready.append(Mirror(name, uri, dates, seconds))
    ready.sort(key=lambda mirror: (*(-mirror.dates[suite].timestamp() for suite in MUTABLE), mirror.seconds))
    chosen = tuple(ready[:2])
    print(f'APT preflight: {time.monotonic() - start:.3f}s; '
          f'{", ".join(mirror.name for mirror in chosen) or "one bounded stock attempt"}', flush=True)
    return chosen or (None,)


def verify_fresh_indexes(mirror, lists):
    """Do not let the authoritative refresh regress behind its signed preflight."""
    uri = urlsplit(mirror.uri)
    prefix = uri.netloc + uri.path.rstrip('/').replace('/', '_')
    now = datetime.now(timezone.utc)
    for suite in SUITES:
        path = lists / f'{prefix}_dists_{suite}_InRelease'
        with path.open('rb') as file:
            updated = release_date(file.read(MAX_RELEASE + 1), suite, now)
        if updated < mirror.dates[suite]:
            raise ValueError(f'APT {mirror.name} {suite} regressed behind its verified preflight')


def deadline_command(args, limit):
    # The watchdog runs inside sudo and wraps only index/package downloads.
    if (list(args[:2]) != ['sudo', 'apt-get']
            or args[-1] != 'update' and '--download-only' not in args):
        raise ValueError('APT deadline only applies to update or download-only')
    return ['sudo', 'timeout', '--signal=TERM', f'--kill-after={KILL_GRACE}s', f'{limit}s', *args[1:]]


@contextmanager
def acquire(update, prepare, download, reset, routes):
    """Retry only a pre-install deadline; never a trust or installation error."""
    if routes is None:
        update(None, None)
        prepare(None)
        yield None, False
        return
    for index, mirror in enumerate(routes):
        name = mirror.name if mirror is not None else 'stock'
        print(f'APT acquisition via {name}', flush=True)
        try:
            update(mirror, UPDATE_DEADLINE)
            prepare(mirror)
            download(mirror, DIRECT_DOWNLOAD_DEADLINE if mirror is not None else DOWNLOAD_DEADLINE)
        except AptDeadline as error:
            if index + 1 == len(routes):
                raise
            next_mirror = routes[index + 1]
            print(f'{error}; retry with {next_mirror.name if next_mirror else "stock"}', flush=True)
            reset()
        else:
            # The caller's package installation is outside the retry handler.
            yield mirror, True
            return
