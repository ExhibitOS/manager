#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Assemble already-built native development outputs in a NEW isolated bundle.

No build, signing, installation replacement, credential access or launch. This
preview is not a qualified distribution and carries no runtime service payload.
"""
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
args = parser.parse_args()
assert args.app.is_absolute() and not args.app.exists(), 'New absolute app path required'
assert args.validation_profile.is_absolute() and not args.validation_profile.exists(), 'New isolated profile required'
assert args.swift_executable.is_file() and args.rust_helper.is_file(), 'Built executables required'
contents = args.app / 'Contents'
(contents / 'MacOS').mkdir(parents=True)
(contents / 'Helpers').mkdir()
shutil.copy2(args.swift_executable, contents / 'MacOS/ExhibitOSManager')
shutil.copy2(args.rust_helper, contents / 'Helpers/exhibitos-native')
if args.runtime_payload:
    assert args.runtime_payload.is_absolute() and args.runtime_payload.is_dir(), 'Absolute runtime payload required'
    manifest = json.loads((args.runtime_payload / 'manifest.json').read_text())
    assert manifest['mode'] == 'local-validation' and not manifest['redistributionQualified'], 'Only unqualified validation payloads belong in this preview producer'
    (contents / 'Resources').mkdir()
    shutil.copytree(args.runtime_payload, contents / 'Resources/native-runtime', symlinks=True)
metadata = dict(CFBundleIdentifier='org.exhibitos.manager', CFBundleName='ExhibitOS Manager',
                CFBundleDisplayName='ExhibitOS Manager', CFBundleExecutable='ExhibitOSManager',
                CFBundlePackageType='APPL', CFBundleVersion='1', CFBundleShortVersionString='0.1.0',
                LSMinimumSystemVersion='14.0', NSHighResolutionCapable=True,
                ExhibitOSValidationProfile=str(args.validation_profile))
with (contents / 'Info.plist').open('wb') as f:
    plistlib.dump(metadata, f)
print(json.dumps({'app': str(args.app), 'profile': str(args.validation_profile), 'kind': 'DEVELOPMENT_PREVIEW_NOT_RELEASE_QUALIFIED'}))
