#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Unix CLI/filesystem recovery proof. Synthetic private roots retained; no engine commands."""
import argparse, fcntl, hashlib, json, os, subprocess, tempfile, time, uuid
from pathlib import Path
p = argparse.ArgumentParser()
p.add_argument('--manager', required=True)
a = p.parse_args()
base = Path(tempfile.mkdtemp(prefix='exhibitos-manager-retry-recovery-', dir='/private/tmp')).resolve()
base.chmod(0o700)
checks = []
def ident(): return str(uuid.uuid4())
def raw(value): return json.dumps(value, separators=(',', ':')).encode()
def digest(value): return hashlib.sha256(value).hexdigest()
def write(path, value):
    path.write_bytes(value if isinstance(value, bytes) else raw(value)); path.chmod(0o600)
def directory(path): path.mkdir(mode=0o700); return path
def invoke(root, action, *args, code=None):
    r = subprocess.run([a.manager, '--root', str(root), action, *map(str, args)], capture_output=True, timeout=30)
    assert not r.stderr, 'Unexpected private CLI stderr; retained for diagnosis'
    value = json.loads(r.stdout)
    assert (r.returncode == 0) == (code is None), (action, value.get('code') if isinstance(value, dict) else 'unexpected')
    if code: assert value['code'] == code, (action, value['code'])
    assert str(base) not in r.stdout.decode(), 'Private path leaked in CLI result'
    return value
def fixture(kind, reserve=False):
    work = directory(base / ident()); source = directory(work / 'source'); destination = source if kind == 'backup' else directory(work / 'destination')
    target, parent, child = ident(), ident(), ident(); at = int(time.time() * 1000)
    job = {'id': target, 'state': 'failed', 'stage': 'preflight' if kind == 'backup' else 'authenticating', 'errorCode': 'CANCELLED', 'createdAt': at, 'updatedAt': at}
    if kind == 'backup': job['operation'] = 'create'; path = directory(source / ('backup-creation-' + target)) / 'job.json'
    else: path = source / 'restoration.json'
    write(path, job)
    audit = {'id': parent, 'targetId': target, 'kind': kind, 'state': 'preparing', 'newJobId': None, 'originalJobSha256': digest(path.read_bytes()), 'destinationRootSha256': digest(str(destination).encode()), 'errorCode': None, 'createdAt': at, 'updatedAt': at}
    audit_path = source / ('maintenance-retry-' + parent + '.json'); write(audit_path, audit)
    preparation = {'formatVersion': 1, 'retryId': parent, 'targetId': target, 'kind': kind, 'originalJobSha256': audit['originalJobSha256'], 'destinationRootSha256': audit['destinationRootSha256'], 'backupWorkspaceIds': [target] if kind == 'backup' else []}
    prepare_path = source / ('retry-preparation-' + parent + '.json'); write(prepare_path, preparation)
    intent_path = source / ('retry-child-intent-' + parent + '.json')
    if reserve: write(intent_path, {'formatVersion': 1, 'retryId': parent, 'jobId': child, 'preparationSha256': digest(prepare_path.read_bytes())})
    write(work / 'external-witness', b'synthetic external key/archive witness: preserve')
    return source, destination, target, parent, child, path, audit_path, prepare_path, intent_path

