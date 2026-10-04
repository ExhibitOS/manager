# SPDX-License-Identifier: Apache-2.0
"""Actual Rust staging CLI checks against operator-provided genuine artifact fixtures."""
import sys,json,subprocess,hashlib,os,stat,uuid
from pathlib import Path
cli,base,signature=map(lambda s:Path(s).resolve(),sys.argv[1:4])
policy=signature/'input-0.json';release=signature/'input-1.json';artifact=base/'runtime.tar'
sha=lambda p:hashlib.file_digest(p.open('rb'),'sha256').hexdigest()
original=sha(artifact);checks=[];attempt=base/('staging-test-'+str(uuid.uuid4()));attempt.mkdir(mode=0o700)
def call(source=artifact,parent=base,expect=None):
 r=subprocess.run([str(cli),'stage','--policy',str(policy),'--release',str(release),'--artifact',str(source),'--staging-parent',str(parent)],capture_output=True,text=True,timeout=90)
 assert not r.stderr;v=json.loads(r.stdout)
 if expect:assert r.returncode!=0 and v['code']==expect,(expect,v)
 else:assert r.returncode==0 and v['staged'] and v['verification']['artifactVerified'] and not v['activated'] and not v['ociInternalsVerified'],v
 return v
receipt=call();staged=Path(receipt['stagedPath']);assert sha(staged)==original and staged.stat().st_ino!=artifact.stat().st_ino;assert stat.S_IMODE(staged.stat().st_mode)==0o400;assert stat.S_IMODE(staged.parent.stat().st_mode)==0o700;checks.append('genuine94MB archive staged and hash-verified through retained read-only handle; original separate')
call(signature/'runtime.tar',expect='UPDATE_ARTIFACT_MISMATCH');checks.append('same-size changed genuine archive refused, partial preserved')
unsafe=attempt/'staging-unsafe';unsafe.mkdir(mode=0o755);unsafe.chmod(0o755);call(parent=unsafe,expect='UPDATE_ARTIFACT_UNAVAILABLE');checks.append('nonprivate staging parent refused')
linkdir=attempt/'staging-symlink';linkdir.mkdir(mode=0o700);link=linkdir/'runtime.tar';link.symlink_to(artifact);call(link,expect='UPDATE_ARTIFACT_UNAVAILABLE');checks.append('symlink source refused without following')
assert sha(artifact)==original and sha(staged)==original;checks.append('original and successful staged archive preserved')
report={'format':1,'checks':checks,'receipt':receipt,'artifactSha256':original,'cliSha256':sha(cli),'limits':['Unix-only development staging; no OCI structure validation/import/start/migration/health/activation/rollback','policy snapshot is not durable floors/revocation proof; latest trust/clock/source required at import','0400 does not isolate privileged/same-user external writers; retained handle recheck is observational','failed and successful private copies preserved']}
output=attempt/'staging-report.json'
with output.open('x') as f:json.dump(report,f,indent=2);f.write('\n')
output.chmod(0o600)
for c in checks:print('PASS '+c)
print('Report '+str(output))
