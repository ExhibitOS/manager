#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Actual offline CLI with retained synthetic host bytes, no engine activation."""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import uuid

p = argparse.ArgumentParser()
p.add_argument('--profile-cli', required=True)
a = p.parse_args()
base = Path(tempfile.mkdtemp(prefix='exhibitos-host-proof-', dir='/private/tmp'))
base.chmod(0o700)
profile = base / 'source'
profile.mkdir(mode=0o700)
external = base / 'external'
external.mkdir(mode=0o700)
key = external / 'host-key.bin'
archive = external / 'host.exb'
checks = []

def private(path, data):
    path.write_bytes(data)
    path.chmod(0o600)

def sha(path):
    h = hashlib.sha256()
    with path.open('rb') as f:
        for block in iter(lambda: f.read(65536), b''):
            h.update(block)
    return h.hexdigest()

def step(name):
    checks.append(name)
    print('PASS ' + name, flush=True)

def call(kind, root=profile, keypath=key, arc=archive, target=None,
         closed=True, stopped=True, extract=True, error=None):
    args = [a.profile_cli, '--profile', str(root), kind, str(keypath), str(arc)]
    if kind == 'checkpoint-host':
        args += ['--apps-closed' if closed else '--missing-app-ack',
                 '--host-writers-stopped' if stopped else '--missing-writer-ack']
    elif kind == 'extract-host':
        args += [str(target), '--extract-only' if extract else '--missing-extract-ack']
    else:
        args += ['--apps-closed']
    r = subprocess.run(args, capture_output=True, timeout=90)
    assert not r.stderr, 'unexpected stderr'
    v = json.loads(r.stdout)
    if error is None:
        assert r.returncode == 0, v.get('code', 'unknown')
    else:
        assert r.returncode != 0 and v['code'] == error, (v.get('code'), error)
    return v

private(key, os.urandom(32))
registry = {'format': 1, 'activeId': str(uuid.uuid4()), 'installations': []}
registry['installations'] = [{'id': registry['activeId'], 'kind': 'default',
                             'createdAt': int(time.time() * 1000)}]
private(profile / 'installation-selection.json', (json.dumps(registry) + '\n').encode())
root = profile / 'local-runtime'
root.mkdir(mode=0o700)
candidate = root / 'restore-candidate'
candidate.mkdir(mode=0o700)
private(candidate / 'candidate.bin', b'synthetic-host-data\x00' * 110000)
private(candidate / 'empty-witness', b'')
private(root / 'raw-job-journal.json', b'{synthetic interrupted journal bytes')
private(root / 'runtime.env', b'SYNTHETIC=1\n')
source = {str(f.relative_to(profile)): (sha(f), f.stat().st_mode & 0o777)
          for f in profile.rglob('*') if f.is_file()}
key_hash = sha(key)
call('checkpoint-host', closed=False, error='PROFILE_ACK_REQUIRED')
call('checkpoint-host', stopped=False, error='HOST_WRITER_ACK_REQUIRED')
assert not archive.exists()
step('explicit closed-app and operator-writer acknowledgements precede copying')
anchor = profile.parent / ('.exhibitos-profile-session-' +
                          hashlib.sha256(str(profile).encode()).hexdigest() + '.lock')
for path, shared in [(anchor, True), (profile / 'profile-session.lock', True),
                     (profile / 'operation.lock', False), (root / 'operation.lock', False)]:
    path.touch(mode=0o600)
    with path.open('r+b') as held:
        fcntl.flock(held, fcntl.LOCK_SH if shared else fcntl.LOCK_EX)
        call('checkpoint-host', error='PROFILE_BUSY')
        assert not archive.exists()
