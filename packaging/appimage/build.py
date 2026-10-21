#!/usr/bin/env python3
"""Assemble a local x86_64 test AppImage from an Ubuntu 22.04 staged payload."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess

APP_ID = 'io.github.kordeone.Kara'
BUNDLED_LIBRARIES = [('libxkbcommon.so.0', 'libxkbcommon0')]
HOST_LIBRARIES = ('libgcc_s.so.1', 'libstdc++.so.6', 'libc.so.6', 'libm.so.6',
                  'ld-linux-x86-64.so.2', 'libGL.so.1', 'libEGL.so.1', 'libvulkan.so.1')


def glibc_versions(binary):
    text = subprocess.check_output(['readelf', '--version-info', str(binary)], text=True)
    return sorted({tuple(map(int, value.split('.'))) for value in re.findall(r'GLIBC_([0-9]+\.[0-9]+)', text)})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--appdir', required=True, type=Path)
    parser.add_argument('--tools', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--epoch', required=True, type=int)
    args = parser.parse_args()
    if args.epoch < 0:
        parser.error('epoch must be nonnegative')
    if args.output.exists():
        parser.error('output already exists; choose a new artifact path')
    root = args.appdir.resolve()
    binary = root / 'usr/bin/kara'
    if not binary.is_file():
        parser.error('stage the payload with packaging/build.py first')
    records = json.loads((args.tools / 'sources.json').read_text())
    for record in records:
        file = args.tools / record['url'].rsplit('/', 1)[1]
        if hashlib.sha256(file.read_bytes()).hexdigest() != record['sha256']:
            raise RuntimeError('AppImage tool checksum mismatch: ' + str(file))
    libraries = root / 'usr/lib'
    libraries.mkdir(exist_ok=True)
    license_dir = root / 'usr/share/licenses/kara/runtime'
    license_dir.mkdir(parents=True, exist_ok=True)
    if any((libraries / name).exists() for name in HOST_LIBRARIES):
        raise RuntimeError('Use a fresh AppDir without bundled host compiler/libc/graphics libraries')
    for soname, package in BUNDLED_LIBRARIES:
        source = Path('/usr/lib/x86_64-linux-gnu') / soname
        shutil.copyfile(source.resolve(), libraries / soname)
        shutil.copyfile(Path('/usr/share/doc') / package / 'copyright', license_dir / (package + '-copyright'))
    source = Path(__file__).resolve().parents[1] / 'licenses'
    for name in ['AppImage-runtime-LICENSE', 'musl-COPYRIGHT', 'libfuse-LGPL2.txt', 'squashfuse-LICENSE', 'zstd-LICENSE', 'zlib-LICENSE', 'runtime-sources.json']:
        shutil.copyfile(source / name, license_dir / name)
    maxima = {}
    for file in [binary, *libraries.iterdir()]:
        versions = glibc_versions(file)
        if versions and versions[-1] > (2, 35):
            raise RuntimeError(str(file) + ' exceeds promised glibc 2.35 baseline')
        maxima[str(file.relative_to(root))] = '.'.join(map(str, versions[-1])) if versions else None
    subprocess.run(['strip', '--strip-unneeded', str(binary)], check=True)
    shutil.copyfile(root / 'usr/share/applications' / (APP_ID + '.desktop'), root / (APP_ID + '.desktop'))
    shutil.copyfile(root / 'usr/share/icons/hicolor/512x512/apps' / (APP_ID + '.png'), root / (APP_ID + '.png'))
    link = root / '.DirIcon'
    if not link.exists():
        link.symlink_to(APP_ID + '.png')
    launcher = root / 'AppRun'
    launcher.write_text('''#!/bin/sh
set -eu
APPDIR=${APPDIR:-$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)}
export LD_LIBRARY_PATH="$APPDIR/usr/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export XDG_DATA_DIRS="$APPDIR/usr/share:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
exec "$APPDIR/usr/bin/kara" "$@"
''')
    launcher.chmod(0o755)
    subprocess.run(['desktop-file-validate', str(root / (APP_ID + '.desktop'))], check=True)
    subprocess.run(['appstreamcli', 'validate', '--no-net', str(root / 'usr/share/metainfo' / (APP_ID + '.metainfo.xml'))], check=True)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env.update(APPIMAGE_EXTRACT_AND_RUN='1', ARCH='x86_64', SOURCE_DATE_EPOCH=str(args.epoch))
    command = [str(args.tools.resolve() / 'appimagetool-x86_64.AppImage'), '--runtime-file',
               str(args.tools.resolve() / 'runtime-x86_64'),
               '--mksquashfs-opt=-processors', '--mksquashfs-opt=2', str(root), str(args.output.resolve())]
    subprocess.run(command, env=env, check=True)
    args.output.chmod(0o755)
    with args.output.open('rb') as file:
        header = file.read(11)
    if header[:4] != b'\x7fELF' or header[8:11] != b'AI\x02':
        raise RuntimeError('Output is not a type-2 ELF AppImage')
    evidence = {'minimum_glibc_policy': '2.35', 'payload_symbol_maxima': maxima, 'host_libraries': HOST_LIBRARIES,
                'artifact_bytes': args.output.stat().st_size,
                'sha256': hashlib.sha256(args.output.read_bytes()).hexdigest(), 'tools': records}
    args.output.with_suffix(args.output.suffix + '.json').write_text(json.dumps(evidence, indent=2) + '\n')
    print(json.dumps(evidence, indent=2))


if __name__ == '__main__':
    main()
