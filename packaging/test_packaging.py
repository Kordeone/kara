#!/usr/bin/env python3
"""Deterministic packaging contract checks; no GUI or user data needed."""
import importlib.util
import json
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import patch
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parent.parent
APP_ID = 'io.github.kordeone.Kara'


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class PackagingTests(unittest.TestCase):
    def test_metadata_identity_and_template_removal(self):
        root = ET.parse(ROOT / 'resources/app.metainfo.xml').getroot()
        self.assertEqual(root.findtext('id'), APP_ID)
        self.assertEqual(root.findtext('launchable'), APP_ID + '.desktop')
        self.assertEqual(root.findtext('provides/binary'), 'kara')
        self.assertIsNotNone(root.find('description/p'))
        self.assertNotIn('com.system76.CosmicApplication', ET.tostring(root).decode())

    def test_desktop_does_not_advertise_unsupported_files(self):
        text = (ROOT / 'resources/app.desktop').read_text()
        self.assertIn('Exec=kara\n', text)
        self.assertIn('Icon=' + APP_ID + '\n', text)
        self.assertIn('Categories=Office;ProjectManagement;', text)
        self.assertNotIn('%F', text)
        self.assertNotIn('MimeType=', text)

    def test_flatpak_permission_allowlist(self):
        manifest = json.loads((ROOT / 'packaging/flatpak' / (APP_ID + '.json')).read_text())
        self.assertEqual(manifest['app-id'], APP_ID)
        self.assertEqual(manifest['command'], 'kara')
        self.assertEqual(set(manifest['finish-args']), {
            '--socket=wayland', '--device=dri', '--talk-name=org.kde.StatusNotifierWatcher',
            '--talk-name=com.system76.CosmicSettingsDaemon',
            '--talk-name=com.system76.CosmicSettingsDaemon.Config',
            *['--filesystem=xdg-config/cosmic/' + name + ':ro' for name in [
                'com.system76.CosmicTk', 'com.system76.CosmicTheme.Mode',
                'com.system76.CosmicTheme.Dark', 'com.system76.CosmicTheme.Light']]})
        self.assertIn('--offline', manifest['modules'][0]['build-commands'][0])

    def test_official_icon_sizes_and_unchanged_master(self):
        import hashlib
        for size in [32, 48, 64, 128, 256, 512]:
            path = ROOT / 'resources/icons/hicolor' / f'{size}x{size}/apps' / (APP_ID + '.png')
            data = path.read_bytes()
            self.assertEqual(data[:8], b'\x89PNG\r\n\x1a\n')
            self.assertEqual(struct.unpack('>II', data[16:24]), (size, size))
        self.assertEqual(hashlib.sha256((ROOT / 'resources/icons/hicolor/512x512/apps' / (APP_ID + '.png')).read_bytes()).hexdigest(),
                         '39a488691af1e2a732e8eb50d61ac289e40109298f6089ea942c60edef71fa4e')

    def test_outputs_use_cargo_out_dir(self):
        text = (ROOT / 'build.rs').read_text()
        self.assertIn('OUT_DIR', text)
        self.assertNotIn('target/xdgen', text)
        self.assertIn('rerun-if-changed=resources/app.metainfo.xml', text)
        self.assertNotIn("'target' / 'xdgen'", (ROOT / 'justfile').read_text())

    def test_appimage_glibc_ceiling_detection(self):
        module = load('appimage', ROOT / 'packaging/appimage/build.py')
        with patch.object(module.subprocess, 'check_output', return_value='GLIBC_2.35 GLIBC_2.17 GLIBC_PRIVATE GLIBC_2.35'):
            self.assertEqual(module.glibc_versions(Path('dummy')), [(2, 17), (2, 35)])

    def test_appimage_leaves_host_compiler_and_graphics_runtimes_unbundled(self):
        module = load('appimage_libraries', ROOT / 'packaging/appimage/build.py')
        self.assertEqual(module.BUNDLED_LIBRARIES, [('libxkbcommon.so.0', 'libxkbcommon0')])
        self.assertIn('libgcc_s.so.1', module.HOST_LIBRARIES)
        self.assertIn('libEGL.so.1', module.HOST_LIBRARIES)
        self.assertIn('libc.so.6', module.HOST_LIBRARIES)

    def test_license_checksums_and_exact_upstream_provenance(self):
        import hashlib
        folder = ROOT / 'packaging/licenses'
        for records in json.loads((folder / 'sources.json').read_text()).values():
            for record in records:
                self.assertEqual(hashlib.sha256((folder / record['file']).read_bytes()).hexdigest(), record['sha256'])
                self.assertTrue(record['url'].startswith('https://raw.githubusercontent.com/'))

    def test_missing_license_evidence_is_rejected(self):
        module = load('stage', ROOT / 'packaging/build.py')
        with tempfile.TemporaryDirectory() as root:
            path = Path(root)
            (path / '.cargo-checksum.json').write_text('{}')
            manifest = path / 'Cargo.toml'
            manifest.write_text('')
            package = {'name': 'missing-license-fixture', 'version': '1', 'id': 'fixture', 'source': None, 'manifest_path': str(manifest)}
            self.assertEqual(module.license_files(package), [])
            with self.assertRaisesRegex(RuntimeError, 'No license text found'):
                module.notices({'packages': [package]}, path / 'notices')


if __name__ == '__main__':
    unittest.main()
