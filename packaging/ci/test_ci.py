#!/usr/bin/env python3
"""Focused CI policy and artifact-validator regressions; no network or GUI."""
import importlib.util
import hashlib
import io
import json
from pathlib import Path
import re
import struct
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('verify_appimage', Path(__file__).with_name('verify-appimage.py'))
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)


class CITests(unittest.TestCase):
    def test_valid_x86_64_type2_header(self):
        header = bytearray(20)
        header[:6] = b'\x7fELF\x02\x01'
        header[8:11] = b'AI\x02'
        header[18:20] = struct.pack('<H', 62)
        verify.check_header(header)
        for field, value in [(0, 0), (4, 1), (5, 2), (10, 1), (18, 183)]:
            invalid = header.copy()
            invalid[field] = value
            with self.assertRaises(RuntimeError):
                verify.check_header(invalid)
        with self.assertRaises(RuntimeError):
            verify.check_header(b'')

    def test_glibc_policy_uses_numeric_versions(self):
        verify.check_versions([], 'runtime')
        verify.check_versions([(2, 9), (2, 17), (2, 35)], 'payload')
        for versions in [[(2, 36)], [(2, 9), (2, 40)], [(3, 0)]]:
            with self.assertRaises(RuntimeError):
                verify.check_versions(versions, 'payload')

    def test_downloads_have_exact_versions_and_hashes(self):
        lock = json.loads(Path(__file__).with_name('tools.lock.json').read_text())
        self.assertEqual(lock['rust_version'], '1.99.1')
        self.assertEqual(len(lock['rust_components']), 5)
        for record in [*lock['rust_components'], lock['actionlint']]:
            self.assertRegex(record['sha256'], r'^[0-9a-f]{64}$')
            self.assertTrue(record['url'].startswith('https://'))
            self.assertNotIn('/latest/', record['url'])
        dockerfile = Path(__file__).with_name('Containerfile').read_text()
        self.assertRegex(dockerfile, r'FROM .*ubuntu:22\.04@sha256:[0-9a-f]{64}')
        self.assertEqual(dockerfile.count('20261020T000000Z'), 4)
        self.assertNotIn('trusted=yes', dockerfile)
        for setting in ['CARGO_PROFILE_DEV_DEBUG=0', 'CARGO_PROFILE_TEST_DEBUG=0', 'CARGO_INCREMENTAL=0']:
            self.assertIn(setting, dockerfile)

    def test_corrupt_tool_download_is_rejected(self):
        spec = importlib.util.spec_from_file_location('install_tools', Path(__file__).with_name('install-tools.py'))
        installer = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(installer)
        content = b'fixture archive bytes'
        record = {'url': 'https://example.invalid/tool.tar.xz', 'sha256': '0' * 64}
        with tempfile.TemporaryDirectory() as folder:
            with patch.object(installer.urllib.request, 'urlopen', return_value=io.BytesIO(content)):
                with self.assertRaisesRegex(RuntimeError, 'checksum mismatch'):
                    installer.fetch(record, Path(folder))
            record['sha256'] = hashlib.sha256(content).hexdigest()
            with patch.object(installer.urllib.request, 'urlopen', return_value=io.BytesIO(content)):
                self.assertEqual(installer.fetch(record, Path(folder)).read_bytes(), content)

    def test_workflow_is_testing_only_and_actions_are_sha_pinned(self):
        text = (ROOT / '.github/workflows/appimage.yml').read_text()
        self.assertIn('contents: read', text)
        self.assertIn('persist-credentials: false', text)
        self.assertIn('--user "$(id -u):$(id -g)"', text)
        self.assertIn('retention-days: 7', text)
        self.assertIn('if-no-files-found: error', text)
        self.assertEqual(len(re.findall(r'uses: [^\s]+@[0-9a-f]{40}(?: |\n)', text)), 2)
        for forbidden in ['pull_request_target:', 'contents: write', 'secrets.', 'tags:', 'gh release', '/home/rapper', '--privileged']:
            self.assertNotIn(forbidden, text)
        script = Path(__file__).with_name('build-appimage.sh').read_text()
        for required in ['bash packaging/validate.sh', 'packaging/build.py', 'packaging/appimage/build.py', 'sha256sum --check']:
            self.assertIn(required, script)


if __name__ == '__main__':
    unittest.main()
