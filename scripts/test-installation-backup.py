#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Actual native preparation + trusted Platform image encrypted configuration/PG restore.
Synthetic installation metadata only, not a Manager-created engine installation.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import subprocess
import tempfile
import time
import uuid

parser=argparse.ArgumentParser()
parser.add_argument('--manager',required=True)
parser.add_argument('--maintenance-image',required=True)
parser.add_argument('--postgres-image',required=True)
parser.add_argument('--docker',default='docker')
args=parser.parse_args()
assert re.fullmatch(r'sha256:[a-f0-9]{64}',args.maintenance_image)
assert re.fullmatch(r'[A-Za-z0-9/_.:-]+@sha256:[a-f0-9]{64}',args.postgres_image)
root=Path(tempfile.mkdtemp(prefix='exhibitos-installation-roundtrip-',dir='/private/tmp')).resolve()
source=root/'source-manager';source.mkdir(mode=0o700);(source/'bundle').mkdir(mode=0o700)
password=secrets.token_hex(24);admin=secrets.token_hex(24);tenant=str(uuid.uuid4());owner='exhibitos-installation-test-'+str(uuid.uuid4())
compose=b'services: {}\n'
manifest={'schemaVersion':'1.0.0-draft.1','bundleId':str(uuid.uuid4()),'version':'0.1.0','protocolVersion':'1','composeSha256':hashlib.sha256(compose).hexdigest(),'preferredEngine':'docker','projectName':'exhibitos-'+str(uuid.uuid4()),'services':['database','platform'],'images':[{'reference':args.maintenance_image,'archive':None}],'ports':[13200],'openUrl':'http://127.0.0.1:13200','readinessUrl':'http://127.0.0.1:13200/api/v1/readiness','minimumFreeBytes':1}
def private(path,data):
    path.write_bytes(data);path.chmod(0o600)
private(source/'bundle/compose.yaml',compose)
private(source/'bundle/manifest.json',json.dumps(manifest).encode())
private(source/'installed.json',json.dumps(manifest).encode())
private(source/'engine.json',b'"docker"')
private(source/'runtime.env',f'EXHIBITOS_PORT=13200\nPOSTGRES_PASSWORD={password}\nDATABASE_URL=postgresql://exhibitos:{password}@database:5432/exhibitos\nADMIN_SUBJECT=local-admin\nADMIN_PASSWORD={admin}\nTENANT_ID={tenant}\n'.encode())
checks=[];containers=[]
def step(name):checks.append(name);print('PASS '+name,flush=True)
def command(arguments,env=None):
    value=subprocess.run(arguments,env={**os.environ,**(env or {})},capture_output=True,timeout=180)
    if value.returncode:raise RuntimeError('Owned integration command failed; private candidate retained')
    assert password.encode() not in value.stdout and admin.encode() not in value.stdout
    return value.stdout.decode().strip()
