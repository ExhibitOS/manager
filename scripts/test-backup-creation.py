#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Actual Manager install/start/create + source-unavailable fresh service restoration.
Only new labelled synthetic resources are used. Private candidates/volumes retained.
"""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import socket
import subprocess
import tempfile
import time
import uuid

p=argparse.ArgumentParser()
for name in ('manager','runtime-image','maintenance-image','postgres-image'):
    p.add_argument('--'+name,required=True)
p.add_argument('--docker',default='docker')
p.add_argument('--workspace-parent',type=Path,default=Path('/private/tmp'))
a=p.parse_args()
for image in (a.runtime_image,a.maintenance_image):assert re.fullmatch(r'sha256:[a-f0-9]{64}',image)
assert re.fullmatch(r'[A-Za-z0-9/_.:-]+@sha256:[a-f0-9]{64}',a.postgres_image)
parent=a.workspace_parent
assert parent.is_absolute() and parent.resolve()==parent and parent.is_dir(), 'Use an existing canonical workspace parent'
assert parent.stat().st_uid==os.getuid(), 'Workspace parent must belong to the operator'
if parent!=Path('/private/tmp'):assert parent.stat().st_mode&0o077==0, 'Persistent fixture parent must be private'
base=Path(tempfile.mkdtemp(prefix='exhibitos-manager-backup-create-',dir=parent)).resolve()
root=base/'manager';root.mkdir(mode=0o700);bundle=root/'bundle';bundle.mkdir(mode=0o700)
key=base/'key.bin';key.write_bytes(secrets.token_bytes(32));key.chmod(0o600)
with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
id=str(uuid.uuid4());project='exhibitos-'+id
labels={'com.exhibitos.bundle':id,'com.exhibitos.project':project,'com.exhibitos.schema':'1.0.0-draft.1'}
checks=[];created=False;target=None
private_values=[]
def private(path,data):path.write_bytes(data);path.chmod(0o600)
def command(args,env=None,success=True):
    r=subprocess.run(args,env={**os.environ,**(env or {})},capture_output=True,timeout=900)
    if success and r.returncode:raise RuntimeError('Owned integration command failed; private fixture retained')
    return r

def docker(args,env=None):return command([a.docker,*args],env).stdout.decode().strip()
def invoke(action,*args,success=True):
    r=command([a.manager,'--root',str(root),action,*args],success=False)
    assert all(v.encode() not in r.stdout for v in private_values), 'CLI must not expose credentials'
    assert not r.stderr,'CLI must emit only safe structured result'
    value=json.loads(r.stdout)
    if success and (r.returncode or isinstance(value,dict) and value.get('state')=='failed'):
        raise RuntimeError('Native '+action+' failed with '+str(value.get('code',value.get('errorCode','unknown'))))
    if not success:assert r.returncode!=0
    return value

def step(name):checks.append(name);print('PASS '+name,flush=True)
def digest(path):
    h=hashlib.sha256()
    with path.open('rb') as f:
        for b in iter(lambda:f.read(1048576),b''):h.update(b)
    return h.hexdigest()

try:
    images=[]
    for index,reference in enumerate((a.runtime_image,a.postgres_image)):
        path=bundle/f'image-{index}.tar';private(path,b'')
        docker(['image','save','--output',str(path),reference])
        images.append({'reference':reference,'archive':{'path':path.name,'sha256':digest(path),'bytes':path.stat().st_size}})
    compose={'services':{
        'database':{'image':a.postgres_image,'environment':{'POSTGRES_PASSWORD':'${POSTGRES_PASSWORD:?required}','POSTGRES_DB':'exhibitos','POSTGRES_USER':'exhibitos'},'volumes':['database:/var/lib/postgresql'],'labels':labels,'healthcheck':{'test':['CMD-SHELL','pg_isready -U exhibitos -d exhibitos'],'interval':'2s','timeout':'3s','retries':30},'restart':'unless-stopped'},
        'platform':{'image':a.runtime_image,'pull_policy':'never','env_file':['../runtime.env'],'environment':{'NODE_ENV':'production','EXHIBITOS_PORT':'${EXHIBITOS_PORT}','AUTH_ORIGIN':'http://127.0.0.1:${EXHIBITOS_PORT}','BLOB_ROOT':'/data/blobs','CONFIG_ROOT':'/data/config'},'ports':['127.0.0.1:${EXHIBITOS_PORT}:8080'],'volumes':['objects:/data/blobs','configuration:/data/config'],'labels':labels,'depends_on':{'database':{'condition':'service_healthy'}},'read_only':True,'tmpfs':['/tmp:rw,nosuid,nodev,size=256m,mode=1777'],'cap_drop':['ALL'],'security_opt':['no-new-privileges:true'],'restart':'unless-stopped','stop_grace_period':'30s'}},
        'volumes':{name:{'labels':labels} for name in ('database','objects','configuration')},'networks':{'default':{'labels':labels}}}
    encoded=(json.dumps(compose,indent=2)+'\n').encode();private(bundle/'compose.yaml',encoded)
    manifest={'schemaVersion':'1.0.0-draft.1','bundleId':id,'version':'0.1.0','protocolVersion':'1','composeSha256':hashlib.sha256(encoded).hexdigest(),'preferredEngine':'docker','projectName':project,'services':['database','platform'],'images':images,'ports':[port],'openUrl':f'http://127.0.0.1:{port}','readinessUrl':f'http://127.0.0.1:{port}/api/v1/readiness','minimumFreeBytes':1024**3}
    private(bundle/'manifest.json',json.dumps(manifest).encode())
    invoke('install');created=True;invoke('start')
    env=dict(line.split('=',1) for line in (root/'runtime.env').read_text().splitlines())
    private_values.extend([env['POSTGRES_PASSWORD'],env['ADMIN_PASSWORD']])
    source_environment=(root/'runtime.env').read_bytes()
    compose_args=['compose','--env-file',str(root/'runtime.env'),'--project-name',project,'--file',str(bundle/'compose.yaml')]
    db=docker([*compose_args,'ps','--all','--quiet','database']);app=docker([*compose_args,'ps','--all','--quiet','platform'])
    step('actual Manager installs and starts a new labelled Runtime with private credentials')
    initializer="import {Pool} from 'pg';import {FileBlobStore} from './packages/storage/dist/index.js';const p=new Pool({connectionString:process.env.DATABASE_URL});try{await p.query(\"CREATE TABLE synthetic_manager_backup(id integer PRIMARY KEY, witness text NOT NULL)\");await p.query(\"INSERT INTO synthetic_manager_backup VALUES(1,'Manager backup original data')\");await new FileBlobStore('/data/blobs').put('synthetic/manager-proof',Buffer.from('synthetic Manager blob'))}finally{await p.end()}"
    docker(['run','--pull=never','--rm','--network',project+'_default','--read-only','--cap-drop','ALL','--security-opt','no-new-privileges:true','--env-file',str(root/'runtime.env'),'--mount',f'type=volume,source={project}_objects,target=/data/blobs','--entrypoint','node',a.maintenance_image,'--input-type=module','-e',initializer])
    assert invoke('create-backup',a.maintenance_image,str(key),'invalid',success=False)['code']=='BACKUP_OPERATOR_ACK_REQUIRED'
    key.chmod(0o644)
    assert invoke('create-backup',a.maintenance_image,str(key),'--external-writers-quiesced',success=False)['code']=='BACKUP_PRIVATE_PERMISSIONS'
    key.chmod(0o600)
    alias=base/'alias.bin';alias.symlink_to(key)
    assert invoke('create-backup',a.maintenance_image,str(alias),'--external-writers-quiesced',success=False)['code']=='BACKUP_PATH_INVALID'
    with (root/'operation.lock').open('rb') as lock:
        fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
        assert invoke('create-backup',a.maintenance_image,str(key),'--external-writers-quiesced',success=False)['code']=='BUSY'
    assert json.loads(docker(['inspect',app]))[0]['State']['Running']
    assert invoke('backup-jobs')==[]
    step('missing acknowledgement, unsafe key, alias and operation overlap fail before pausing writers')
    receipt=invoke('create-backup',a.maintenance_image,str(key),'--external-writers-quiesced')
    assert receipt['operation']=='created-and-authenticated' and receipt['writersPaused']
    workspace=root/('backup-creation-'+receipt['id']);archive=workspace/'archive'
    jobs=invoke('backup-jobs');assert jobs[-1]['state']=='completed' and jobs[-1]['stage']=='complete'
    assert (root/'runtime.env').read_bytes()==source_environment
    assert not json.loads(docker(['inspect',app]))[0]['State']['Running']
    assert json.loads(docker(['inspect',db]))[0]['State']['Running']
    assert all(f.stat().st_mode&0o777==0o600 for f in archive.rglob('*') if f.is_file())
    step('Manager pauses only Platform and creates authenticated DB/blob/configuration/image archive with durable completed job')
    invoke('start');assert invoke('status')['state']=='running';invoke('stop')
    source_archive_hashes={str(f.relative_to(archive)):digest(f) for f in archive.rglob('*') if f.is_file()}
    root.rename(base/'retained-source-manager');root=base/'retained-source-manager';archive=root/('backup-creation-'+receipt['id'])/'archive'
    step('original runtime restarts unchanged, then source DB is stopped and original Manager path is unavailable')
    target='exhibitos-backup-target-'+str(uuid.uuid4())
    docker(['run','--pull=never','-d','--name',target,'--label','com.exhibitos.backup.test='+id,'-p','127.0.0.1::5432','-e','POSTGRES_USER=exhibitos','-e','POSTGRES_DB=exhibitos','-e','POSTGRES_PASSWORD',a.postgres_image],{'POSTGRES_PASSWORD':env['POSTGRES_PASSWORD']})
    for _ in range(90):
        r=subprocess.run([a.docker,'exec',target,'pg_isready','-U','exhibitos','-d','exhibitos'],capture_output=True)
        if r.returncode==0:break
        time.sleep(.5)
    else:raise RuntimeError('Fresh target readiness failed')
    pgport=docker(['port',target,'5432/tcp']).split(':')[-1]
    output=base/'restore-work';output.mkdir(mode=0o700);blobs=output/'blobs';blobs.mkdir(mode=0o700)
    restored=json.loads(docker(['run','--pull=never','--rm','--user',f'{os.getuid()}:{os.getgid()}','--read-only','--cap-drop','ALL','--security-opt','no-new-privileges:true','--tmpfs','/tmp:rw,nosuid,nodev,size=256m,mode=1777','--mount',f'type=bind,source={key},target=/key,readonly','--mount',f'type=bind,source={archive},target=/archive,readonly','--mount',f'type=bind,source={output},target=/restore-work','-e','DATABASE_URL','-e','BLOB_ROOT=/restore-work/blobs',a.maintenance_image,'restore','--key-file','/key','--source','/archive','--destination','/restore-work/restored','--quiesced','--fresh-destination'],{'DATABASE_URL':f"postgresql://exhibitos:{env['POSTGRES_PASSWORD']}@host.docker.internal:{pgport}/exhibitos"}))
    assert restored['operation']=='restored-and-verified'
    assert docker(['exec',target,'psql','-U','exhibitos','-d','exhibitos','-Atc','SELECT witness FROM synthetic_manager_backup WHERE id=1'])=='Manager backup original data'
    assert (blobs/'synthetic/manager-proof').read_bytes()==b'synthetic Manager blob'
    config=output/'restored/configuration';assert (config/'manager-runtime.env').read_bytes()==source_environment
    assert (config/'freeze-signing-key.json').stat().st_size>0
    inventory=json.loads((config/'manager-image-inventory.json').read_text())
    for image in inventory:
        restored_image=output/'restored/deployment'/image['archive'];assert restored_image.stat().st_size==image['bytes'] and digest(restored_image)==image['sha256']
        assert json.loads(docker(['image','inspect',image['contentId']]))[0]['Id']==image['contentId']
    assert source_archive_hashes=={str(f.relative_to(archive)):digest(f) for f in archive.rglob('*') if f.is_file()}
    step('fresh target restores actual DB witness, blob bytes, original credentials/signing key and both image archives without changing source ciphertext')
    report={'format':1,'checks':checks,'receipt':receipt,'sourcePathUnavailable':True,'sourceDatabaseStopped':True,'imageArchives':[{k:v for k,v in i.items() if k!='reference'} for i in inventory],'limits':['same Docker engine; no cold engine image import or Manager restore activation','native UI/Windows/Podman/OEX/signed update and cancellation remain','new synthetic work volumes contain retained private plaintext and candidate bytes']}
    private(base/'backup-creation-report.json',(json.dumps(report,indent=2)+'\n').encode());print('Report '+str(base/'backup-creation-report.json'))
finally:
    # Stop only precisely labelled new fixture resources; retain volumes/candidates.
    if created:
        ids=docker(['ps','--all','--filter','label=com.exhibitos.project='+project,'--format','{{.ID}}']).splitlines()
        for cid in ids:
            info=json.loads(docker(['inspect',cid]))[0]
            assert info['Config']['Labels']['com.exhibitos.bundle']==id
            docker(['stop','-t','1',cid])
    if target:
        info=json.loads(docker(['inspect',target]))[0];assert info['Config']['Labels']['com.exhibitos.backup.test']==id
        docker(['stop','-t','1',target])