step('independent pathname anchor, legacy app-session, profile and root locks refuse')
saved = call('checkpoint-host')
assert not saved['externalVolumesSaved']
assert saved['hostWriterQuiescence'] == 'operator-acknowledged'
cipher = archive.read_bytes()
assert b'synthetic-host-data' not in cipher and archive.stat().st_mode & 0o777 == 0o600
archive_hash = sha(archive)
call('checkpoint-host', error='PROFILE_DESTINATION_EXISTS')
step('actual authenticated host archive includes raw candidate/journal bytes without overwriting')
target = external / 'recovered'
wrong = external / 'wrong-key.bin'
private(wrong, os.urandom(32))
call('extract-host', keypath=wrong, target=target, error='HOST_CHECKPOINT_INVALID')
for name, data in [('truncated.exb', cipher[:-1]),
                   ('tampered.exb', cipher[:-1] + bytes([cipher[-1] ^ 1]))]:
    bad = external / name
    private(bad, data)
    call('extract-host', arc=bad, target=target, error='HOST_CHECKPOINT_INVALID')
    assert not target.exists()
step('wrong key, truncation and tampering retain private failure staging without target publication')
call('extract-host', root=base / 'wrong-namespace', target=target, error='HOST_CHECKPOINT_INVALID')
metadata = external / 'metadata.exb'
call('backup-stream', arc=metadata)
call('extract-host', arc=metadata, target=target, error='HOST_CHECKPOINT_INVALID')
call('extract-host', target=target, extract=False, error='PROFILE_ACK_REQUIRED')
assert not target.exists()
step('original canonical namespace, payload domain and extraction acknowledgement enforced')
# Change only the namespace of this NEW synthetic source; preserve all its bytes.
retained = base / 'retained-source'
profile.rename(retained)
opened = call('extract-host', target=target)
assert opened['id'] == saved['id'] and opened['manifestSha256'] == saved['manifestSha256']
assert opened['operation'] == 'host-profile-extracted-not-activated'
assert target.stat().st_mode & 0o777 == 0o700
restored = target / 'profile'
for name, (h, mode) in source.items():
    assert sha(restored / name) == h and (restored / name).stat().st_mode & 0o777 == mode
    assert sha(retained / name) == h
for name in ['operation.lock', 'profile-session.lock', 'local-runtime/operation.lock']:
    assert not (restored / name).exists()
assert not profile.exists() and not (target / 'payload.pending').exists()
assert sha(archive) == archive_hash and sha(key) == key_hash
assert json.loads((target / 'verified.json').read_text()) == opened
call('extract-host', target=target, error='PROFILE_DESTINATION_EXISTS')
step('original profile absent: exact host bytes and POSIX modes extracted privately without activation')
# Separate unsupported sources; never delete or repair the test files.
for kind in ['symlink', 'hardlink', 'unsafe-mode']:
    bad_profile = base / ('bad-' + kind)
    bad_profile.mkdir(mode=0o700)
    private(bad_profile / 'installation-selection.json',
            (retained / 'installation-selection.json').read_bytes())
    bad = bad_profile / 'unsupported'
    if kind == 'symlink':
        bad.symlink_to(key)
    elif kind == 'hardlink':
        private(bad_profile / 'first', b'synthetic')
        os.link(bad_profile / 'first', bad)
    else:
        private(bad, b'synthetic')
        bad.chmod(0o622)
    output = external / (kind + '.exb')
    call('checkpoint-host', root=bad_profile, arc=output, error='HOST_CHECKPOINT_INVALID')
    assert not output.exists()
step('unsupported symlink, hardlink and writable permissions refuse instead of silently skipping')
assert sha(key) == key_hash and sha(archive) == archive_hash
assert all(sha(retained / n) == h for n, (h, _) in source.items())
step('original synthetic bytes, external key and completed archive preserved')
report = {'format': 1, 'checks': checks, 'fixture': str(base), 'saved': saved,
          'extracted': opened, 'cliSha256': sha(Path(a.profile_cli)), 'archiveSha256': archive_hash,
          'limits': ['Unix actual host-file CLI; no external engine volumes, live activation, GUI, Windows or powerloss',
                     'writer quiescence is operator acknowledgement, not engine verification',
                     'all source/key/archive/quarantine files retained; no automatic deletion']}
output = base / 'host-report.json'
private(output, (json.dumps(report, indent=2) + '\n').encode())
print('Report ' + str(output), flush=True)
print('SHA256 ' + sha(output), flush=True)
