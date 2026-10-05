#!/usr/bin/env python3
"""Fresh synthetic CLI host loss/recovery; leaves failures, retires successful fixture."""
import argparse
from pathlib import Path
import hashlib
import json
import os
import shutil
import stat
import subprocess
import tempfile
import uuid


def sha(path):
    h = hashlib.sha256()
    with path.open('rb') as f:
        for b in iter(lambda: f.read(1024 * 1024), b''):
            h.update(b)
    return h.hexdigest()


def run(cli):
    root = Path(tempfile.mkdtemp(prefix='exhibitos-missing-host-cli-', dir='/private/tmp'))
    initial = root.lstat()
    available = shutil.disk_usage(root).free
    assert available > 4 * 65 * 1024 * 1024 + 6 * 1024**3
    profile = root / 'profile'
    profile.mkdir(mode=0o700)
    (profile / 'local-runtime').mkdir(mode=0o700)
    registry = json.dumps({'format': 1, 'activeId': str(uuid.uuid4()), 'installations': []})
    v = json.loads(registry)
    v['installations'] = [{'id': v['activeId'], 'kind': 'default', 'createdAt': 1}]
    registry = json.dumps(v).encode()
    def write(path, data):
        with os.fdopen(os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), 'wb') as f:
            f.write(data)
    write(profile / 'installation-selection.json', registry)
    write(profile / 'witness.bin', b'W' * (65 * 1024 * 1024))
    witness = sha(profile / 'witness.bin')
    key = root / 'key.bin'
    write(key, os.urandom(32))
    policy = root / 'policy.json'
    write(policy, json.dumps({'format': 1, 'channel': 'development', 'target': 'linux-arm64',
                             'protocolVersion': 1, 'sourceSchemaSha256': 'c' * 64,
                             'minimumSequence': 1, 'minimumIssuedAt': 0,
                             'publicKeys': ['d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a']}).encode())
    checks = []
    def call(command, flags=(), error=None):
        r = subprocess.run([str(cli), command, '--profile', str(profile), '--installation', 'default', *map(str, flags), '--apps-closed'], capture_output=True, text=True, timeout=600)
        assert not r.stderr, r.stderr
        value = json.loads(r.stdout)
        if error:
            assert r.returncode != 0 and value['code'] == error, value
        else:
            assert r.returncode == 0, value
        return value
    before = call('trust-provision', ['--policy', policy])
    pair = root / 'pair'
    call('checkpoint-host-trust', ['--key-file', key, '--destination', pair, '--host-writers-stopped'])
    hashes = {str(p.relative_to(root)): sha(p) for p in (key, pair / 'host.bin', pair / 'trust.bin')}
    flags = ['--host-archive', pair / 'host.bin', '--trust-archive', pair / 'trust.bin', '--key', key, '--absent-original-profile']
    call('restore-missing-host', flags, 'HOST_RESTORE_TARGET_EXISTS')
    checks.append('existing original profile refuses before any overwrite')
    old = root / 'original-retained'
    profile.rename(old)
    receipt = call('restore-missing-host', flags)
    assert receipt['hostProfileRestored'] and receipt['currentTrustPreserved']
    assert not any(receipt[k] for k in ('trustAuthorityRestored', 'runtimeDataRestored', 'runtimeStarted'))
    assert sha(profile / 'witness.bin') == sha(old / 'witness.bin') == witness
    assert (profile / 'installation-selection.json').read_bytes() == registry
    assert stat.S_IMODE((profile / 'witness.bin').stat().st_mode) == 0o600
    assert call('trust-status') == before
    stage = Path(receipt['recoveryWorkspace'])
    assert (stage / 'completed.json').is_file() and not (stage / 'host/payload.pending').exists()
    checks.append('65MiB exact witness/mode and original registry recover at original namespace; current trust unchanged on reopen')
    restored = root / 'restored-retained'
    profile.rename(restored)
    trust = next(p for p in root.iterdir() if p.name.startswith('.exhibitos-release-trust-'))
    moved = root / 'authority-retained'
    trust.rename(moved)
    call('restore-missing-host', flags, 'UPDATE_TRUST_MISSING')
    assert not profile.exists()
    moved.rename(trust)
    restored.rename(profile)
    assert call('trust-status') == before
    checks.append('missing independent authority refuses without bootstrap or host publication')
    for relative, expected in hashes.items():
        assert sha(root / relative) == expected
    checks.append('encrypted pair and external key unchanged throughout recovery/refusals')
    report = {'format': 1, 'passed': True, 'checks': checks, 'cliSha256': sha(cli),
              'freshWitnessBytes': 65 * 1024 * 1024, 'witnessSha256': witness,
              'sourceCommit': subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip(),
              'peakBudget': 4 * 65 * 1024 * 1024 + 6 * 1024**3,
              'availableBefore': available, 'recovery': receipt,
              'limitations': ['fresh synthetic65MiB host CLI proof; not full195file corpus or external DB/blob restoration',
                             'retained latest authority required; no lost-trust recovery, runtime start or update/rollback'],
              'successfulFreshFixtureRetired': False}
    # Entire scope was generated here; no user-supplied root or historical scan.
    final = root.lstat()
    assert (final.st_dev, final.st_ino, final.st_uid, stat.S_IMODE(final.st_mode)) == (initial.st_dev, initial.st_ino, initial.st_uid, 0o700)
    assert shutil.rmtree.avoids_symlink_attacks
    shutil.rmtree(root)
    report['successfulFreshFixtureRetired'] = True
    return report


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('cli', type=Path)
    a = p.parse_args()
    print(json.dumps(run(a.cli.resolve()), indent=2))
