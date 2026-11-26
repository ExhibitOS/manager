#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
import argparse
import contextlib
import importlib.util
import io
import json
import os
import plistlib
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

SOURCE = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location('preview', SOURCE / 'scripts/package-native-preview.py')
preview = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(preview)


class LocalInstalledCLITests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        run = self.root / 'isolated'
        run.mkdir(mode=0o700)
        runtime = self.root / 'runtime'
        runtime.mkdir()
        (runtime / 'regular').write_bytes(b'local runtime')
        (runtime / 'link').symlink_to('regular')
        self.manifest = {
            'format': 'exhibitos-native-runtime-v1', 'platform': 'darwin-arm64',
            'mode': 'local-validation', 'redistributionQualified': False, 'officialSigned': False,
            'files': [
                {'path': 'regular', 'bytes': 13, 'executable': False, 'sha256': preview.digest(runtime / 'regular')},
                {'path': 'link', 'link': 'regular'},
            ],
        }
        gui, helper = self.root / 'cli', self.root / 'helper'
        gui.write_bytes(preview.LOCAL_CLI_MARKER.encode('ascii'))
        helper.write_bytes(b'debug helper')
        for path in (gui, helper):
            path.chmod(0o700)
        self.args = argparse.Namespace(
            app=run / preview.LOCAL_CLI_APP, swift_executable=gui, rust_helper=helper,
            runtime_payload=runtime, legal_resources=None, legal_inventory=None,
            local_cli_receipt=self.root / 'build.json',
        )
        self.write_pins()
        self.calls = []
        self.report_extra = ''
        self.stack = [
            patch.object(preview.sys, 'platform', 'darwin'),
            patch.object(preview.shutil, 'disk_usage', return_value=argparse.Namespace(free=2**35)),
            patch.object(preview.subprocess, 'check_output', side_effect=lambda args, **_: {
                'HEAD': 'commit', 'HEAD^{tree}': 'tree', '--porcelain': '',
            }[args[-1]]),
            patch.object(preview, 'codesign', side_effect=self.fake_codesign),
        ]
        for mock in self.stack:
            mock.start()
            self.addCleanup(mock.stop)

    def write_pins(self):
        manifest = self.args.runtime_payload / 'manifest.json'
        manifest.write_text(json.dumps(self.manifest))
        receipt = {
            'version': 1, 'managerCommit': 'commit', 'managerTree': 'tree',
            'appSHA256': preview.digest(self.args.swift_executable), 'helperSHA256': preview.digest(self.args.rust_helper),
            'runtimeManifestSHA256': preview.digest(manifest),
        }
        self.args.local_cli_receipt.write_text(json.dumps(receipt))
        self.args.local_cli_receipt_sha256 = preview.digest(self.args.local_cli_receipt)

    def fake_codesign(self, *args):
        self.calls.append(args)
        item = args[-1]
        if args[0] == '--force' and item.suffix == '.app':
            sealed = item / 'Contents/_CodeSignature'
            sealed.mkdir()
            (sealed / 'CodeResources').write_text('fixture seal')
        if args[0] == '--display':
            identifier = preview.APP_ID if item.suffix == '.app' else preview.HELPER_ID
            return f'Identifier={identifier}\nSignature=adhoc\nTeamIdentifier=not set\n{self.report_extra}'
        return ''

    def test_local_assembly_binds_one_run_and_verifies_ad_hoc_without_entitlements(self):
        result = preview.assemble_local_cli(self.args, SOURCE)
        app = self.args.app
        info = plistlib.loads((app / 'Contents/Info.plist').read_bytes())
        self.assertEqual(info['CFBundleIdentifier'], preview.APP_ID)
        self.assertEqual(info['CFBundleExecutable'], 'ExhibitOSManager')
        self.assertEqual(info['ExhibitOSLocalInstalledCLI'], {
            'version': preview.LOCAL_CLI_MARKER, 'root': str(app.parent),
            'runtimeManifestSHA256': result['inputs']['runtimeManifestSHA256'],
        })
        self.assertTrue(os.access(app / preview.APP_EXECUTABLE, os.X_OK))
        self.assertTrue((app / preview.RUNTIME_PATH / 'link').is_symlink())
        self.assertFalse((app / 'Contents/Resources/native-hosting').exists())
        self.assertFalse((app / 'Contents/Resources/native-migration').exists())
        self.assertEqual(set(app.parent.iterdir()), {app})
        self.assertEqual(result['kind'], 'LOCAL_INSTALLED_CLI_NOT_RELEASE_QUALIFIED')
        sign_calls = [call for call in self.calls if call[0] == '--force']
        self.assertEqual([call[2:6] for call in sign_calls], [
            ('-', '--timestamp=none', '--identifier', preview.HELPER_ID),
            ('-', '--timestamp=none', '--identifier', preview.APP_ID),
        ])
        self.assertTrue(any(call[0] == '--verify' and '--strict' in call and '--deep' in call for call in self.calls))

    def test_refuses_changed_or_official_runtime_before_signing(self):
        self.manifest['officialSigned'] = True
        self.write_pins()
        with self.assertRaisesRegex(ValueError, 'nonofficial'):
            preview.assemble_local_cli(self.args, SOURCE)
        self.manifest['officialSigned'] = False
        self.write_pins()
        (self.args.runtime_payload / 'regular').write_bytes(b'modified')
        with self.assertRaisesRegex(ValueError, 'file mismatch'):
            preview.assemble_local_cli(self.args, SOURCE)
        self.assertFalse(self.calls)
        self.assertFalse(self.args.app.exists())

    def test_refuses_ordinary_executable_and_occupied_or_install_output(self):
        self.args.swift_executable.write_bytes(b'ordinary executable')
        self.write_pins()
        with self.assertRaisesRegex(ValueError, 'local-only Manager debug executable'):
            preview.assemble_local_cli(self.args, SOURCE)
        (self.args.app.parent / 'profile').mkdir()
        with self.assertRaisesRegex(ValueError, 'new empty private run directory'):
            preview.assemble_local_cli(self.args, SOURCE)
        install = self.root / 'Applications' / 'new'
        install.mkdir(parents=True)
        self.args.app = install / preview.LOCAL_CLI_APP
        with self.assertRaisesRegex(ValueError, 'new empty private run directory'):
            preview.assemble_local_cli(self.args, SOURCE)
        self.assertFalse(self.calls)

    def test_signature_mismatch_does_not_publish_an_executable(self):
        self.report_extra = '<key>keychain-access-groups</key>'
        with self.assertRaisesRegex(ValueError, 'signature or entitlements'):
            preview.assemble_local_cli(self.args, SOURCE)
        self.assertEqual(list(self.args.app.parent.iterdir()), [])

    def test_preview_remains_headless_and_release_refuses_local_executable(self):
        self.args.validation_profile = self.root / 'unused-preview-profile'
        preview.assemble(self.args, SOURCE)
        info = plistlib.loads((self.args.app / 'Contents/Info.plist').read_bytes())
        self.assertNotIn('CFBundleExecutable', info)
        self.assertNotIn('ExhibitOSLocalInstalledCLI', info)
        self.assertFalse(os.access(self.args.app / preview.APP_EXECUTABLE, os.X_OK))
        self.args.app = self.root / 'release.app'
        self.args.legal_resources = self.root / 'legal'
        self.args.legal_inventory = self.root / 'legal.json'
        self.args.release_receipt = self.root / 'release.json'
        self.args.legal_inventory.write_text('{}')
        build = json.loads(self.args.local_cli_receipt.read_text())
        build['legalInventorySHA256'] = '0' * 64
        self.args.release_receipt.write_text(json.dumps(build))
        with self.assertRaisesRegex(ValueError, 'cannot be assembled for release'):
            preview.release_inputs(self.args, SOURCE)
        self.assertFalse(self.args.app.exists())
        self.assertFalse(self.calls)

    def test_ordinary_release_still_assembles_only_with_finalized_runtime_and_legal_tree(self):
        self.args.swift_executable.write_bytes(b'ordinary executable')
        self.manifest.update(mode='release', redistributionQualified=True, officialSigned=True)
        self.write_pins()
        legal = self.root / 'legal'
        legal.mkdir()
        index = legal / 'index.html'
        index.write_text('fixture source notices')
        inventory = self.root / 'legal.json'
        inventory.write_text(json.dumps({'files': [
            {'path': 'index.html', 'bytes': index.stat().st_size, 'sha256': preview.digest(index)},
        ]}))
        release = self.root / 'release.json'
        build = json.loads(self.args.local_cli_receipt.read_text())
        build['legalInventorySHA256'] = preview.digest(inventory)
        release.write_text(json.dumps(build))
        app = self.root / 'release.app'
        argv = ['package-native-preview.py', '--release-receipt', str(release), '--app', str(app),
                '--swift-executable', str(self.args.swift_executable), '--rust-helper', str(self.args.rust_helper),
                '--runtime-payload', str(self.args.runtime_payload), '--legal-resources', str(legal),
                '--legal-inventory', str(inventory)]
        with patch.object(preview.sys, 'argv', argv), contextlib.redirect_stdout(io.StringIO()) as output:
            preview.main()
        info = plistlib.loads((app / 'Contents/Info.plist').read_bytes())
        self.assertNotIn('ExhibitOSLocalInstalledCLI', info)
        self.assertEqual(info['CFBundleExecutable'], 'ExhibitOSManager')
        self.assertEqual(json.loads(output.getvalue())['kind'], 'RELEASE_ASSEMBLY_PENDING_APP_SEAL')
        self.assertFalse(self.calls)


if __name__ == '__main__':
    unittest.main()