def step(text): checks.append(text); print('PASS ' + text, flush=True)
for kind in ['backup', 'restoration']:
    f = fixture(kind); source, destination, target, parent, child, original, audit_path, prep, intent = f; root = '--same-root' if kind == 'backup' else destination
    before, old = audit_path.read_bytes(), original.read_bytes()
    history = invoke(source, 'retry-diagnostic-history'); assert history[0]['id'] == parent and history[0]['state'] == 'preparing'
    proof = invoke(source, 'diagnose-retry', parent, root); assert proof['outcome'] == 'no-child-created' and proof['canReconcile']
    assert audit_path.read_bytes() == before and original.read_bytes() == old
    invoke(source, 'reconcile-retry', parent, root, '--no-consent', code='RETRY_ACK_REQUIRED'); assert audit_path.read_bytes() == before
    receipt = invoke(source, 'reconcile-retry', parent, root, '--preserve-candidates'); assert receipt['newJobId'] is None and receipt['dataPreserved']
    assert original.read_bytes() == old and json.loads(audit_path.read_bytes())['errorCode'] == 'RETRY_NOT_STARTED'
    assert (source / ('retry-diagnosis-' + receipt['diagnosisId'] + '-before.json')).read_bytes() == before
    clearance = source / ('retry-clearance-' + parent + '-' + receipt['diagnosisId'] + '.json'); assert json.loads(clearance.read_bytes())['auditSha256'] == digest(audit_path.read_bytes())
    step(kind + ': pure history/diagnosis, acknowledgement refusal and durable pre-child reconciliation preserve original bytes')

    f = fixture(kind, True); source, destination, target, parent, child, original, audit_path, prep, intent = f; root = '--same-root' if kind == 'backup' else destination
    workspace = directory(destination / (('backup-creation-' if kind == 'backup' else 'restore-') + child)); write(workspace / 'candidate-witness', b'synthetic child candidate retained')
    job = {'id': child, 'state': 'running', 'stage': 'preflight' if kind == 'backup' else 'authenticating', 'errorCode': None, 'createdAt': 1, 'updatedAt': 1}
    if kind == 'backup': job['operation'] = 'create'; path = workspace / 'job.json'
    else: path = destination / 'restoration.json'
    write(path, job); before, old, child_bytes = audit_path.read_bytes(), original.read_bytes(), path.read_bytes()
    proof = invoke(source, 'diagnose-retry', parent, root); assert proof['outcome'] == 'child-found' and proof['newJobId'] == child and proof['childJobSha256'] == digest(child_bytes); assert audit_path.read_bytes() == before
    receipt = invoke(source, 'reconcile-retry', parent, root, '--preserve-candidates'); assert receipt['newJobId'] == child
    assert path.read_bytes() == child_bytes and original.read_bytes() == old and (workspace / 'candidate-witness').read_bytes() == b'synthetic child candidate retained'
    audit = json.loads(audit_path.read_bytes()); assert audit['state'] == 'interrupted' and audit['newJobId'] == child
    invoke(source, 'reconcile-retry', parent, root, '--preserve-candidates', code='RETRY_RECOVERY_UNPROVEN')
    step(kind + ': actual CLI links reserved orphan journal without recovering/relabeling child or repeating old job')

    f = fixture(kind, True); source, destination, target, parent, child, original, audit_path, prep, intent = f; root = '--same-root' if kind == 'backup' else destination
    workspace = directory(destination / (('backup-creation-' if kind == 'backup' else 'restore-') + child))
    receipt = invoke(source, 'reconcile-retry', parent, root, '--preserve-candidates'); assert receipt['newJobId'] is None and workspace.is_dir()
    if kind == 'backup':
        assert invoke(source, 'maintenance-retries')[0]['state'] == 'failed'
        clearance = source / ('retry-clearance-' + parent + '-' + receipt['diagnosisId'] + '.json'); saved = clearance.read_bytes(); clearance.unlink()
        write(source.parent / 'external-key', b'K' * 32)
        invoke(source, 'retry-backup', target, 'sha256:' + 'a' * 64, source.parent / 'external-key', '--preserve-candidates', '--external-writers-quiesced', code='STATE_UNAVAILABLE')
        repaired = invoke(source, 'reconcile-retry', parent, root, '--preserve-candidates'); assert repaired['diagnosisId'] != receipt['diagnosisId'] and saved
        assert invoke(source, 'maintenance-retries')[0]['state'] == 'failed'
    step(kind + ': reserved empty candidate is retained; current reader accepts only proven clearance and missing clearance remains fenced')

    f = fixture(kind, True); source, destination, target, parent, child, original, audit_path, prep, intent = f; root = '--same-root' if kind == 'backup' else destination
    workspace = directory(destination / (('backup-creation-' if kind == 'backup' else 'restore-') + child)); write(workspace / 'candidate-witness', b'incomplete data: never discard')
    before = audit_path.read_bytes(); proof = invoke(source, 'diagnose-retry', parent, root); assert proof['outcome'] == 'candidate-incomplete' and not proof['canReconcile']
    invoke(source, 'reconcile-retry', parent, root, '--preserve-candidates', code='RETRY_RECOVERY_UNPROVEN'); assert audit_path.read_bytes() == before and (workspace / 'candidate-witness').read_bytes() == b'incomplete data: never discard'
    step(kind + ': incomplete candidate remains inspectable through diagnostic-only root open and refuses recovery')

    f = fixture(kind); source, destination, target, parent, child, original, audit_path, prep, intent = f; root = '--same-root' if kind == 'backup' else destination
    before = audit_path.read_bytes(); prep.unlink(); assert invoke(source, 'diagnose-retry', parent, root)['outcome'] == 'unproven'; invoke(source, 'reconcile-retry', parent, root, '--preserve-candidates', code='RETRY_RECOVERY_UNPROVEN'); assert audit_path.read_bytes() == before
    step(kind + ': legacy missing preparation preserves fence and never guesses a child')

    f = fixture(kind); source, destination, target, parent, child, original, audit_path, prep, intent = f; root = '--same-root' if kind == 'backup' else destination
    write(intent, b'{incomplete proof'); before = audit_path.read_bytes(); invoke(source, 'diagnose-retry', parent, root, code='STATE_INVALID'); assert audit_path.read_bytes() == before
    original.write_bytes(b'changed original'); invoke(source, 'diagnose-retry', parent, root, code='RETRY_SOURCE_CHANGED')
    step(kind + ': malformed reservation and changed original refuse without rewriting audit')

f = fixture('restoration', True); source, destination, target, parent, child, original, audit_path, prep, intent = f
workspace = directory(destination / ('restore-' + child)); foreign = ident(); write(destination / 'restoration.json', {'id': foreign, 'state': 'running', 'stage': 'authenticating', 'errorCode': None, 'createdAt': 1, 'updatedAt': 1}); before = (destination / 'restoration.json').read_bytes()
invoke(source, 'diagnose-retry', parent, destination, code='RETRY_CANDIDATE_CONFLICT'); assert (destination / 'restoration.json').read_bytes() == before
invoke(source, 'diagnose-retry', parent, directory(source.parent / 'wrong-destination'), code='RETRY_DESTINATION_INVALID')
step('foreign child journal and wrong canonical destination are refused and byte-preserved')
for locked in [source, destination]:
    path = locked / 'operation.lock'; write(path, b'')
    with path.open('rb') as handle:
        fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB); invoke(source, 'diagnose-retry', parent, destination, code='BUSY'); fcntl.flock(handle, fcntl.LOCK_UN)
step('actual independent Unix source and destination flock ownership prevents diagnosis')
report = {'format': 1, 'checks': checks, 'managerSha256': digest(Path(a.manager).read_bytes()), 'limits': ['Synthetic real CLI/filesystem only; no engine operation or native GUI', 'All fixtures retained; only owned synthetic missing-clearance/legacy fixtures model metadata loss', 'Process interruption protocol, not powerloss or production data backup']}
path = base / 'recovery-report.json'; write(path, report)
print('Report ' + str(path)); print('SHA256 ' + digest(path.read_bytes()))
