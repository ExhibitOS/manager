#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Actual native fresh installation recovery using a retained synthetic encrypted fixture.
No original data/backup/volume deletion. New candidate volumes and private journals retained.
"""
import argparse,fcntl,hashlib,json,os,secrets,shutil,socket,subprocess,tempfile,uuid
from pathlib import Path
from urllib.request import Request,urlopen
p=argparse.ArgumentParser()
p.add_argument('--reuse-test-workspace');p.add_argument('--fixture',required=True);p.add_argument('--manager',required=True);p.add_argument('--docker',default='docker')
a=p.parse_args();fixture=Path(a.fixture).resolve();manager=Path(a.manager).resolve()
report=json.loads((fixture/'backup-creation-report.json').read_text());image=report['receipt']['image']
original=fixture/'retained-source-manager';offline=fixture/'retained-source-manager.temporarily-unavailable'
assert original.is_dir() and not offline.exists()
manifest=json.loads((original/'bundle/manifest.json').read_text());source=original/('backup-creation-'+report['receipt']['id'])/'archive';key=fixture/'key.bin'
if a.reuse_test_workspace:
 base=Path(a.reuse_test_workspace).resolve()
 assert base.parent==Path('/private/tmp') and base.name.startswith('exhibitos-manager-full-restore-') and base.is_dir() and base.stat().st_uid==os.getuid() and base.stat().st_mode&0o777==0o700
 archive=base/'archive'
 assert json.loads((archive/'complete.json').read_text())['id']==report['receipt']['backupId']
else:
 base=Path(tempfile.mkdtemp(prefix='exhibitos-manager-full-restore-',dir='/private/tmp')).resolve();base.chmod(0o700)
 archive=base/'archive';shutil.copytree(source,archive);archive.chmod(0o700)
attempt=base/('attempt-'+str(uuid.uuid4()));attempt.mkdir(mode=0o700);print('Private fixture '+str(attempt),flush=True)
key_before=key.read_bytes();checks=[];target=attempt/'new-manager';target.mkdir(mode=0o700)
def command(args,timeout=900):return subprocess.run(args,capture_output=True,timeout=timeout)
def docker(*args):
 r=command([a.docker,*args]);assert r.returncode==0,'Owned Docker command failed';return r.stdout
private_env=dict(line.split('=',1) for line in (original/'runtime.env').read_text().splitlines())
private_values=[private_env['POSTGRES_PASSWORD'],private_env['ADMIN_PASSWORD']]
def invoke(root,action,*args,ok=True):
 r=command([str(manager),'--root',str(root),action,*map(str,args)])
 assert not r.stderr and all(v.encode() not in r.stdout for v in private_values),'Native result must not expose credentials'
 result=json.loads(r.stdout)
 assert (r.returncode==0)==ok,(action,result.get('code',result.get('errorCode','unknown')) if isinstance(result,dict) else 'invalid')
 return result
with socket.socket() as listener:listener.bind(('127.0.0.1',0));port=listener.getsockname()[1]
def restore(root,source=archive,keyfile=key,ack='--fresh-installation',image_id=image,ok=False):return invoke(root,'restore-backup',image_id,keyfile,source,port,ack,ok=ok)
def fresh(name):root=attempt/name;root.mkdir(mode=0o700);return root
def hashes(directory):return {str(f.relative_to(directory)):hashlib.sha256(f.read_bytes()).hexdigest() for f in directory.rglob('*') if f.is_file()}
def states(project):return sorted(docker('ps','--all','--filter','label=com.docker.compose.project='+project,'--format','{{.ID}} {{.State}}').decode().splitlines())
def step(text):checks.append(text);print('PASS '+text,flush=True)
archive_before=hashes(archive);original_before=hashes(source);source_states=states(manifest['projectName'])
assert source_states and all(line.endswith('exited') for line in source_states),'Synthetic source must already be stopped'
try:
 root=fresh('ack-required');assert restore(root,ack='invalid')['code']=='BACKUP_OPERATOR_ACK_REQUIRED'
 assert restore(root,image_id='image:latest')['code']=='BACKUP_IMAGE_INVALID'
 alias=attempt/'key-alias';alias.symlink_to(key);assert restore(root,keyfile=alias)['code']=='BACKUP_PATH_INVALID'
 root=fresh('existing-target');(root/'keep').write_bytes(b'Original candidate marker');assert restore(root)['code']=='RESTORE_FRESH_ROOT_REQUIRED';assert (root/'keep').read_bytes()==b'Original candidate marker'
 root=fresh('busy');invoke(root,'restoration-status')
 with (root/'operation.lock').open('rb') as lock:
  fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB);assert restore(root)['code']=='BUSY'
 step('acknowledgement, immutable image, alias, existing target and actual process lock fail before resource creation')
 wrong=attempt/'wrong-key';wrong.write_bytes(secrets.token_bytes(32));wrong.chmod(0o600);root=fresh('wrong-key-target');assert restore(root,keyfile=wrong)['code']=='RESTORE_FAILED'
 job=invoke(root,'restoration-status');assert job['state']=='failed' and job['stage']=='authenticating' and not (root/'installed.json').exists()
 assert invoke(root,'start',ok=False)['errorCode']=='RESTORE_RECOVERY_REQUIRED'
 step('actual wrong key retains failed journal and blocks ordinary writer start without installing target')
 corrupted=base/'corrupted-archive'
 if not corrupted.exists():
  shutil.copytree(archive,corrupted);corrupted.chmod(0o700)
  path=next((corrupted/'files').iterdir());data=bytearray(path.read_bytes());data[-1]^=1;path.write_bytes(data)
 assert hashes(corrupted)!=hashes(archive)
 root=fresh('tampered-target');assert restore(root,source=corrupted)['code']=='RESTORE_FAILED';assert not (root/'installed.json').exists()
 step('actual authenticated ciphertext corruption rejects before target installation')
 original.rename(offline);assert not original.exists()
 receipt=restore(target,ok=True);assert receipt['operation']=='restored-and-running' and receipt['backupId']==report['receipt']['backupId'] and receipt['authenticatedManifestSha256']==report['receipt']['authenticatedManifestSha256']
 restored_manifest=json.loads((target/'bundle/manifest.json').read_text());assert restored_manifest['bundleId']!=manifest['bundleId'] and restored_manifest['projectName']!=manifest['projectName'] and restored_manifest['ports']==[port]
 new_env=dict(line.split('=',1) for line in (target/'runtime.env').read_text().splitlines());assert all(new_env[k]==v for k,v in private_env.items() if k!='EXHIBITOS_PORT')
 assert invoke(target,'restoration-status')['state']=='completed' and invoke(target,'status')['state']=='running'
 step('source Manager path unavailable: actual CLI imports preserved images into new identity/volumes/port, preserves credentials and reaches Runtime readiness')
 project=restored_manifest['projectName'];compose=['compose','--env-file',str(target/'runtime.env'),'--project-name',project,'--file',str(target/'bundle/compose.yaml')]
 db=docker(*compose,'ps','--all','--quiet','database').decode().strip();app=docker(*compose,'ps','--all','--quiet','platform').decode().strip()
 assert docker('exec',db,'psql','-U','exhibitos','-d','exhibitos','-Atc','SELECT witness FROM synthetic_manager_backup WHERE id=1').decode().strip()=='Manager backup original data'
 assert docker('exec',app,'node','--input-type=module','-e',"import{readFile}from'node:fs/promises';process.stdout.write(await readFile('/data/blobs/synthetic/manager-proof'))")==b'synthetic Manager blob'
 expected_signing=(fixture/'restore-work/restored/configuration/freeze-signing-key.json').read_bytes()
 assert docker('exec',app,'node','--input-type=module','-e',"import{readFile}from'node:fs/promises';process.stdout.write(await readFile('/data/config/freeze-signing-key.json'))")==expected_signing
 url=receipt['openUrl'];login={'subject':private_env['ADMIN_SUBJECT'],'password':private_env['ADMIN_PASSWORD'],'tenantId':private_env['TENANT_ID']}
 request=Request(url+'/api/v1/auth/login',data=json.dumps(login).encode(),headers={'Content-Type':'application/json','Origin':url},method='POST')
 with urlopen(request,timeout=15) as response:assert response.status==200 and response.headers.get('Set-Cookie')
 with urlopen(url,timeout=15) as response:assert response.status==200 and len(response.read())>100
 step('restored actual DB witness/blob/signing key and original administrator HTTP login/web work')
 invoke(target,'stop');assert invoke(target,'status')['state']=='stopped';invoke(target,'start');assert invoke(target,'status')['state']=='running'
 step('completed receipt permits actual stop/start with preserved restored service data')
 assert archive_before==hashes(archive) and original_before==hashes(offline/('backup-creation-'+report['receipt']['id'])/'archive') and key.read_bytes()==key_before and states(manifest['projectName'])==source_states
 step('original encrypted source/key and source container IDs/states remain unchanged')
 output={'format':1,'checks':checks,'receipt':receipt,'sourcePathUnavailable':True,'limits':['same Docker engine with existing image content cache; archive load path exercised, not cold-engine qualification','native GUI/Windows/Podman/full-corpus freeze/update/cancel UI remain','all new private candidates/volumes retained; no original dataset introduced']}
 result=attempt/'restoration-report.json';result.write_text(json.dumps(output,indent=2)+'\n');result.chmod(0o600)
 print('Report '+str(result));print('SHA256 '+hashlib.sha256(result.read_bytes()).hexdigest())
finally:
 if (target/'bundle/manifest.json').exists():
  new=json.loads((target/'bundle/manifest.json').read_text());project=new['projectName']
  for cid in docker('ps','--all','--filter','label=com.docker.compose.project='+project,'--format','{{.ID}}').decode().splitlines():
   value=json.loads(docker('inspect',cid))[0];assert value['Config']['Labels']['com.exhibitos.bundle']==new['bundleId'] and value['Config']['Labels']['com.exhibitos.project']==project
   docker('stop','--time','1',cid)
 if offline.exists():assert not original.exists();offline.rename(original)
