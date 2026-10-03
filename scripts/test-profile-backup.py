#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Actual encrypted offline profile round-trip with retained synthetic data only."""
import argparse,fcntl,hashlib,json,os,shutil,subprocess,tempfile,time,uuid
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--profile-cli',required=True);a=p.parse_args();base=Path(tempfile.mkdtemp(prefix='exhibitos-manager-profile-proof-',dir='/private/tmp'));base.chmod(0o700);profile=base/'source-profile';profile.mkdir(mode=0o700);outside=base/'external';outside.mkdir(mode=0o700);key=outside/'profile-key.bin';key.write_bytes(os.urandom(32));key.chmod(0o600);archive=outside/'profile.exb';checks=[]
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def private(p,b):p.write_bytes(b);p.chmod(0o600)
def step(s):checks.append(s);print('PASS '+s,flush=True)
def call(root,kind,keypath=key,archivepath=archive,ack=True,expected=None):
 r=subprocess.run([a.profile_cli,'--profile',str(root),kind,str(keypath),str(archivepath),'--apps-closed' if ack else '--no-consent'],capture_output=True,timeout=90);assert not r.stderr;v=json.loads(r.stdout)
 if expected is None:assert r.returncode==0,v.get('code','unknown')
 else:assert r.returncode!=0 and v['code']==expected,(v.get('code','unknown'),expected)
 return v
at=int(time.time()*1000);first=str(uuid.uuid4());second=str(uuid.uuid4());registry={'format':1,'activeId':second,'installations':[{'id':first,'kind':'default','createdAt':at},{'id':second,'kind':'recovery','createdAt':at}]};raw=(json.dumps(registry)+'\n').encode();history=profile/'selection-history';history.mkdir(mode=0o700);old={**registry,'activeId':first,'installations':registry['installations'][:1]};oldbytes=(json.dumps(old)+'\n').encode();history_name=f'{at}-{uuid.uuid4()}-{hashlib.sha256(oldbytes).hexdigest()}.json';private(history/history_name,oldbytes);private(profile/'installation-selection.json',raw);(profile/'installations').mkdir(mode=0o700)
for root in [profile/'local-runtime',profile/'installations'/second]:root.mkdir(mode=0o700);private(root/'synthetic-witness',b'private synthetic space witness')
source_hashes={str(f.relative_to(profile)):sha(f) for f in profile.rglob('*') if f.is_file()};keyhash=sha(key)
empty=base/'empty-runtime-root';empty.mkdir(mode=0o700);call(empty,'backup',expected='PROFILE_FORMAT_INVALID');assert not list(empty.iterdir()) and not archive.exists();step('missing registry backup source refuses before creating lock files in a fresh runtime folder')
before_paths=sorted(str(f.relative_to(base)) for f in base.rglob('*'));call(profile,'backup',archivepath=outside/('x'*300),expected='PROFILE_DESTINATION_UNAVAILABLE');assert sorted(str(f.relative_to(base)) for f in base.rglob('*'))==before_paths and (profile/'installation-selection.json').read_bytes()==raw and sha(key)==keyhash;step('actual inaccessible archive name returns destination-unavailable before creating profile locks or pending files')
call(profile,'backup',ack=False,expected='PROFILE_ACK_REQUIRED');assert not archive.exists();step('acknowledgement rejects before archive creation')
for path in [profile/'profile-session.lock',profile/'operation.lock',profile/'local-runtime/operation.lock']:
 path.touch(mode=0o600)
 with path.open('r+b') as held:
  fcntl.flock(held,fcntl.LOCK_SH if path.name=='profile-session.lock' else fcntl.LOCK_EX)
  call(profile,'backup',expected='PROFILE_BUSY');assert not archive.exists()
step('actual cross-process shared app-session and profile/root operation locks refuse offline backup')
receipt=call(profile,'backup');assert receipt['spaces']==2 and receipt['history']==1;cipher=archive.read_bytes();assert raw not in cipher and second.encode() not in cipher;assert archive.stat().st_mode&0o777==0o600;cipherhash=sha(archive);call(profile,'backup',expected='PROFILE_DESTINATION_EXISTS');step('actual AES-GCM archive excludes plaintext registry and never overwrites an existing copy')
wrong=outside/'wrong-key.bin';private(wrong,os.urandom(32));call(profile,'restore',keypath=wrong,expected='PROFILE_AUTHENTICATION_FAILED')
for name,data in [('truncated.exb',cipher[:-1]),('tampered.exb',cipher[:-1]+bytes([cipher[-1]^1]))]:
 bad=outside/name;private(bad,data);call(profile,'restore',archivepath=bad,expected='PROFILE_AUTHENTICATION_FAILED');assert (profile/'installation-selection.json').read_bytes()==raw
