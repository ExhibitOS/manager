#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Stage a new headless Manager preview and its optional validation runtime."""
import argparse
import json
import plistlib
import shutil
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--app', required=True, type=Path)
parser.add_argument('--swift-executable', required=True, type=Path)
parser.add_argument('--rust-helper', required=True, type=Path)
parser.add_argument('--validation-profile', required=True, type=Path)
parser.add_argument('--runtime-payload', type=Path)
parser.add_argument('--legal-resources', type=Path)
args = parser.parse_args()
assert args.app.is_absolute() and not args.app.exists(), 'New absolute app path required'
assert args.validation_profile.is_absolute() and not args.validation_profile.exists(), 'New isolated profile required'
assert args.swift_executable.is_file() and args.rust_helper.is_file(), 'Built executables required'
if args.legal_resources:
    assert args.legal_resources.is_absolute() and args.legal_resources.is_dir(), 'Absolute legal resources directory required'
    assert (args.legal_resources / 'index.html').is_file(), 'Legal source and notice index required'
    assert not args.app.is_relative_to(args.legal_resources), 'App must be outside legal resources'
    assert not args.legal_resources.is_symlink() and all(not p.is_symlink() and (p.is_file() or p.is_dir()) for p in args.legal_resources.rglob('*')), 'Legal resources must contain regular files and directories'
contents = args.app / 'Contents'
(contents / 'MacOS').mkdir(parents=True)
(contents / 'Helpers').mkdir()
(contents / 'Resources').mkdir()
# Keep the app bytes for the fixed launcher to prepare. A raw preview has no
# Launch Services entry and no executable main program.
shutil.copyfile(args.swift_executable, contents / 'MacOS/ExhibitOSManager')
shutil.copy2(args.rust_helper, contents / 'Helpers/exhibitos-native')
source_root = Path(__file__).resolve().parent.parent
brand_source = source_root / 'native/Resources/Brand'
brand = contents / 'Resources/Brand'
brand.mkdir()
shutil.copy2(brand_source / 'manager.icns', contents / 'Resources/manager.icns')
for name in ['manager.png', 'studio.svg', 'cms.svg', 'THIRD-PARTY-NOTICES.txt',
             'MDI-LICENSE.txt', 'Manrope-OFL.txt']:
    shutil.copy2(brand_source / name, brand / name)
# The upstream MDI notice links Apache-2.0; include the existing full project
# license beside that unmodified attribution, without a new build dependency.
shutil.copy2(source_root / 'LICENSE', brand / 'Apache-2.0.txt')
if args.legal_resources:
    shutil.copytree(args.legal_resources, contents / 'Resources/NativeRuntime-Legal')
if args.runtime_payload:
    assert args.runtime_payload.is_absolute() and args.runtime_payload.is_dir(), 'Absolute runtime payload required'
    manifest = json.loads((args.runtime_payload / 'manifest.json').read_text())
    assert manifest['mode'] == 'local-validation' and not manifest['redistributionQualified'], 'Only unqualified validation payloads belong in this preview producer'
    shutil.copytree(args.runtime_payload, contents / 'Resources/native-runtime', symlinks=True)
    # Fixed Manager-owned public provider code; the helper checks the same
    # source bytes compiled into it before any credential-bearing execution.
    hosting = contents / 'Resources/native-hosting'
    for name in ['scripts/native-hosting-runner.mjs',
                 'vendor/deployment/src/pages-render.mjs',
                 'vendor/deployment/src/pages-asset-hash.mjs',
                 'vendor/deployment/src/pages-render-worker.mjs',
                 'vendor/deployment/src/bounded-json.mjs',
                 'vendor/deployment/LICENSE',
                 'vendor/deployment/native-hosting-source.json']:
        target = hosting / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source_root / name, target)
    migration = contents / 'Resources/native-migration'
    for name in ['scripts/native-legacy-verify.mjs',
                 'scripts/native-legacy-inspect.mjs',
                 'scripts/native-legacy-apply.mjs',
                 'scripts/native-legacy-activation-check.mjs',
                 'vendor/platform/freeze-signature.mjs', 'vendor/platform/freeze-signature.ts',
                 'vendor/platform/migrations.mjs', 'vendor/platform/migrations.ts',
                 'vendor/platform/LICENSE', 'vendor/platform/native-migration-source.json',
                 'vendor/deployment/src/bounded-json.mjs',
                 'vendor/deployment/LICENSE', 'vendor/deployment/source.json']:
        target = migration / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source_root / name, target)
metadata = dict(CFBundleIdentifier='org.exhibitos.manager', CFBundleName='ExhibitOS Manager',
                CFBundleDisplayName='ExhibitOS Manager',
                CFBundlePackageType='APPL', CFBundleVersion='1', CFBundleShortVersionString='0.1.0',
                LSMinimumSystemVersion='14.0', NSHighResolutionCapable=True,
                CFBundleIconFile='manager.icns',
                ExhibitOSValidationProfile=str(args.validation_profile))
with (contents / 'Info.plist').open('wb') as f:
    plistlib.dump(metadata, f)
print(json.dumps({'app': str(args.app), 'profile': str(args.validation_profile), 'kind': 'DEVELOPMENT_PREVIEW_NOT_RELEASE_QUALIFIED'}))
