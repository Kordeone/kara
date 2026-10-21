#!/usr/bin/env python3
"""Export an already validated glibc-2.35 payload without executing it unsandboxed.

This supported prebuilt-payload route does not replace sandbox launch testing.
The source-build manifest remains the preferred native SDK build route.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--payload', required=True, type=Path, help='staged /app contents')
    parser.add_argument('--builddir', required=True, type=Path, help='new directory')
    parser.add_argument('--repo', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    if args.builddir.exists() or args.output.exists():
        parser.error('builddir and output must not already exist')
    manifest = json.loads((ROOT / 'packaging/flatpak/io.github.kordeone.Kara.json').read_text())
    binary = args.payload / 'bin/kara'
    versions = re.findall(r'GLIBC_([0-9]+\.[0-9]+)', subprocess.check_output(
        ['readelf', '--version-info', str(binary)], text=True))
    if any(tuple(map(int, value.split('.'))) > (2, 35) for value in versions):
        parser.error('prebuilt route requires verified glibc <=2.35 payload')
    subprocess.run(['desktop-file-validate', str(args.payload / 'share/applications' / (manifest['app-id'] + '.desktop'))], check=True)
    subprocess.run(['appstreamcli', 'validate', '--no-net', str(args.payload / 'share/metainfo' / (manifest['app-id'] + '.metainfo.xml'))], check=True)
    subprocess.run(['flatpak', 'build-init', '--arch=x86_64', str(args.builddir), manifest['app-id'],
                    manifest['sdk'], manifest['runtime'], manifest['runtime-version']], check=True)
    shutil.copytree(args.payload, args.builddir / 'files', dirs_exist_ok=True)
    subprocess.run(['strip', '--strip-unneeded', str(args.builddir / 'files/bin/kara')], check=True)
    subprocess.run(['flatpak', 'build-finish', '--command=' + manifest['command'],
                    *manifest['finish-args'], str(args.builddir)], check=True)
    subprocess.run(['flatpak', 'build-export', '--arch=x86_64', str(args.repo), str(args.builddir)], check=True)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(['flatpak', 'build-bundle', '--arch=x86_64',
                    '--runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo', str(args.repo),
                    str(args.output), manifest['app-id']], check=True)
    record = {'route': 'prebuilt Ubuntu 22.04 payload; full native SDK build/launch blocked by nested namespace restriction',
              'app_id': manifest['app-id'], 'runtime': manifest['runtime'] + '//'+manifest['runtime-version'],
              'finish_args': manifest['finish-args'], 'artifact_bytes': args.output.stat().st_size,
              'sha256': hashlib.sha256(args.output.read_bytes()).hexdigest(),
              'executable_sha256': hashlib.sha256((args.builddir / 'files/bin/kara').read_bytes()).hexdigest()}
    args.output.with_suffix(args.output.suffix + '.json').write_text(json.dumps(record, indent=2) + '\n')
    print(json.dumps(record, indent=2))


if __name__ == '__main__':
    main()
