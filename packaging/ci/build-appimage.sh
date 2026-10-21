#!/bin/bash
# One entry point for GitHub Actions and isolated local CI-step validation.
set -euo pipefail
if [ "$(id -u)" -eq 0 ]; then
    echo 'Run CI as an unprivileged user; root invalidates filesystem-denial tests.' >&2
    exit 2
fi
source_root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$source_root"
if [ "$#" -ne 1 ] || [ -e "$1" ]; then
    echo 'Usage: build-appimage.sh NEW_BUILD_DIRECTORY' >&2
    exit 2
fi
mkdir -p "$1"
build_root=$(CDPATH='' cd -- "$1" && pwd)
artifacts="$build_root/artifacts"
mkdir -p "$artifacts" "${TMPDIR:-/tmp}" "$HOME"
exec > >(tee "$build_root/build.log") 2>&1
python3 packaging/ci/check-identity.py
# All downloads precede the frozen/offline source build. No shared CI cache.
cargo fetch --locked
export CARGO_NET_OFFLINE=true
# A container may mount a git worktree read-only. Trust only this exact path.
commit=$(git -c safe.directory="$source_root" rev-parse HEAD)
epoch=$(git -c safe.directory="$source_root" show -s --format=%ct HEAD)
version=$(python3 -c 'import json, subprocess; print(json.loads(subprocess.check_output(["cargo", "metadata", "--locked", "--offline", "--no-deps", "--format-version=1"]))["packages"][0]["version"])')
name="Kara-${version}-testing-${commit:0:12}-x86_64.AppImage"
{
    printf 'source_commit=%s\nsource_date_epoch=%s\n' "$commit" "$epoch"
    rustc --version --verbose
    cargo --version
    actionlint --version
    shellcheck --version
    dpkg-query -W -f='${Package}\t${Version}\n'
} > "$artifacts/build-environment.txt"
actionlint .github/workflows/appimage.yml
shellcheck packaging/validate.sh packaging/ci/build-appimage.sh
bash packaging/validate.sh
python3 packaging/ci/test_ci.py
python3 packaging/build.py --destdir "$build_root/AppDir" --prefix /usr --offline
python3 packaging/appimage/fetch-tools.py "$build_root/tools"
python3 packaging/appimage/build.py --appdir "$build_root/AppDir" \
    --tools "$build_root/tools" --epoch "$epoch" --output "$artifacts/$name"
python3 packaging/ci/verify-appimage.py "$artifacts/$name"
(
    cd "$artifacts"
    sha256sum "$name" > "$name.sha256"
    sha256sum --check "$name.sha256"
)
python3 - "$artifacts/$name" "$commit" <<'PY'
import hashlib
from pathlib import Path
import sys
artifact = Path(sys.argv[1])
size = artifact.stat().st_size
summary = (f'# Kara AppImage testing artifact\n\nSource: `{sys.argv[2]}`\n\n'
           f'File: `{artifact.name}`\n\nActual size: {size:,} bytes ({size / 1048576:.2f} MiB)\n\n'
           f'SHA-256: `{hashlib.sha256(artifact.read_bytes()).hexdigest()}`\n\n'
           'x86_64; maximum required glibc 2.35. Headless checks only; no GUI acceptance.\n')
(artifact.parent / 'summary.md').write_text(summary)
print(summary)
PY
