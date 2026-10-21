#!/usr/bin/env python3
"""Install checksum-pinned official Rust components and actionlint in CI only."""
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import urllib.request


def fetch(record, folder):
    archive = folder / record['url'].rsplit('/', 1)[1]
    with urllib.request.urlopen(record['url'], timeout=120) as response:
        archive.write_bytes(response.read())
    if hashlib.sha256(archive.read_bytes()).hexdigest() != record['sha256']:
        raise RuntimeError('Tool download checksum mismatch: ' + archive.name)
    return archive


def main():
    lock = json.loads(Path(__file__).with_name('tools.lock.json').read_text())
    # /tmp exists during image construction, before /work is mounted at runtime.
    with tempfile.TemporaryDirectory(dir='/tmp') as directory:
        folder = Path(directory)
        for component in lock['rust_components']:
            archive = fetch(component, folder)
            subprocess.run(['tar', '-xJf', str(archive), '-C', str(folder)], check=True)
            installer = folder / archive.name.removesuffix('.tar.xz') / 'install.sh'
            subprocess.run(['sh', str(installer), '--prefix=/opt/rust', '--disable-ldconfig'], check=True)
        archive = fetch(lock['actionlint'], folder)
        Path('/opt/ci/bin').mkdir(parents=True)
        subprocess.run(['tar', '-xzf', str(archive), '-C', '/opt/ci/bin', 'actionlint'], check=True)
    subprocess.run(['/opt/rust/bin/rustc', '--version', '--verbose'], check=True)
    subprocess.run(['/opt/ci/bin/actionlint', '--version'], check=True)


if __name__ == '__main__':
    main()
