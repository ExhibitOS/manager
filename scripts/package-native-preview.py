#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Stage an unsigned app or explicitly seal a private copy for local qualification."""
import argparse
import hashlib
import itertools
import json
import os
import plistlib
import shutil
import stat
import subprocess
import sys
import tempfile
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


SIGNING_LEAF = '805040A7D2F984CAEE1B7AD8378765E215FAB2E1'
SIGNING_TEAM = 'W563ZQD9G6'
APP_ID = 'org.exhibitos.manager'
HELPER_ID = 'org.exhibitos.manager.native'
APP_EXECUTABLE = 'Contents/MacOS/ExhibitOSManager'
HELPER_EXECUTABLE = 'Contents/Helpers/exhibitos-native'
RUNTIME_PATH = 'Contents/Resources/native-runtime'
SEAL_METADATA = {'Contents/_CodeSignature', 'Contents/_CodeSignature/CodeResources'}


def app_inventory(root):
    entries = {}
    for path in itertools.chain((root,), root.rglob('*')):
        name = path.relative_to(root).as_posix()
        mode = path.lstat().st_mode
        if stat.S_ISREG(mode):
            entries[name] = (mode, path.stat().st_size, digest(path))
        elif stat.S_ISDIR(mode):
            entries[name] = (mode,)
        else:
            require(stat.S_ISLNK(mode) and name.startswith(RUNTIME_PATH + '/'),
                    f'Unexpected staged entry: {name}')
            entries[name] = (mode, os.readlink(path))
    return entries


