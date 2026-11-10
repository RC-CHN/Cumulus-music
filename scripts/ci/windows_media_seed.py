"""One reviewed Windows FFmpeg cache seed; never trust a prefix match."""
import hashlib
import json
import stat
import subprocess

from windows_media_recipe import HERE, ROOT, RECIPE_INPUTS, cache_key, sha256

# Verified runtime artifact 13724479766 (run 42730743761) used an exact cache
# hit. Its independently verified manifest covers the retained build receipt.
# The later release job cold-built different archive bytes, so its digest
# cannot attest the separate archive cached by its parallel Native job.
REVIEWED_RECIPE = 'dfb4bde0225808d8cdb5865cd0cac4da94a7c4844530d700265d6d15c1503b88'
LEGACY_BLOB = 'd5521083db443f2d6e67df5f9390186620776953'
LEGACY_BLOB_SHA256 = '6c0c0f68a45525b243e9dbede1973a953afa7744a234c81a50c0285fc47d2fb0'
LEGACY_CACHE_KEY = 'windows-media-ucrt64-dcedee48f57b31f8536d51fcfa81a2e470fde5fc3130557099c2f764c2f17cf4'
INVENTORY_SHA256 = '480149f0fc04f3ba474151e28478306f15e830915de4146664476d38ff3b26bd'
PATCHED_CONFIG_SHA256 = '874f974eb056aa32457cf0c175fb0f19bba1eabd4eaeea5f9d2e117f969169db'
RECEIPT_SHA256 = '72f29f401a2d78410419c27b1bd0bc2391ed869ee9786cfbe9920f523a35f2c9'
PACKAGE_NAME = 'mingw-w64-ucrt-x86_64-ffmpeg-9.0.2-3.2-any.pkg.tar.zst'
PACKAGE_SHA256 = 'f356fe7b7ed872185609d1cd4c8e33af55d9f88f27ad0dae911423e19809ecf9'


def digest(data):
    return hashlib.sha256(data).hexdigest()


def patched_configuration(config):
    # The reviewed patch adds this one flag to the shared build.
    if config.count('--enable-gpl') != 1 or '--enable-ladspa' in config:
        return None
    i = config.index('--enable-gpl') + 1
    return config[:i] + ['--enable-ladspa'] + config[i:]


def source_blob():
    try:
        return subprocess.check_output(['git', 'cat-file', 'blob', LEGACY_BLOB], cwd=ROOT, stderr=subprocess.DEVNULL)
    except (OSError, subprocess.CalledProcessError):
        # A shallow checkout must not fetch history just to seed a cache.
        return None


def legacy_key(plan):
    packages = plan['packages'].encode()
    args = plan['configuration']
    patched = patched_configuration(args)
    if (digest(packages) != INVENTORY_SHA256 or patched is None or
            digest(json.dumps(patched, separators=(',', ':')).encode()) != PATCHED_CONFIG_SHA256):
        return None
    inputs = [(HERE / name).read_bytes() for name in RECIPE_INPUTS]
    static = b''.join(len(data).to_bytes(8, 'big') + data for data in inputs)
    if digest(static) != REVIEWED_RECIPE or plan['key'] != cache_key(plan['packages'], args):
        return None
    script = source_blob()
    if script is None or digest(script) != LEGACY_BLOB_SHA256:
        return None
    _, lock, build, patch = inputs
    key = 'windows-media-ucrt64-' + digest(lock + script + build + patch + packages)
    return key if key == LEGACY_CACHE_KEY else None


def plain(info):
    return not (getattr(info, 'st_file_attributes', 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT)


def verify_seed(cache, plan, matched, lock, verify_cache):
    expected = legacy_key(plan)
    assert expected is not None and matched == expected, 'legacy media cache is not an exact reviewed hit'
    root = cache.lstat()
    assert stat.S_ISDIR(root.st_mode) and plain(root), 'legacy media cache must be a plain directory'
    members = set()
    for path in cache.iterdir():
        info = path.lstat()
        assert stat.S_ISREG(info.st_mode) and plain(info), 'legacy media cache has a non-file member'
        members.add(path.name)
    receipt_path = cache / 'build.json'
    assert sha256(receipt_path) == RECEIPT_SHA256, 'legacy media receipt differs from attested bytes'
    receipt = json.loads(receipt_path.read_text(encoding='utf-8'))
    assert members == {*receipt['files'], 'build.json'}, 'legacy media cache has an unreviewed file'
    package = verify_cache(cache, {**plan, 'key': expected}, lock)
    assert package.name == PACKAGE_NAME and sha256(package) == PACKAGE_SHA256, 'legacy media archive differs from attested package'
    return package
