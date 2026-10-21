#!/usr/bin/env python3
"""Validate the actual extracted CI AppImage; never launch the graphical app."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import struct
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
APP_ID = 'io.github.kordeone.Kara'


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def check_header(header):
    require(len(header) >= 20 and header[:4] == b'\x7fELF' and header[8:11] == b'AI\x02',
            'Not a type-2 AppImage')
    require(header[4:6] == b'\x02\x01' and struct.unpack('<H', header[18:20])[0] == 62,
            'AppImage must be little-endian x86_64 ELF64')


def check_versions(versions, path):
    require(not versions or max(versions) <= (2, 35), str(path) + ' exceeds glibc 2.35')


def validate_payload(root, builder):
    desktop = root / (APP_ID + '.desktop')
    require(desktop.read_bytes() == (root / 'usr/share/applications' / desktop.name).read_bytes(),
            'Root and installed desktop entries differ')
    subprocess.run(['desktop-file-validate', str(desktop)], check=True)
    subprocess.run(['appstreamcli', 'validate', '--no-net', str(root / 'usr/share/metainfo' / (APP_ID + '.metainfo.xml'))], check=True)
    for size in [32, 48, 64, 128, 256, 512]:
        relative = Path(f'usr/share/icons/hicolor/{size}x{size}/apps/{APP_ID}.png')
        require((root / relative).read_bytes() == (ROOT / 'resources' / str(relative).removeprefix('usr/share/')).read_bytes(),
                'Official icon differs: ' + str(size))
    require((root / (APP_ID + '.png')).read_bytes() == (ROOT / 'resources/icons/hicolor/512x512/apps' / (APP_ID + '.png')).read_bytes(),
            'Root launcher icon differs')
    require((root / '.DirIcon').resolve() == root / (APP_ID + '.png'), 'Invalid .DirIcon link')
    libraries = root / 'usr/lib'
    require({path.name for path in libraries.iterdir()} == {name for name, _ in builder.BUNDLED_LIBRARIES},
            'Unexpected or missing bundled runtime library')
    licenses = root / 'usr/share/licenses/kara'
    for relative in ['LICENSE', 'THIRD_PARTY_NOTICES.md', 'Tabler-Icons-LICENSE.txt',
                     'Rust-COPYRIGHT-library.html', 'dependencies/dependencies.json',
                     'runtime/libxkbcommon0-copyright', 'runtime/AppImage-runtime-LICENSE',
                     'runtime/musl-COPYRIGHT', 'runtime/libfuse-LGPL2.txt',
                     'runtime/squashfuse-LICENSE', 'runtime/zstd-LICENSE',
                     'runtime/zlib-LICENSE', 'runtime/runtime-sources.json']:
        require((licenses / relative).stat().st_size > 0, 'Missing license: ' + relative)
    require((licenses / 'LICENSE').read_bytes() == (ROOT / 'LICENSE').read_bytes(), 'Kara license differs')
    require((licenses / 'Tabler-Icons-LICENSE.txt').read_bytes() == (ROOT / 'assets/tabler/LICENSE.txt').read_bytes(), 'Tabler license differs')
    maxima = {}
    for file in [root / 'usr/bin/kara', *libraries.iterdir()]:
        header = file.read_bytes()[:20]
        require(header[:6] == b'\x7fELF\x02\x01' and struct.unpack('<H', header[18:20])[0] == 62,
                'Non-x86_64 payload ELF: ' + str(file))
        versions = builder.glibc_versions(file)
        check_versions(versions, file)
        maxima[str(file.relative_to(root))] = '.'.join(map(str, versions[-1])) if versions else None
        env = dict(os.environ, LD_LIBRARY_PATH=str(libraries))
        linked = subprocess.check_output(['ldd', str(file)], env=env, text=True)
        require('not found' not in linked, 'Unresolved payload dependency: ' + linked)
    require(os.access(root / 'AppRun', os.X_OK), 'AppRun is not executable')
    subprocess.run(['shellcheck', str(root / 'AppRun')], check=True)
    return maxima


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('artifact', type=Path)
    args = parser.parse_args()
    artifact = args.artifact.resolve()
    with artifact.open('rb') as file:
        check_header(file.read(20))
    require(os.access(artifact, os.X_OK), 'AppImage is not executable')
    spec = importlib.util.spec_from_file_location('appimage_builder', ROOT / 'packaging/appimage/build.py')
    builder = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(builder)
    check_versions(builder.glibc_versions(artifact), artifact)
    evidence = json.loads(artifact.with_suffix('.AppImage.json').read_text())
    digest = hashlib.sha256(artifact.read_bytes()).hexdigest()
    require(evidence['sha256'] == digest and evidence['artifact_bytes'] == artifact.stat().st_size,
            'Artifact and assembly evidence differ')
    with tempfile.TemporaryDirectory(prefix='kara-appimage-extract-') as temporary:
        subprocess.run([str(artifact), '--appimage-extract'], cwd=temporary, check=True,
                       stdout=subprocess.DEVNULL)
        maxima = validate_payload(Path(temporary) / 'squashfs-root', builder)
    require(maxima == evidence['payload_symbol_maxima'], 'Extracted glibc requirements differ from assembly evidence')
    report = {'artifact': artifact.name, 'artifact_bytes': artifact.stat().st_size,
              'sha256': digest, 'validated_payload_glibc': maxima,
              'extraction': 'passed', 'metadata_icons_licenses_dependencies': 'passed',
              'graphical_acceptance': 'not performed'}
    artifact.with_suffix('.AppImage.validation.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
