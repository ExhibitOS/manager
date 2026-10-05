#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Read-only Engine source observation; retires only its own verified image exports.
Requires an existing Prepared registered source/recovery candidate and qualified
images. This is not an update, a coherent backup or a lost-data restoration test.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess


def require(value, message):
    if not value:
        raise RuntimeError(message)


def digest(path):
    h = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            h.update(block)
    return h.hexdigest()


def census(profile):
    result = {}
    for path in profile.rglob('*'):
        before = path.lstat()
        require(not stat.S_ISLNK(before.st_mode), 'aliased fixture path')
        if not stat.S_ISREG(before.st_mode) or path.name in ('operation.lock', 'profile-session.lock'):
            continue
        proof = (before.st_size, stat.S_IMODE(before.st_mode), digest(path))
        after = path.lstat()
        require((before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns,
                 before.st_ctime_ns) == (after.st_dev, after.st_ino, after.st_size,
                                        after.st_mtime_ns, after.st_ctime_ns),
                'fixture changed while hashing')
        result[str(path.relative_to(profile))] = proof
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cli', type=Path, required=True)
    parser.add_argument('--profile', type=Path, required=True)
    parser.add_argument('--image', required=True)
    parser.add_argument('--docker', required=True)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    require(args.profile.is_absolute() and args.profile.resolve() == args.profile,
            'profile must be canonical')
    require(args.report.is_absolute() and args.report.parent.resolve() == args.report.parent,
            'report parent must be canonical')
    require(args.report.parent.stat().st_mode & 0o077 == 0, 'private report parent required')
    require(not args.report.exists() and not args.report.is_symlink(), 'report already exists')
    budget = 8 * 1024**3  # maximum fresh exports (2GiB) plus conservative 6GiB floor
    available = shutil.disk_usage(args.profile).free
    require(available >= budget, 'insufficient observation budget')
    common = ['--profile', str(args.profile), '--installation', 'default']

    def call(command, extra=()):
        run = subprocess.run([str(args.cli), command, *common, *extra, '--apps-closed'],
                             capture_output=True, text=True, timeout=900)
        value = json.loads(run.stdout)
        require(run.returncode == 0, 'CLI refused: ' + str(value.get('code')))
        return value

    def engine(*arguments):
        return subprocess.check_output([args.docker, *arguments], text=True)

    def containers():
        ids = engine('ps', '-aq').split()
        values = json.loads(engine('inspect', *ids)) if ids else []
        return {v['Id']: (v['Name'], v['State']['Status'], v['State']['Running'],
                           v['State']['ExitCode']) for v in values}

    before = call('update-intent')
    files = census(args.profile)
    old_containers = containers()
    old_volumes = sorted(engine('volume', 'ls', '-q').split())
    proof = call('verify-update-source-recovery-transient',
                 ['--image', args.image, '--external-writers-quiesced'])
    observation = proof['observation']
    require(all(proof[k] is True for k in ('dataInventoryVerified',
            'configurationInventoryVerified', 'imageBytesVerified')), 'missing complete checks')
    require(proof['preflightVerified'] is False and proof['updateExecuted'] is False,
            'unverified execution scope')
    first, last = observation['inventory'], observation['repeatedInventory']
    require(first['sourceContentSha256'] == last['sourceContentSha256'] and
            first['inventory'] == last['inventory'], 'repeated data inventory mismatch')
    config = observation['configuration']
    require(len(config['files']) == 7 and len(config['images']) == 2,
            'full supported configuration/image scope missing')
    require(config['imageArchivesRetained'] is False, 'exports not retired')
    workspace = Path(config['exportWorkspace'])
    require(workspace.is_relative_to(args.profile) and workspace.resolve() == workspace,
            'unexpected observation workspace')
    require(sorted(p.name for p in workspace.iterdir()) == ['verified-images.json'],
            'unexpected remaining exports')
    require(call('update-intent') == before, 'Prepared intent/trust changed')
    after = census(args.profile)
    require(all(after.get(name) == value for name, value in files.items()),
            'old source bytes/modes changed')
    require(containers() == old_containers and
            sorted(engine('volume', 'ls', '-q').split()) == old_volumes,
            'Engine containers/volume inventory changed')
    report = {'format': 1, 'passed': True, 'cliSha256': digest(args.cli),
              'requiredBeforeBytes': budget, 'availableBeforeBytes': available,
              'originalFilesPreserved': len(files), 'oldBytesModesPreserved': True,
              'intentTrustPreserved': True, 'containerStatesVolumeNamesPreserved': True,
              'freshImageBytesRetired': sum(i['bytes'] for i in config['images']),
              'retainedMarkerBytes': (workspace / 'verified-images.json').stat().st_size,
              'observation': observation, 'fullCheckpointRoundTripExecuted': False,
              'newExternalDataArchive': False, 'fullRecoveryProven': False,
              'preflightVerified': False, 'updateExecuted': False}
    with os.fdopen(os.open(args.report, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), 'w') as stream:
        json.dump(report, stream, indent=2)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())
    print(json.dumps({'passed': True, 'report': str(args.report),
                      'reportSha256': digest(args.report), 'oldFiles': len(files),
                      'freshImageBytesRetired': report['freshImageBytesRetired']}))


if __name__ == '__main__':
    main()
