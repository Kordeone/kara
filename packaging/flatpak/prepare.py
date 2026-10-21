#!/usr/bin/env python3
"""Prepare a local offline Flatpak source tree and exact locked vendor sources."""
import argparse
import json
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path, help='new, nonexistent staging directory')
    parser.add_argument('--toolchain', type=Path, help='optional exact Rust 1.99.1 directory; bypasses rolling SDK Rust extension')
    args = parser.parse_args()
    output = args.output.resolve()
    if output.exists() or output == ROOT or ROOT in output.parents:
        parser.error('output must be new and outside the source tree')
    output.mkdir(parents=True)
    source = output / 'source'
    shutil.copytree(ROOT, source, ignore=shutil.ignore_patterns('.git', 'target', 'dist', '__pycache__', 'work'))
    config = source / '.cargo'
    config.mkdir(exist_ok=True)
    with (config / 'config.toml').open('w') as file:
        subprocess.run(['cargo', 'vendor', '--locked', '--versioned-dirs', 'vendor'], cwd=source, stdout=file, check=True)
    manifest = json.loads((ROOT / 'packaging/flatpak/io.github.kordeone.Kara.json').read_text())
    manifest['modules'][0]['sources'][0] = {'type': 'dir', 'path': str(source)}
    if args.toolchain:
        toolchain = args.toolchain.resolve()
        version = subprocess.check_output([str(toolchain / 'bin/rustc'), '--version'], text=True)
        if not version.startswith('rustc 1.99.1 '):
            parser.error('toolchain must be Rust 1.99.1')
        manifest.pop('sdk-extensions')
        manifest['build-options']['append-path'] = '/run/build/kara/pinned-rust/bin'
        manifest['modules'][0]['sources'].append({'type': 'dir', 'path': str(toolchain), 'dest': 'pinned-rust'})
    path = output / 'io.github.kordeone.Kara.json'
    path.write_text(json.dumps(manifest, indent=2) + '\n')
    print(path)


if __name__ == '__main__':
    main()
