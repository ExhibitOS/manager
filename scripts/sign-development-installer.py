#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Sign an existing reviewed macOS development App copy and its private DMG."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import shutil
import signal
import stat
import subprocess
import sys
import time

CAP = 1024 ** 3
RESERVE = 192 * 1024 ** 2
FLOOR = 6 * 1024 ** 3
CHECK = None


def require(condition, code):
    if not condition:
        raise ValueError(code)


def digest(path):
    result = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(4 * 1024 ** 2), b''):
            if CHECK:
                CHECK()
            result.update(block)
    return result.hexdigest()


def parents(path):
    for item in (path, *path.parents):
        require(not item.is_symlink() and item.is_dir(), 'DEVELOPMENT_PARENT_UNSAFE')


def inventory(root):
    parents(root)
    result = {}
    for item in sorted(root.rglob('*')):
        metadata = item.lstat()
        require(not stat.S_ISLNK(metadata.st_mode), 'DEVELOPMENT_APP_LINK_REFUSED')
        if stat.S_ISREG(metadata.st_mode):
            require(metadata.st_nlink == 1, 'DEVELOPMENT_APP_LINK_REFUSED')
            result[str(item.relative_to(root))] = {
                'sha256': digest(item), 'bytes': metadata.st_size,
                'mode': stat.S_IMODE(metadata.st_mode)}
        else:
            require(stat.S_ISDIR(metadata.st_mode), 'DEVELOPMENT_APP_TYPE_INVALID')
    return result


def source(repo, commit):
    options = {'cwd': repo, 'capture_output': True, 'text': True, 'timeout': 10}
    head = subprocess.run(['git', 'rev-parse', 'HEAD'], **options)
    tree = subprocess.run(['git', 'rev-parse', 'HEAD^{tree}'], **options)
    dirty = subprocess.run(['git', 'status', '--porcelain=v1', '--untracked-files=all'], **options)
    require(head.returncode == tree.returncode == dirty.returncode == 0,
            'DEVELOPMENT_SOURCE_INVALID')
    require(head.stdout.strip() == commit and not dirty.stdout.strip(),
            'DEVELOPMENT_SOURCE_CHANGED_OR_DIRTY')
    return {'commit': commit, 'tree': tree.stdout.strip()}


def build_binding(repo, pipeline_commit, build_commit):
    exclusions = ['docs/**', 'README.md', 'AGENTS.md', 'scripts/sign-development-installer.py']
    def blobs(commit):
        result = subprocess.run(['git', 'ls-tree', '-r', '-z', commit], cwd=repo,
                                capture_output=True, timeout=10)
        require(result.returncode == 0 and len(result.stdout) <= 4 * 1024 ** 2,
                'DEVELOPMENT_BUILD_SOURCE_INVALID')
        values = {}
        for entry in result.stdout.split(b'\0'):
            if not entry:
                continue
            metadata, name = entry.split(b'\t', 1)
            path = name.decode('utf-8')
            if path.startswith('docs/') or path in exclusions[1:]:
                continue
            values[path] = metadata.decode('ascii')
        require(values, 'DEVELOPMENT_BUILD_SOURCE_EMPTY')
        return values
    original, current = blobs(build_commit), blobs(pipeline_commit)
    require(original == current, 'DEVELOPMENT_PRODUCT_BUILD_INPUTS_CHANGED')
    return {'buildSourceCommit': build_commit, 'pipelineSourceCommit': pipeline_commit,
            'comparedTrackedFiles': len(original), 'excludedDocumentationAndTool': exclusions,
            'trackedBuildInputsSha256': hashlib.sha256(json.dumps(original, sort_keys=True,
                separators=(',', ':')).encode()).hexdigest()}


