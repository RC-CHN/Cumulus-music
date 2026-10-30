#!/usr/bin/env bash
# Called only through windows_media.py, with an isolated build directory.
set -euo pipefail
test "$MSYSTEM" = UCRT64
test "$MINGW_PACKAGE_PREFIX" = mingw-w64-ucrt-x86_64
cd "$(cygpath -u "$1")"
export MAKEFLAGS="-j${CUMULUS_MEDIA_BUILD_JOBS:-2}"
# Source integrity comes from the pinned recipe/source SHA256s. The patched
# recipe deliberately has no .asc download or unauthenticated keyserver step.
makepkg-mingw --syncdeps --noconfirm --cleanbuild --clean
