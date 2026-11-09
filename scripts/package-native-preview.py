#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Stage a headless validation preview or verified release inputs for sealing.

Assembly never signs, installs or runs the app.
"""
import argparse
import hashlib
import json
import os
import plistlib
import shutil
import stat
import subprocess
from pathlib import Path

RELEASE_STAGE_MAX_BYTES = 1280 * 1024 ** 2


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def read_pinned_json(path, sha256, limit):
    require(path.is_file() and not path.is_symlink() and path.stat().st_size <= limit,
            'Release input must be a bounded regular file')
    data = path.read_bytes()
    require(hashlib.sha256(data).hexdigest() == sha256, 'Release input digest changed')
    return json.loads(data)


def verify_release_tree(root, rows, *, runtime=False):
    require(root.is_absolute() and root.resolve(strict=True) == root and root.is_dir(),
            'Release tree root must be canonical')
    names = [row['path'] for row in rows]
    require(0 < len(names) <= 50_000 and len(names) == len(set(names)),
            'Release tree inventory is invalid')
    expected = dict(zip(names, rows))
    if runtime:
        require('manifest.json' not in expected, 'Runtime manifest must be outside its file inventory')
    else:
        require('index.html' in expected, 'Release source index missing')
    actual = set()
    for path in root.rglob('*'):
        mode = path.lstat().st_mode
        if stat.S_ISDIR(mode):
            continue
        name = path.relative_to(root).as_posix()
        if runtime and name == 'manifest.json':
            continue
        row = expected.get(name)
        require(row is not None, f'Unexpected release file: {name}')
        actual.add(name)
        if stat.S_ISLNK(mode):
            target = os.readlink(path)
            require(runtime and not Path(target).is_absolute() and target == row.get('link')
                    and path.resolve(strict=True).is_relative_to(root), f'Invalid runtime link: {name}')
        else:
            require(stat.S_ISREG(mode) and 'link' not in row and path.stat().st_size == row['bytes']
                    and (not runtime or bool(mode & 0o111) == row['executable'])
                    and digest(path) == row['sha256'], f'Release file mismatch: {name}')
    require(actual == set(expected), 'Release tree inventory is incomplete')


def release_inputs(args, source_root):
    receipt_path = args.release_receipt
    require(receipt_path.is_absolute() and receipt_path.resolve(strict=True) == receipt_path
            and receipt_path.is_file() and receipt_path.stat().st_size <= 65_536,
            'Release receipt must be a bounded canonical file')
    receipt = json.loads(receipt_path.read_bytes())
    require(set(receipt) == {'version', 'managerCommit', 'managerTree', 'appSHA256', 'helperSHA256',
                             'runtimeManifestSHA256', 'legalInventorySHA256'} and receipt['version'] == 1,
            'Release receipt fields are invalid')
    require(args.runtime_payload and args.legal_resources and args.legal_inventory,
            'Release assembly requires the runtime, legal resources and external legal inventory')
    require(args.legal_inventory.is_absolute() and args.legal_inventory.resolve(strict=True) == args.legal_inventory
            and not args.legal_inventory.is_relative_to(args.legal_resources),
            'Release legal inventory must be outside the copied resources')
    require(args.app.is_absolute() and args.app.suffix == '.app'
            and args.app.parent.resolve(strict=True) == args.app.parent
            and not args.app.exists() and not args.app.is_symlink()
            and not any(args.app.is_relative_to(root) for root in (args.runtime_payload, args.legal_resources)),
            'Release output must be a new canonical app path outside its inputs')
    git = lambda *argv: subprocess.check_output(['git', '-C', str(source_root), *argv], text=True).strip()
    require(git('rev-parse', 'HEAD') == receipt['managerCommit']
            and git('rev-parse', 'HEAD^{tree}') == receipt['managerTree']
            and not git('status', '--porcelain'), 'Manager checkout does not match release build receipt')
    for binary, field in [(args.swift_executable, 'appSHA256'), (args.rust_helper, 'helperSHA256')]:
        require(binary.is_absolute() and binary.resolve(strict=True) == binary and binary.is_file()
                and binary.stat().st_mode & 0o111 and digest(binary) == receipt[field],
                'Manager executable does not match release build receipt')
    manifest = read_pinned_json(args.runtime_payload / 'manifest.json', receipt['runtimeManifestSHA256'], 16 * 1024 ** 2)
    require(manifest.get('format') == 'exhibitos-native-runtime-v1' and manifest.get('platform') == 'darwin-arm64'
            and manifest.get('mode') == 'release' and manifest.get('redistributionQualified') is True
            and manifest.get('officialSigned') is True, 'Finalized release runtime required')
    legal = read_pinned_json(args.legal_inventory, receipt['legalInventorySHA256'], 2 * 1024 ** 2)
    size = sum(row.get('bytes', 0) for row in manifest['files'] + legal['files'])
    size += sum(binary.stat().st_size for binary in (args.swift_executable, args.rust_helper))
    require(0 < size <= RELEASE_STAGE_MAX_BYTES and shutil.disk_usage(args.app.parent).free > size + 64 * 1024 ** 2,
            'Insufficient reserved space for release assembly')
    verify_release_tree(args.runtime_payload, manifest['files'], runtime=True)
    verify_release_tree(args.legal_resources, legal['files'])
    return receipt, manifest, legal, size


parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--app', required=True, type=Path)
parser.add_argument('--swift-executable', required=True, type=Path)
parser.add_argument('--rust-helper', required=True, type=Path)
mode = parser.add_mutually_exclusive_group(required=True)
mode.add_argument('--validation-profile', type=Path)
mode.add_argument('--release-receipt', type=Path)
parser.add_argument('--legal-inventory', type=Path)
parser.add_argument('--runtime-payload', type=Path)
parser.add_argument('--legal-resources', type=Path)
args = parser.parse_args()
assert args.app.is_absolute() and not args.app.exists(), 'New absolute app path required'
source_root = Path(__file__).resolve().parent.parent
release = release_inputs(args, source_root) if args.release_receipt else None
if not release:
    assert args.validation_profile.is_absolute() and not args.validation_profile.exists(), 'New isolated profile required'
    assert args.legal_inventory is None, 'Legal inventory is a release input'
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
gui = contents / 'MacOS/ExhibitOSManager'
if release:
    shutil.copy2(args.swift_executable, gui)
else:
    # The fixed launcher adds the app entry and executable mode during preparation.
    shutil.copyfile(args.swift_executable, gui)
shutil.copy2(args.rust_helper, contents / 'Helpers/exhibitos-native')
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
    if not release:
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
                CFBundleIconFile='manager.icns')
if release:
    metadata['CFBundleExecutable'] = 'ExhibitOSManager'
else:
    metadata['ExhibitOSValidationProfile'] = str(args.validation_profile)
with (contents / 'Info.plist').open('wb') as f:
    plistlib.dump(metadata, f)
if release:
    receipt, manifest, legal, size = release
    resources = contents / 'Resources'
    read_pinned_json(resources / 'native-runtime/manifest.json', receipt['runtimeManifestSHA256'], 16 * 1024 ** 2)
    verify_release_tree(resources / 'native-runtime', manifest['files'], runtime=True)
    verify_release_tree(resources / 'NativeRuntime-Legal', legal['files'])
    require(digest(contents / 'MacOS/ExhibitOSManager') == receipt['appSHA256']
            and digest(contents / 'Helpers/exhibitos-native') == receipt['helperSHA256'],
            'Assembled Manager executable changed')
    staged_size = sum(file.stat().st_size for file in args.app.rglob('*')
                      if not file.is_symlink() and file.is_file())
    require(staged_size <= RELEASE_STAGE_MAX_BYTES, 'Assembled release exceeds the stage limit')
    print(json.dumps({'app': str(args.app), 'kind': 'RELEASE_ASSEMBLY_PENDING_APP_SEAL',
                      'inputs': receipt, 'inputBytes': size, 'stagedBytes': staged_size,
                      'stageLimitBytes': RELEASE_STAGE_MAX_BYTES}))
else:
    print(json.dumps({'app': str(args.app), 'profile': str(args.validation_profile), 'kind': 'DEVELOPMENT_PREVIEW_NOT_RELEASE_QUALIFIED'}))