def main():
    global CHECK
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', required=True, type=Path)
    parser.add_argument('--inventory', required=True, type=Path)
    parser.add_argument('--inventory-sha256', required=True)
    parser.add_argument('--source-commit', required=True, help='Clean pipeline checkout HEAD')
    parser.add_argument('--build-source-commit', required=True, help='Original reviewed App build commit')
    parser.add_argument('--identity-sha1', required=True)
    parser.add_argument('--output', required=True, type=Path,
                        help='Fresh absolute directory outside the source checkout')
    parser.add_argument('--execute-development-signing', action='store_true')
    args = parser.parse_args()
    require(args.execute_development_signing, 'DEVELOPMENT_EXPLICIT_EXECUTION_REQUIRED')
    require(sys.platform == 'darwin', 'DEVELOPMENT_HOST_UNSUPPORTED')
    require(re.fullmatch(r'[a-f0-9]{40}', args.source_commit) and
            re.fullmatch(r'[a-f0-9]{40}', args.build_source_commit), 'DEVELOPMENT_COMMIT_INVALID')
    require(re.fullmatch(r'[A-F0-9]{40}', args.identity_sha1), 'DEVELOPMENT_IDENTITY_INVALID')
    require(re.fullmatch(r'[a-f0-9]{64}', args.inventory_sha256), 'DEVELOPMENT_INVENTORY_HASH_INVALID')
    repo = Path(__file__).resolve().parent.parent
    for item in (args.app, args.inventory, args.output):
        require(item.is_absolute() and '..' not in item.parts, 'DEVELOPMENT_ABSOLUTE_PATH_REQUIRED')
    parents(args.output.parent)
    parents(args.inventory.parent)
    require(not args.output.exists() and not args.output.is_symlink(), 'DEVELOPMENT_OUTPUT_EXISTS')
    require(repo not in args.output.parents and args.app not in args.output.parents,
            'DEVELOPMENT_OUTPUT_SCOPE_INVALID')
    metadata = args.inventory.lstat()
    require(stat.S_ISREG(metadata.st_mode) and metadata.st_nlink == 1 and
            metadata.st_size <= 4 * 1024 ** 2, 'DEVELOPMENT_INVENTORY_UNSAFE')
    require(digest(args.inventory) == args.inventory_sha256, 'DEVELOPMENT_INPUT_CHANGED')
    expected = json.loads(args.inventory.read_bytes())
    before = inventory(args.app)
    require(expected == before and before and sum(v['bytes'] for v in before.values()) <= 180 * 1024 ** 2,
            'DEVELOPMENT_APP_INPUT_MISMATCH_OR_TOO_LARGE')
    info = plistlib.loads((args.app / 'Contents/Info.plist').read_bytes())
    require(info.get('CFBundleIdentifier') == 'org.exhibitos.manager', 'DEVELOPMENT_APP_IDENTIFIER_INVALID')
    version = info.get('CFBundleShortVersionString')
    require(isinstance(version, str) and re.fullmatch(r'\d+\.\d+\.\d+', version), 'DEVELOPMENT_APP_VERSION_INVALID')
    pinned_source = source(repo, args.source_commit)
    binding = build_binding(repo, args.source_commit, args.build_source_commit)
    identity = subprocess.run(['/usr/bin/security', 'find-identity', '-v', '-p', 'codesigning'],
                              capture_output=True, text=True, timeout=10)
    require(identity.returncode == 0 and len(identity.stdout) <= 65536 and
            re.search(r'^\s*\d+\) ' + args.identity_sha1 + r' "Apple Development:[^\n]+"$',
                      identity.stdout, re.MULTILINE), 'DEVELOPMENT_IDENTITY_UNAVAILABLE_OR_WRONG_CLASS')
    initial = shutil.disk_usage(args.output.parent).free
    require(initial >= CAP + RESERVE + FLOOR, 'DEVELOPMENT_SPACE_ADMISSION_FAILED')
    args.output.mkdir(mode=0o700)
    stage = args.output / 'staging'
    stage.mkdir(mode=0o700)
    mount = args.output / 'mounted'
    mount.mkdir(mode=0o700)
    temporary = args.output / 'temporary'
    temporary.mkdir(mode=0o700)
    started = time.monotonic()
    peak = 0
    attached = False
    device = None
    report = {'format': 1, 'state': 'RUNNING', 'distribution': 'apple-development-local',
              'source': pinned_source, 'appBuildBinding': binding, 'inputInventorySha256': args.inventory_sha256,
              'identitySha1': args.identity_sha1, 'version': version,
              'productionSigned': False, 'notarized': False, 'gatekeeperQualified': False,
              'nativeGUIExecuted': False, 'privateKeyExported': False, 'commands': []}

    def check():
        nonlocal peak
        free = shutil.disk_usage(args.output).free
        peak = max(peak, initial - free)
        require(free >= FLOOR and peak <= CAP, 'DEVELOPMENT_SPACE_BOUND_EXCEEDED')
        require(time.monotonic() - started < 270, 'DEVELOPMENT_WORK_DEADLINE')

    CHECK = check

    def command(arguments, label, cleanup=False):
        if not cleanup:
            check()
        stdout, stderr = args.output / (label + '.out'), args.output / (label + '.err')
        environment = dict(os.environ, TMPDIR=str(temporary))
        with stdout.open('xb') as out, stderr.open('xb') as err:
            process = subprocess.Popen(arguments, stdout=out, stderr=err,
                                       env=environment, start_new_session=True)
            child_start = time.monotonic()
            while process.poll() is None:
                try:
                    if cleanup:
                        require(time.monotonic() - child_start < 25, 'DEVELOPMENT_CLEANUP_DEADLINE')
                    else:
                        check()
                    require(stdout.stat().st_size + stderr.stat().st_size <= 1024 ** 2,
                            'DEVELOPMENT_OUTPUT_BOUND_EXCEEDED')
                except BaseException:
                    os.killpg(process.pid, signal.SIGTERM)
                    try:
                        process.wait(timeout=3)
                    except subprocess.TimeoutExpired:
                        os.killpg(process.pid, signal.SIGKILL)
                        process.wait()
                    raise
                time.sleep(0.1)
        require(stdout.stat().st_size + stderr.stat().st_size <= 1024 ** 2,
                'DEVELOPMENT_OUTPUT_BOUND_EXCEEDED')
        report['commands'].append({'label': label, 'exitCode': process.returncode})
        require(process.returncode == 0, 'DEVELOPMENT_COMMAND_FAILED_' + label.upper().replace('-', '_'))
        return stdout.read_bytes() + stderr.read_bytes()

    dmg = args.output / ('ExhibitOS Manager_' + version + '_development.dmg')
    try:
        copied = stage / 'ExhibitOS Manager.app'
        command(['/usr/bin/ditto', str(args.app), str(copied)], 'copy')
        require(inventory(copied) == before, 'DEVELOPMENT_COPY_CHANGED')
        command(['/usr/bin/codesign', '--force', '--sign', args.identity_sha1, '--timestamp=none',
                 '--identifier', 'org.exhibitos.manager', str(copied)], 'sign-app')
        command(['/usr/bin/codesign', '--verify', '--deep', '--strict', '--verbose=2', str(copied)], 'verify-app')
        authority = command(['/usr/bin/codesign', '--display', '--verbose=4', str(copied)], 'app-authority')
        require(b'Authority=Apple Development:' in authority and b'Signature=adhoc' not in authority,
                'DEVELOPMENT_SIGNATURE_CLASS_INVALID')
        signed = inventory(copied)
        resources = lambda values: {k: v for k, v in values.items() if k.startswith('Contents/Resources/')}
        require(resources(before) == resources(signed), 'DEVELOPMENT_RESOURCES_CHANGED')
        command(['/usr/bin/hdiutil', 'create', '-srcfolder', str(stage), '-volname',
                 'ExhibitOS Manager Development', '-fs', 'HFS+', '-format', 'UDZO', '-size', '256m', str(dmg)], 'create-dmg')
        require(dmg.stat().st_size <= 300 * 1024 ** 2, 'DEVELOPMENT_DMG_TOO_LARGE')
        command(['/usr/bin/codesign', '--force', '--sign', args.identity_sha1, '--timestamp=none',
                 '--identifier', 'org.exhibitos.manager.development.dmg', str(dmg)], 'sign-dmg')
        command(['/usr/bin/codesign', '--verify', '--strict', '--verbose=2', str(dmg)], 'verify-dmg-signature')
        authority = command(['/usr/bin/codesign', '--display', '--verbose=4', str(dmg)], 'dmg-authority')
        require(b'Authority=Apple Development:' in authority and b'Signature=adhoc' not in authority,
                'DEVELOPMENT_SIGNATURE_CLASS_INVALID')
        command(['/usr/bin/hdiutil', 'verify', str(dmg)], 'verify-image')
        image_hash = digest(dmg)
        attached = True
        command(['/usr/bin/hdiutil', 'attach', '-readonly', '-nobrowse', '-noautoopen', '-mountpoint',
                 str(mount), '-plist', str(dmg)], 'attach')
        entries = [e for e in plistlib.loads((args.output / 'attach.out').read_bytes())['system-entities']
                   if e.get('mount-point') == str(mount)]
        require(len(entries) == 1, 'DEVELOPMENT_MOUNT_INVALID')
        device = entries[0]['dev-entry']
        mounted = mount / 'ExhibitOS Manager.app'
        require(inventory(mounted) == signed, 'DEVELOPMENT_MOUNTED_APP_CHANGED')
        command(['/usr/bin/codesign', '--verify', '--deep', '--strict', '--verbose=2', str(mounted)], 'verify-mounted-app')
        require(inventory(args.app) == before and source(repo, args.source_commit) == pinned_source and
                build_binding(repo, args.source_commit, args.build_source_commit) == binding and
                digest(args.inventory) == args.inventory_sha256 and digest(dmg) == image_hash,
                'DEVELOPMENT_INPUT_CHANGED')
        (args.output / 'signed-inventory.json').write_text(json.dumps(signed, sort_keys=True) + '\n')
        report.update(state='PASS', originalAppUnchanged=True, resourcesUnchanged=True,
                      artifact={'name': dmg.name, 'bytes': dmg.stat().st_size, 'sha256': image_hash},
                      signedInventorySha256=digest(args.output / 'signed-inventory.json'))
    except BaseException as error:
        report.update(state='FAIL', errorType=type(error).__name__, code=str(error)
                      if isinstance(error, ValueError) else 'DEVELOPMENT_EXECUTION_FAILED')
    finally:
        CHECK = None
        try:
            target = device
            if attached and target is None:
                if os.path.ismount(mount):
                    target = str(mount)
                else:
                    details = subprocess.run(['/usr/bin/hdiutil', 'info', '-plist'], capture_output=True, timeout=8)
                    require(details.returncode == 0 and len(details.stdout) <= 1024 ** 2,
                            'DEVELOPMENT_ATTACHMENT_INSPECTION_FAILED')
                    owned = [i for i in plistlib.loads(details.stdout).get('images', [])
                             if i.get('image-path') == str(dmg)]
                    require(len(owned) <= 1, 'DEVELOPMENT_ATTACHMENT_AMBIGUOUS')
                    if owned:
                        devices = [e['dev-entry'] for e in owned[0].get('system-entities', [])
                                   if isinstance(e.get('dev-entry'), str) and e['dev-entry'].startswith('/dev/disk')]
                        require(devices, 'DEVELOPMENT_ATTACHMENT_DEVICE_MISSING')
                        target = devices[0]
            if target:
                command(['/usr/bin/hdiutil', 'detach', target], 'detach-owned-image', cleanup=True)
            report['ownedImageDetached'] = not os.path.ismount(mount)
            require(report['ownedImageDetached'], 'DEVELOPMENT_IMAGE_STILL_MOUNTED')
        except BaseException:
            report.update(state='FAIL', cleanupCode='DEVELOPMENT_OWNED_IMAGE_DETACH_FAILED')
        report.update(seconds=time.monotonic() - started, peakBytes=peak,
                      availableBefore=initial, availableAfter=shutil.disk_usage(args.output).free)
        receipt = args.output / 'result.json'
        receipt.write_text(json.dumps(report, indent=2) + '\n')
        for item in (receipt, args.output):
            descriptor = os.open(item, os.O_RDONLY)
            os.fsync(descriptor)
            os.close(descriptor)
        print(json.dumps({'state': report['state'], 'receipt': str(receipt), 'sha256': digest(receipt)}))
    return 0 if report['state'] == 'PASS' else 1


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (ValueError, OSError, subprocess.SubprocessError, json.JSONDecodeError) as error:
        print(str(error) if isinstance(error, ValueError) else 'DEVELOPMENT_PREFLIGHT_FAILED', file=sys.stderr)
        sys.exit(1)