step('wrong external key, truncation and tamper refuse pointer replacement with original bytes unchanged')
# Make the entire original profile path unavailable. Target spaces are new synthetic witness directories,
# not a runtime DB/blob recovery claim.
retained=base/'retained-source-profile';profile.rename(retained);target=base/'target-profile';target.mkdir(mode=0o700);(target/'installations').mkdir(mode=0o700)
for root in [target/'local-runtime',target/'installations'/second]:root.mkdir(mode=0o700);private(root/'synthetic-witness',b'private synthetic space witness')
third=str(uuid.uuid4());(target/'installations'/third).mkdir(mode=0o700);private(target/'installations'/third/'later-witness',b'newer space retained');newer={**registry,'activeId':third,'installations':registry['installations']+[{'id':third,'kind':'recovery','createdAt':at}]};newerbytes=(json.dumps(newer)+'\n').encode();private(target/'installation-selection.json',newerbytes);before_witnesses={str(f.relative_to(target)):sha(f) for f in target.rglob('*') if f.name.endswith('witness')}
with (target/'profile-session.lock').open('w+b') as held:
 os.chmod(held.name,0o600);fcntl.flock(held,fcntl.LOCK_SH);call(target,'restore',expected='PROFILE_BUSY');assert (target/'installation-selection.json').read_bytes()==newerbytes
step('target live-session lock rejects actual restoration before registry replacement')
restore_receipt=call(target,'restore');assert restore_receipt['id']==receipt['id'];assert (target/'installation-selection.json').read_bytes()==raw;assert (target/'selection-history'/history_name).read_bytes()==oldbytes;assert before_witnesses=={str(f.relative_to(target)):sha(f) for f in target.rglob('*') if f.name.endswith('witness')};assert any((d/'previous-registry.bin').read_bytes()==newerbytes for d in target.glob('profile-restore-*'));step('original profile unavailable: authenticated pointer/history restore preserves all target and later-only space data plus previous pointer')
checkpoint=outside/'next-profile-checkpoint.exb';call(target,'backup',archivepath=checkpoint);checkpoint_target=base/'checkpoint-target-profile';checkpoint_target.mkdir(mode=0o700);call(checkpoint_target,'restore',archivepath=checkpoint);assert any(f.read_bytes()==newerbytes for f in (checkpoint_target/'selection-history').iterdir());step('later-only registrations remain in authenticated history after the next encrypted profile backup/restore')
private(target/'installation-selection.json',b'');call(target,'restore');assert (target/'installation-selection.json').read_bytes()==raw;assert any((d/'previous-registry.bin').read_bytes()==b'' for d in target.glob('profile-restore-*'));step('zero-byte corrupted current pointer is preserved before authenticated repair')
private(target/'selection-history'/history_name,b'changed synthetic history');private(target/'installation-selection.json',newerbytes);call(target,'restore',expected='PROFILE_HISTORY_CONFLICT');assert (target/'installation-selection.json').read_bytes()==newerbytes;step('history name collision refuses pointer changes before writes')
missing=base/'missing-spaces-profile';missing.mkdir(mode=0o700);call(missing,'restore');assert (missing/'installation-selection.json').read_bytes()==raw and not(missing/'local-runtime').exists() and not(missing/'installations').exists();step('missing registered runtime folders are never regenerated by profile restoration')
alias=outside/'key-alias';alias.symlink_to(key);call(missing,'restore',keypath=alias,expected='PROFILE_KEY_INVALID');key.chmod(0o644);r=subprocess.run([a.profile_cli,'--profile',str(missing),'restore',str(key),str(archive),'--apps-closed'],capture_output=True);assert r.returncode!=0;key.chmod(0o600);step('alias and permissive external keys fail closed')
assert keyhash==sha(key) and cipherhash==sha(archive);assert all(sha(retained/n)==h for n,h in source_hashes.items());step('original profile metadata/history/witnesses, external key and encrypted archive remain unchanged')
report={'format':1,'checks':checks,'receipt':receipt,'restore':restore_receipt,'fixture':str(base),'archiveSha256':cipherhash,'limits':['actual Unix CLI/profile metadata, not native GUI/Windows/powerloss/runtime DB/blob backup','all old/source/archive/key/target/failed candidates retained; no engine lifecycle','source root deliberately renamed only inside new synthetic fixture']};output=base/'profile-report.json';private(output,(json.dumps(report,indent=2)+'\n').encode());print('Report '+str(output),flush=True);print('SHA256 '+sha(output),flush=True)
