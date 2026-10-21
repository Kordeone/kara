#!/usr/bin/env python3
"""Build and stage a locked Kara payload without source-tree build outputs."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
APP_ID = 'io.github.kordeone.Kara'


def metadata():
    return json.loads(subprocess.check_output(
        ['cargo', 'metadata', '--locked', '--offline', '--filter-platform', 'x86_64-unknown-linux-gnu', '--format-version=1'], cwd=ROOT))


def license_files(package):
    base = Path(package['manifest_path']).parent
    candidates = []
    if package.get('license_file'):
        candidates.append(base / package['license_file'])
    # Workspace crates can inherit their repository root's license.
    for parent in [base, *base.parents]:
        candidates.extend(p for p in parent.iterdir() if p.is_file() and
                          p.name.lower().startswith(('license', 'licence', 'copying', 'notice', 'copyright')))
        if (parent / '.git').exists() or (parent / '.cargo-checksum.json').exists():
            break
    if not candidates:
        for parent in [base, *base.parents]:
            source_license = parent / 'src/LICENSE'
            if source_license.is_file():
                candidates.append(source_license)
                break
            if (parent / '.git').exists() or (parent / '.cargo-checksum.json').exists():
                break
    supplement = ROOT / 'packaging/licenses/sources.json'
    if supplement.exists():
        for record in json.loads(supplement.read_text()).get(package['name'] + '@' + package['version'], []):
            import hashlib
            file = supplement.parent / record['file']
            if hashlib.sha256(file.read_bytes()).hexdigest() != record['sha256']:
                raise RuntimeError('Supplementary license checksum mismatch: ' + str(file))
            candidates.append(file)
    return sorted(set(p.resolve() for p in candidates if p.is_file()))


def notices(graph, destination):
    destination.mkdir(parents=True, exist_ok=True)
    records = []
    for package in sorted(graph['packages'], key=lambda p: p['id']):
        if package['name'] == 'kara' and package['source'] is None:
            continue
        files = license_files(package)
        if not files:
            raise RuntimeError('No license text found for ' + package['id'])
        import hashlib
        key = package['name'] + '-' + package['version'] + '-' + hashlib.sha256(package['id'].encode()).hexdigest()[:10]
        folder = destination / key
        folder.mkdir(exist_ok=True)
        copied = []
        for i, file in enumerate(files):
            name = str(i) + '-' + file.name
            shutil.copyfile(file, folder / name)
            copied.append(key + '/' + name)
        records.append({k: package.get(k) for k in ('name', 'version', 'source', 'license', 'repository')} |
                       {'texts': copied, 'license_text_review_required': package['name'] == 'drm-fourcc'})
        if package['name'] == 'libcosmic':
            for font in ['noto', 'open-sans']:
                source = ROOT / 'packaging/licenses' / ('libcosmic-font-' + font + '-LICENSE')
                shutil.copyfile(source, destination / ('libcosmic-font-' + font + '-LICENSE'))
    supplement = ROOT / 'packaging/licenses/sources.json'
    if supplement.exists():
        shutil.copyfile(supplement, destination / 'supplementary-sources.json')
    (destination / 'dependencies.json').write_text(json.dumps(records, indent=2) + '\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--destdir', required=True, type=Path)
    parser.add_argument('--prefix', default='/usr')
    parser.add_argument('--offline', action='store_true')
    args = parser.parse_args()
    if not args.prefix.startswith('/') or '..' in Path(args.prefix).parts:
        parser.error('prefix must be absolute without parent traversal')
    expected = 'rustc 1.99.1 '
    actual = subprocess.check_output(['rustc', '--version'], text=True)
    if not actual.startswith(expected):
        raise RuntimeError('Use the pinned Rust 1.99.1 toolchain; found ' + actual.strip())
    graph = metadata()
    root_id = next(p['id'] for p in graph['packages'] if Path(p['manifest_path']) == ROOT / 'Cargo.toml')
    command = ['cargo', 'build', '--release', '--locked', '--message-format=json-render-diagnostics']
    if args.offline:
        command += ['--frozen', '--offline']
    process = subprocess.Popen(command, cwd=ROOT, stdout=subprocess.PIPE, text=True)
    binary = generated = None
    for line in process.stdout:
        message = json.loads(line)
        if message.get('package_id') != root_id:
            continue
        if message['reason'] == 'build-script-executed':
            generated = Path(message['out_dir']) / 'xdgen'
        if message['reason'] == 'compiler-artifact' and message.get('executable'):
            binary = Path(message['executable'])
    if process.wait() or binary is None or generated is None:
        raise RuntimeError('Build failed or Cargo did not return binary/metadata locations')
    prefix = args.destdir.resolve() / args.prefix.lstrip('/')
    def install(source, relative, executable=False):
        path = prefix / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, path)
        path.chmod(0o755 if executable else 0o644)
    install(binary, 'bin/kara', True)
    install(generated / 'app.desktop', 'share/applications/' + APP_ID + '.desktop')
    install(generated / 'app.metainfo.xml', 'share/metainfo/' + APP_ID + '.metainfo.xml')
    for image in sorted((ROOT / 'resources/icons/hicolor').glob('*/apps/' + APP_ID + '.png')):
        install(image, 'share/icons/hicolor/' + str(image.relative_to(ROOT / 'resources/icons/hicolor')))
    install(ROOT / 'LICENSE', 'share/licenses/kara/LICENSE')
    install(ROOT / 'THIRD_PARTY_NOTICES.md', 'share/licenses/kara/THIRD_PARTY_NOTICES.md')
    install(ROOT / 'assets/tabler/LICENSE.txt', 'share/licenses/kara/Tabler-Icons-LICENSE.txt')
    install(ROOT / 'packaging/licenses/CC0-1.0.txt', 'share/licenses/kara/CC0-1.0.txt')
    import hashlib
    rust_record = json.loads((ROOT / 'packaging/licenses/rust-notices.json').read_text())
    rust_notices = ROOT / 'packaging/licenses' / rust_record['file']
    if hashlib.sha256(rust_notices.read_bytes()).hexdigest() != rust_record['sha256']:
        raise RuntimeError('Rust standard-library notice checksum mismatch')
    install(rust_notices, 'share/licenses/kara/Rust-COPYRIGHT-library.html')
    notices(graph, prefix / 'share/licenses/kara/dependencies')
    subprocess.run(['desktop-file-validate', str(prefix / 'share/applications' / (APP_ID + '.desktop'))], check=True)
    subprocess.run(['appstreamcli', 'validate', '--no-net', str(prefix / 'share/metainfo' / (APP_ID + '.metainfo.xml'))], check=True)
    print('Staged validated payload:', prefix)


if __name__ == '__main__':
    main()