def docker(arguments,env=None):return command([args.docker,*arguments],env)
try:
    receipt=json.loads(command([args.manager,'--root',str(source),'prepare-installation-backup']))
    assert receipt['operation']=='prepared-configuration' and receipt['files']==5
    workspace=source/('backup-preparation-'+receipt['id']);mapping=json.loads((workspace/'configuration-files.json').read_text())
    originals={name:Path(path).read_bytes() for name,path in mapping.items()}
    assert all(Path(p).stat().st_mode&0o777==0o600 for p in mapping.values())
    step('native CLI prepares private exact configuration without exposing credentials')
    (root/'key').mkdir(mode=0o700);key=root/'key/key.bin';private(key,secrets.token_bytes(32))
    output=root/'output';output.mkdir(mode=0o700);blobs=output/'source-blobs';blobs.mkdir(mode=0o700)
    names=[];ports=[]
    for suffix in ('source','target'):
        name=owner+'-'+suffix
        docker(['run','--pull=never','-d','--name',name,'--label','com.exhibitos.installation.test='+owner,'-p','127.0.0.1::5432','-e','POSTGRES_USER=exhibitos','-e','POSTGRES_DB=exhibitos','-e','POSTGRES_PASSWORD',args.postgres_image],{'POSTGRES_PASSWORD':password})
        containers.append(name);names.append(name);ports.append(docker(['port',name,'5432/tcp']).split(':')[-1])
        for attempt in range(90):
            probe=subprocess.run([args.docker,'exec',name,'pg_isready','-h','127.0.0.1','-U','exhibitos','-d','exhibitos'],capture_output=True)
            if probe.returncode==0:break
            time.sleep(.5)
        else:raise RuntimeError('Owned PostgreSQL readiness failed')
    urls=[f'postgresql://exhibitos:{password}@host.docker.internal:{port}/exhibitos' for port in ports]
    env={'DATABASE_URL':urls[0]}
    initializer="import {Pool} from 'pg';import {migrate} from './packages/storage/dist/index.js';const p=new Pool({connectionString:process.env.DATABASE_URL});try{await migrate(p,'/opt/exhibitos/database/migrations');await p.query(\"CREATE TABLE synthetic_installation_backup(id integer PRIMARY KEY, witness text NOT NULL)\");await p.query(\"INSERT INTO synthetic_installation_backup VALUES(1,'synthetic installation preserved')\")}finally{await p.end()}"
    docker(['run','--pull=never','--rm','--entrypoint','node','--read-only','--cap-drop','ALL','--security-opt','no-new-privileges:true','-e','DATABASE_URL',args.maintenance_image,'--input-type=module','-e',initializer],env)
    def backup_cli(arguments,environment,include_configuration=False):
        mounts=['--mount',f'type=bind,source={key},target=/key,readonly','--mount',f'type=bind,source={output},target=/work']
        if include_configuration:mounts+=['--mount',f'type=bind,source={workspace},target={workspace},readonly']
        return json.loads(docker(['run','--pull=never','--rm','--user',f'{os.getuid()}:{os.getgid()}','--read-only','--tmpfs','/tmp:rw,nosuid,nodev,size=256m,mode=1777','--cap-drop','ALL','--security-opt','no-new-privileges:true',*mounts,*sum((['-e',name] for name in environment),[]),args.maintenance_image,*arguments],environment))
    result=backup_cli(['create','--key-file','/key','--destination','/work/archive','--quiesced'],{'DATABASE_URL':urls[0],'BLOB_ROOT':'/work/source-blobs','BACKUP_CONFIGURATION_FILES':json.dumps(mapping)},True)
    assert result['operation']=='created';step('trusted image encrypts actual PG and all five native installation files')
    docker(['stop','-t','1',names[0]]);source.rename(root/'retained-source-manager');assert not source.exists()
    step('original Manager installation path and source database are unavailable')
    restored_blobs=output/'restored-blobs';restored_blobs.mkdir(mode=0o700)
    result=backup_cli(['restore','--key-file','/key','--source','/work/archive','--destination','/work/restore','--quiesced','--fresh-destination'],{'DATABASE_URL':urls[1],'BLOB_ROOT':'/work/restored-blobs'})
    assert result['operation']=='restored-and-verified'
    restored=output/'restore/configuration'
    for name,expected in originals.items():
        assert (restored/name).read_bytes()==expected and (restored/name).stat().st_mode&0o777==0o600
    step('fresh PostgreSQL restore stages identical private installation configuration')
    witness=docker(['exec',names[1],'psql','-U','exhibitos','-d','exhibitos','-Atc','SELECT witness FROM synthetic_installation_backup WHERE id=1'])
    assert witness=='synthetic installation preserved';step('actual restored PG witness and original credentials are preserved')
    report={'format':1,'checks':checks,'receipt':receipt,'files':{name:hashlib.sha256(data).hexdigest() for name,data in originals.items()},'sourceUnavailable':True,'limits':['synthetic metadata; no actual Manager engine installation or restore activation','no DB/blob/image creation UI, native GUI, Windows/Podman or update qualification']}
    private(root/'installation-roundtrip-report.json',(json.dumps(report,indent=2)+'\n').encode());print('Report '+str(root/'installation-roundtrip-report.json'))
finally:
    for name in reversed(containers):
        value=json.loads(subprocess.check_output([args.docker,'inspect',name],text=True))[0]
        assert value['Config']['Labels']['com.exhibitos.installation.test']==owner
        docker(['rm','-fv',name])
