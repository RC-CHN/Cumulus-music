"""Seed the pinned setup-msys2 installer cache without executing the installer."""
import http.client
import os
from pathlib import Path
import ssl
import stat
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request

from windows_media import sha256


# Same SFX and cache identity as setup-msys2 ec48f7c5447b3140e2b088413ae3a55687bccb6e.
ARCHIVE = 'msys2-base-x86_64-20260927.sfx.exe'
ARCHIVE_BYTES = 43_117_824
ARCHIVE_SHA256 = 'ad336cccfda47758b5e15cda993fbba421115cb0b126697daef1ee4dfe37209f'
TOOL_CACHE_VERSION = '0.0.20260927'
SOURCES = (
    ('MSYS2 distribution', f'https://repo.msys2.org/distrib/x86_64/{ARCHIVE}'),
    ('GitHub release', f'https://github.com/msys2/msys2-installer/releases/download/2026-09-27/{ARCHIVE}'),
)
OFFICIAL_HOSTS = {
    'repo.msys2.org',
    'github.com',
    'release-assets.githubusercontent.com',
    'objects.githubusercontent.com',
}
# The distribution may eventually retire this pinned version. The GitHub
# release can still supply those same bytes, as it can after a transient error.
UNAVAILABLE_HTTP = {404, 408, 410, 429, 500, 502, 503, 504}
TRANSFER_SECONDS = 100


class BootstrapError(RuntimeError):
    pass


def require_official_https(url):
    try:
        target = urllib.parse.urlsplit(url)
        port = target.port
    except ValueError:
        raise BootstrapError('Installer download URL is malformed.') from None
    if (target.scheme != 'https' or target.hostname not in OFFICIAL_HOSTS
            or target.username is not None or target.password is not None
            or port not in (None, 443)):
        raise BootstrapError('Installer download must stay on an official HTTPS origin.')


class OfficialRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, message, headers, newurl):
        require_official_https(newurl)
        return super().redirect_request(request, fp, code, message, headers, newurl)


def managed_entry(path, predicate):
    """Do not read through a symlink or a Windows junction in the managed cache."""
    try:
        info = path.lstat()
    except FileNotFoundError:
        return False
    reparse = getattr(info, 'st_file_attributes', 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT
    if reparse or not predicate(info.st_mode):
        raise BootstrapError('Unexpected file type in the MSYS2 installer tool cache.')
    return True


def verified(path):
    return (managed_entry(path, stat.S_ISREG)
            and path.stat().st_size == ARCHIVE_BYTES and sha256(path) == ARCHIVE_SHA256)


def transfer(url, destination, open_url):
    require_official_https(url)
    deadline = time.monotonic() + TRANSFER_SECONDS
    with open_url(url, timeout=25) as response, destination.open('xb') as output:
        require_official_https(response.geturl())
        if response.status != 200:
            raise BootstrapError('Installer download returned an unexpected response status.')
        length = response.headers.get('Content-Length')
        if length is not None and length != str(ARCHIVE_BYTES):
            raise BootstrapError('MSYS2 installer has the wrong Content-Length.')
        size = 0
        while True:
            # read1 lets a slow peer return a short block before we check the
            # deadline; read at most the exact size plus one byte.
            block = response.read1(min(256 * 1024, ARCHIVE_BYTES - size + 1))
            if time.monotonic() >= deadline:
                raise TimeoutError
            if not block:
                break
            size += len(block)
            if size > ARCHIVE_BYTES:
                raise BootstrapError('MSYS2 installer exceeds its pinned byte length.')
            output.write(block)
        if length is not None and size < ARCHIVE_BYTES:
            # HTTPResponse.read1 may report an early EOF rather than raise
            # IncompleteRead for a connection that ended before its framing.
            raise http.client.IncompleteRead(b'', ARCHIVE_BYTES - size)
    if not verified(destination):
        raise BootstrapError('MSYS2 installer failed its pinned byte length or SHA-256 check.')


def download(destination, open_url):
    for source, url in SOURCES:
        try:
            transfer(url, destination, open_url)
        except urllib.error.HTTPError as error:
            error.close()
            if error.code not in UNAVAILABLE_HTTP:
                raise BootstrapError(f'{source}: installer download returned HTTP {error.code}.') from None
            print(f'{source}: installer download returned HTTP {error.code}.', flush=True)
        except urllib.error.URLError as error:
            if isinstance(error.reason, ssl.SSLError):
                raise BootstrapError(f'{source}: installer TLS verification failed.') from None
            print(f'{source}: installer transfer failed ({type(error.reason).__name__}).', flush=True)
        except ssl.SSLError:
            raise BootstrapError(f'{source}: installer TLS transfer failed.') from None
        except (TimeoutError, ConnectionError, http.client.IncompleteRead) as error:
            print(f'{source}: installer transfer failed ({type(error).__name__}).', flush=True)
        else:
            print(f'{source}: verified pinned MSYS2 installer.', flush=True)
            return
        destination.unlink(missing_ok=True)
    raise BootstrapError('Both official MSYS2 installer sources failed to transfer.')


def seed(root, open_url=None):
    if not root.is_absolute() or not root.is_dir():
        raise BootstrapError('RUNNER_TOOL_CACHE must be an existing absolute directory.')
    cache = root
    for component in ('msys2-installer', TOOL_CACHE_VERSION, 'x64'):
        cache = cache / component
        cache.mkdir(exist_ok=True)
        managed_entry(cache, stat.S_ISDIR)
    installer = cache / 'base.exe'
    marker = cache.with_suffix('.complete')
    ready = managed_entry(marker, stat.S_ISREG)
    cached = verified(installer)
    if ready and cached:
        print('MSYS2 installer tool cache already verified.', flush=True)
        return

    # An interrupted recovery can leave an unmarked file but never a ready
    # unverified one. The pinned action checks this marker, then rehashes.
    if ready:
        marker.unlink()
    with tempfile.TemporaryDirectory(dir=cache, prefix='.bootstrap-') as directory:
        scratch = Path(directory)
        if not cached:
            download(scratch / 'base.exe', open_url or urllib.request.build_opener(OfficialRedirect()).open)
            os.replace(scratch / 'base.exe', installer)
        ready_file = scratch / 'complete'
        ready_file.touch(exist_ok=False)
        os.replace(ready_file, marker)


def main():
    try:
        seed(Path(os.environ.get('RUNNER_TOOL_CACHE', '')))
    except BootstrapError as error:
        raise SystemExit(str(error)) from None


if __name__ == '__main__':
    main()
