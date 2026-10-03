#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Actual Docker CLI regression with operator-supplied synthetic archive fixtures.
Never supplies key contents on argv, changes source archives, or removes old data.
"""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser()
for name in ('manager', 'root', 'image', 'key', 'source', 'expected-manifest', 'corrupted-source'):
    parser.add_argument('--' + name, required=True)
args = parser.parse_args()
root = Path(args.root)
key = Path(args.key)
source = Path(args.source)

def digest(path):
    value = hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1048576), b''):
            value.update(chunk)
    return value.hexdigest()

def inventory(directory):
    return {str(path.relative_to(directory)): digest(path)
            for path in sorted(directory.rglob('*')) if path.is_file()}

before, key_before = inventory(source), key.read_bytes()
checks = []
def invoke(image=args.image, key_path=key, archive=source):
    result = subprocess.run([args.manager, '--root', str(root), 'verify-backup',
        image, str(key_path), str(archive)], capture_output=True, timeout=120)
    # Only structured native safe results are accepted; never dump raw stderr.
    assert not result.stderr, 'Native result must not expose engine stderr'
    assert key_before not in result.stdout, 'Key contents must not enter output'
    return result.returncode, json.loads(result.stdout)

def reject(code, **options):
    status, value = invoke(**options)
    assert status == 1 and value['code'] == code, 'Unexpected safe rejection'

status, value = invoke()
assert status == 0 and value['operation'] == 'verified' and value['files'] > 0
workspace = root / ('backup-verification-' + value['id'])
assert json.loads((workspace / 'receipt.json').read_text()) == value
assert value['authenticatedManifestSha256'] == digest(Path(args.expected_manifest))
assert digest(workspace / 'plaintext/manifest.json') == value['authenticatedManifestSha256']
assert workspace.stat().st_mode & 0o777 == 0o700
assert (workspace / 'receipt.json').stat().st_mode & 0o777 == 0o600
checks.append('actual isolated image authenticates archive and binds private receipt to manifest hash')

wrong = root / 'fixture-wrong-key.bin'
with wrong.open('xb') as stream:
    stream.write(os.urandom(32))
wrong.chmod(0o600)
reject('BACKUP_VERIFICATION_FAILED', key_path=wrong)
assert len(list(root.glob('backup-verification-*/failed.json'))) == 1
checks.append('actual wrong key refuses verification and retains failed candidate privately')
reject('BACKUP_VERIFICATION_FAILED', archive=Path(args.corrupted_source))
assert len(list(root.glob('backup-verification-*/failed.json'))) == 2
checks.append('actual corrupted authenticated archive refuses success')
reject('BACKUP_IMAGE_INVALID', image='exhibitos:latest')
checks.append('mutable image tag cannot authorize maintenance')
wrong.chmod(0o644)
reject('BACKUP_PRIVATE_PERMISSIONS', key_path=wrong)
wrong.chmod(0o600)
checks.append('public key permissions fail before engine execution')
alias = root / 'fixture-key-alias'
alias.symlink_to(key)
reject('BACKUP_PATH_INVALID', key_path=alias)
checks.append('key symlink cannot escape validated mount')
with (root / 'operation.lock').open('r+b') as lock:
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    reject('BUSY')
checks.append('real process lock excludes concurrent lifecycle mutations')
assert inventory(source) == before and key.read_bytes() == key_before
checks.append('original encrypted archive and external key remain byte identical')
report = {'passed': True, 'checks': checks, 'receipt': value,
    'limits': ['synthetic macOS Docker verification only', 'not database restore or Manager UI acceptance',
        'interrupted process candidates retained for operator inspection', 'Windows deliberately gated']}
(root / 'verification-test.json').write_text(json.dumps(report, indent=2) + '\n')
(root / 'verification-test.json').chmod(0o600)
for check in checks:
    print('PASS ' + check)
print('Actual Manager backup verification checks: ' + str(len(checks)))
