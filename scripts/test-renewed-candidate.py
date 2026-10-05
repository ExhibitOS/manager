#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Actual fresh development candidate restore; not target apply or full preflight."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import shutil
import socket
import stat
import subprocess
import uuid


def need(value, message):
    if not value:
        raise RuntimeError(message)


def sha(path):
    result = hashlib.sha256()
    with path.open('rb') as source:
        for block in iter(lambda: source.read(1048576), b''):
            result.update(block)
    return result.hexdigest()


def canonical(value, directory=False):
    path = Path(value)
    need(path.is_absolute() and path.resolve(strict=True) == path and not path.is_symlink(), 'noncanonical input')
    need(path.is_dir() if directory else path.is_file(), 'input type invalid')
    return path


def put(directory, name, value):
    path = directory / name
    with path.open('x') as output:
        os.chmod(path, 0o600)
        json.dump(value, output, indent=2)
        output.write('\n')
    return path


def census(root):
    result = {}
    for path in root.rglob('*'):
        metadata = path.lstat()
        need(not stat.S_ISLNK(metadata.st_mode), 'unexpected snapshot link')
        if stat.S_ISREG(metadata.st_mode) and path.name not in ('operation.lock', 'profile-session.lock'):
            result[str(path.relative_to(root))] = {'sha256': sha(path), 'bytes': metadata.st_size, 'mode': stat.S_IMODE(metadata.st_mode)}
        else:
            need(stat.S_ISDIR(metadata.st_mode) or stat.S_ISREG(metadata.st_mode), 'unexpected snapshot object')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for option in ('cli', 'manager', 'renewal-report', 'archive', 'key', 'maintenance-image'):
        parser.add_argument('--' + option, required=True)
    parser.add_argument('--local-development-fixture', action='store_true', required=True)
    args = parser.parse_args()
    cli, manager = canonical(args.cli), canonical(args.manager)
    renewal_path, key = canonical(args.renewal_report), canonical(args.key)
    archive = canonical(args.archive, True)
    need(key.stat().st_size == 32 and stat.S_IMODE(key.stat().st_mode) == 0o600, 'key input invalid')
    renewal = json.loads(renewal_path.read_text())
    need(renewal['privateSigningKeyPersisted'] is False and renewal['updateExecuted'] is False, 'not local development renewal')
    need(renewal['candidateRestored'] is False and renewal['preflightVerified'] is False, 'unexpected renewal result')
    plan = renewal['newPlan']
    target = Path(renewal['registeredTarget']['targetPath'])
    profile = canonical(str(target.parent.parent), True)
    target = canonical(str(target), True)
    need(target == profile / 'installations' / plan['targetInstance'], 'target path mismatch')
    need(renewal['registeredTarget']['activeInstance'] == plan['sourceInstance'], 'source registration mismatch')
    release = json.loads(json.loads(canonical(renewal['releasePath']).read_text())['payload'])
    need(release['channel'] == 'development' and release['expiresAt'] > int(datetime.datetime.now(datetime.timezone.utc).timestamp()), 'renewed release expired or wrong channel')
    artifact = canonical(renewal['artifact']['path'])
    need(sha(artifact) == renewal['artifact']['sha256'] and artifact.stat().st_size == renewal['artifact']['bytes'], 'artifact changed')
    need(args.maintenance_image.startswith('sha256:') and len(args.maintenance_image) == 71 and all(c in '0123456789abcdef' for c in args.maintenance_image[7:]), 'maintenance image must be immutable')
    docker = shutil.which('docker')
    need(docker is not None, 'Docker unavailable')

    def docker_call(extra):
        result = subprocess.run([docker, *extra], capture_output=True, text=True, timeout=120)
        need(result.returncode == 0, 'Docker observation failed')
        return result.stdout

    def containers():
        ids = docker_call(['ps', '-aq']).split()
        if not ids:
            return {}
        lines = docker_call(['inspect', '--format', '{{.Id}} {{.State.Status}} {{.State.Running}} {{.State.ExitCode}}', *ids]).splitlines()
        return {row[0]: row[1:] for row in (line.split() for line in lines)}

    def call(action, extra=()):
        result = subprocess.run([str(cli), action, '--profile', str(profile), '--installation', 'default', *extra, '--apps-closed'], capture_output=True, text=True, timeout=900)
        need(not result.stderr, 'unexpected CLI diagnostics')
        value = json.loads(result.stdout)
        need(result.returncode == 0, 'update CLI refused: ' + str(value.get('code')))
        return value

    def manager_call(action):
        result = subprocess.run([str(manager), '--root', str(target), action], capture_output=True, text=True, timeout=120)
        need(not result.stderr, 'unexpected Manager diagnostics')
        value = json.loads(result.stdout)
        need(result.returncode == 0, 'Manager CLI refused: ' + str(value.get('code')))
        return value

    before = call('update-intent')
    need(before['intent']['update']['plan'] == plan and before['intent']['update']['stage'] == 'prepared', 'current Prepared mismatch')
    need(before['intent']['update']['preflight'] is None, 'not unexecuted preparation')
    need(not list(target.iterdir()), 'target not fresh; existing candidate preserved')
    files, backups, states = census(profile), census(archive), containers()
    key_hash = sha(key)
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        port = listener.getsockname()[1]
    workspace = profile.parent / ('renewed-candidate-' + str(uuid.uuid4()))
    workspace.mkdir(mode=0o700)
    started = datetime.datetime.now(datetime.timezone.utc).isoformat()
    put(workspace, 'before.json', {'startedAt': started, 'plan': plan, 'intent': before, 'fileHashes': files, 'archiveHashes': backups, 'containerStates': states, 'keySha256': key_hash, 'port': port})
    print(json.dumps({'stage': 'before_actual_restore', 'workspace': str(workspace), 'target': plan['targetInstance']}), flush=True)
    # No automatic cleanup, stop or retry on failure. The exact failed candidate is retained.
    receipt = call('prepare-update-candidate', ['--maintenance-image', args.maintenance_image, '--key', str(key), '--archive', str(archive), '--port', str(port), '--fresh-candidate', '--external-writers-quiesced'])
    put(workspace, 'candidate-receipt.json', receipt)
    need(receipt['candidatePrepared'] is True and receipt['preflightVerified'] is False and receipt['updateExecuted'] is False, 'candidate scope mismatch')
    restored = receipt['restoration']
    proof = restored['sourceVerification']
    need(restored['backupId'] == plan['backupId'] and restored['authenticatedManifestSha256'] == plan['backupManifest'], 'backup binding mismatch')
    need(proof['inventorySha256'] == plan['sourceInventory'] and proof['schemaSha256'] == plan['sourceSchema'] and proof['runtimeImageSha256'] == plan['sourceImage'], 'restored source binding mismatch')
    need(call('update-intent') == before, 'intent/floors changed')
    need(census(archive) == backups and sha(key) == key_hash, 'archive/key changed')
    after = census(profile)
    need(all(after.get(name) == value for name, value in files.items()), 'original profile bytes/modes changed')
    current = containers()
    need(all(current.get(name) == value for name, value in states.items()), 'existing container state changed')
    status = manager_call('status')
    need(status['state'] == 'running', 'candidate not running')
    stopped = manager_call('stop')
    need(manager_call('status')['state'] == 'stopped', 'candidate not stopped')
    need(call('update-intent') == before, 'intent changed after explicit candidate stop')
    report = {'startedAt': started, 'completedAt': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'cliSha256': sha(cli), 'managerCliSha256': sha(manager), 'renewalReport': str(renewal_path), 'receipt': receipt, 'runningStatus': status, 'stoppedResult': stopped, 'originalProfileFiles': len(files), 'oldProfileFilesAndModesUnchanged': True, 'oldContainerStatesUnchanged': True, 'archiveKeyUnchanged': True, 'candidateStopped': True, 'sourceImageRestoredNotTargetImageApplied': True, 'preflightVerified': False, 'updateExecuted': False, 'workspace': str(workspace), 'targetPath': str(target), 'limitations': ['fresh source-version candidate restore, not target update/migration/health/rollback', 'fresh current source/compatibility/resource/coherent recovery remains required', 'no live authority restoration or production signing', 'Engine observations alone are not full original volume byte preservation proof']}
    report_path = put(workspace, 'report.json', report)
    print(json.dumps({'status': 'PASS', 'report': str(report_path)}), flush=True)


if __name__ == '__main__':
    main()