def inventory_digest(entries):
    return hashlib.sha256(json.dumps(entries, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def codesign(*arguments):
    result = subprocess.run(['/usr/bin/codesign', *arguments], text=True,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=180)
    require(result.returncode == 0,
            f'codesign {arguments[0]} exited {result.returncode}: {result.stderr.strip()}')
    return result.stdout + result.stderr


def verify_seal(path, identifier, *, bundle=False):
    requirement = (
        f'anchor apple generic and identifier "{identifier}" '
        f'and certificate leaf[subject.OU] = "{SIGNING_TEAM}" '
        'and certificate 1[field.1.2.840.113635.100.6.2.6] exists '
        'and certificate leaf[field.1.2.840.113635.100.6.1.13] exists'
    )
    codesign('--verify', '--strict', '--all-architectures',
             *(['--deep'] if bundle else []), f'-R={requirement}', path)
    report = codesign('--display', '--verbose=4', '--entitlements', ':-', path)
    lines = report.splitlines()
    require(f'Identifier={identifier}' in lines and f'TeamIdentifier={SIGNING_TEAM}' in lines
            and any(line.startswith('Authority=Developer ID Application:') for line in lines)
            and any(line.startswith('Timestamp=') and line[10:].strip() for line in lines)
            and any('flags=' in line and '(runtime)' in line for line in lines)
            and '<key>' not in report, f'Unexpected signature metadata: {identifier}')
    with tempfile.TemporaryDirectory(prefix='exhibitos-seal-cert-') as directory:
        prefix = Path(directory) / 'cert'
        codesign('--display', f'--extract-certificates={prefix}', path)
        leaf = hashlib.sha1(prefix.with_name('cert0').read_bytes()).hexdigest().upper()
        require(leaf == SIGNING_LEAF, f'Unexpected signing certificate: {identifier}')


def seal_release(argv):
    parser = argparse.ArgumentParser(description='Seal a private copy of a staged Swift release app')
    parser.add_argument('--staged-app', required=True, type=Path)
    parser.add_argument('--sealed-app', required=True, type=Path)
    parser.add_argument('--assembly-receipt', required=True, type=Path)
    parser.add_argument('--assembly-receipt-sha256', required=True)
    parser.add_argument('--staged-inventory-sha256', required=True)
    parser.add_argument('--legal-inventory', required=True, type=Path)
    args = parser.parse_args(argv)
    source, target = args.staged_app, args.sealed_app
    require(source.is_absolute() and source.resolve(strict=True) == source
            and source.is_dir() and source.suffix == '.app', 'Expected a canonical staged app')
    require(target.is_absolute() and target.suffix == '.app' and not target.is_relative_to(source)
            and target.parent.resolve(strict=True) == target.parent and target.parent.is_dir()
            and not target.exists() and not target.is_symlink(), 'Expected a fresh canonical output app')
    parent = target.parent.stat()
    require(parent.st_uid == os.getuid() and not parent.st_mode & 0o077,
            'Seal output parent must be private and owned by the current user')
    assembly = read_pinned_json(args.assembly_receipt, args.assembly_receipt_sha256, 65_536)
    require(assembly['kind'] == 'RELEASE_ASSEMBLY_PENDING_APP_SEAL' and assembly['app'] == str(source),
            'Assembly receipt does not identify this stage')
    inputs = assembly['inputs']
    require(set(inputs) == {'version', 'managerCommit', 'managerTree', 'appSHA256', 'helperSHA256',
                            'runtimeManifestSHA256', 'legalInventorySHA256'} and inputs['version'] == 1,
            'Assembly build receipt fields are invalid')
    metadata = plistlib.loads((source / 'Contents/Info.plist').read_bytes())
    require(metadata.get('CFBundleIdentifier') == APP_ID
            and metadata.get('CFBundleExecutable') == Path(APP_EXECUTABLE).name
            and 'ExhibitOSValidationProfile' not in metadata, 'Unexpected Swift app identity')
    runtime = source / RUNTIME_PATH
    manifest = read_pinned_json(runtime / 'manifest.json', inputs['runtimeManifestSHA256'], 16 * 1024 ** 2)
    require(manifest.get('format') == 'exhibitos-native-runtime-v1' and manifest.get('platform') == 'darwin-arm64'
            and manifest.get('mode') == 'release' and manifest.get('officialSigned') is True
            and manifest.get('redistributionQualified') is True, 'Finalized release runtime required')
    verify_release_tree(runtime, manifest['files'], runtime=True)
    legal = read_pinned_json(args.legal_inventory, inputs['legalInventorySHA256'], 2 * 1024 ** 2)
    verify_release_tree(source / 'Contents/Resources/NativeRuntime-Legal', legal['files'])
    before = app_inventory(source)
    require(inventory_digest(before) == args.staged_inventory_sha256
            and assembly.get('stagedInventorySHA256', args.staged_inventory_sha256) == args.staged_inventory_sha256,
            'Staged app differs from the pinned assembly inventory')
    executable_fields = {APP_EXECUTABLE: 'appSHA256', HELPER_EXECUTABLE: 'helperSHA256'}
    for path, field in executable_fields.items():
        mode, _, sha = before[path]
        require(stat.S_ISREG(mode) and mode & 0o111 and sha == inputs[field],
                f'Staged executable does not match build receipt: {path}')
    staged_bytes = sum(entry[1] for entry in before.values() if stat.S_ISREG(entry[0]))
    require('Contents/_CodeSignature' not in before and staged_bytes == assembly['stagedBytes']
            and 0 < staged_bytes <= RELEASE_STAGE_MAX_BYTES
            and shutil.disk_usage(target.parent).free > staged_bytes + 6 * 1024 ** 3,
            'Staged app size, existing seal or free space prevents sealing')

    shutil.copytree(source, target, symlinks=True)
    require(app_inventory(target) == before, 'Private copy differs from staged app')
    for path, identifier in ((target / HELPER_EXECUTABLE, HELPER_ID), (target, APP_ID)):
        codesign('--force', '--sign', SIGNING_LEAF, '--timestamp', '--options', 'runtime',
                 '--identifier', identifier, path)
    verify_seal(target / HELPER_EXECUTABLE, HELPER_ID)
    verify_seal(target, APP_ID, bundle=True)

    after = app_inventory(target)
    require(set(after) == set(before) | SEAL_METADATA
            and stat.S_ISDIR(after['Contents/_CodeSignature'][0])
            and stat.S_ISREG(after['Contents/_CodeSignature/CodeResources'][0]),
            'Unexpected signed app paths')
    for path, entry in before.items():
        if path in executable_fields:
            require(after[path][0] == entry[0], f'Signing changed executable mode: {path}')
        else:
            require(after[path] == entry, f'Signing changed staged resource: {path}')
    require(app_inventory(source) == before, 'Original stage changed during sealing')
    print(json.dumps({
        'kind': 'RELEASE_APP_SEALED_FOR_LOCAL_QUALIFICATION', 'app': str(target),
        'assemblyReceiptSHA256': args.assembly_receipt_sha256,
        'stagedInventorySHA256': args.staged_inventory_sha256, 'inputs': inputs,
        'certificateSHA1': SIGNING_LEAF, 'teamIdentifier': SIGNING_TEAM,
        'stagedBytes': staged_bytes,
        'sealedBytes': sum(entry[1] for entry in after.values() if stat.S_ISREG(entry[0])),
        'appSHA256': after[APP_EXECUTABLE][2], 'helperSHA256': after[HELPER_EXECUTABLE][2],
        'codeResourcesSHA256': after['Contents/_CodeSignature/CodeResources'][2],
    }))


if sys.argv[1:2] == ['seal-release']:
    seal_release(sys.argv[2:])
    raise SystemExit(0)


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
    migration = contents / 'Resources/native-migration'
    hosting = migration if release else contents / 'Resources/native-hosting'
    hosting_files = ['scripts/native-hosting-runner.mjs',
                     'vendor/deployment/src/pages-render.mjs',
                     'vendor/deployment/src/pages-asset-hash.mjs',
                     'vendor/deployment/src/pages-render-worker.mjs',
                     'vendor/deployment/native-hosting-source.json']
    # The release layout shares these two files with migration. Preview keeps
    # the older separate hosting directory for existing validation helpers.
    if not release:
        hosting_files += ['vendor/deployment/src/bounded-json.mjs', 'vendor/deployment/LICENSE']
    for name in hosting_files:
        target = hosting / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source_root / name, target)
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
                      'stageLimitBytes': RELEASE_STAGE_MAX_BYTES,
                      'stagedInventorySHA256': inventory_digest(app_inventory(args.app))}))
else:
    print(json.dumps({'app': str(args.app), 'profile': str(args.validation_profile), 'kind': 'DEVELOPMENT_PREVIEW_NOT_RELEASE_QUALIFIED'}))
