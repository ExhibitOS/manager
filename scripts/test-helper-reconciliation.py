#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Actual owned synthetic Docker helper reconciliation; no DB restore/data pruning.
All new candidates, work volumes and private journals are retained.
"""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import time
import uuid

p=argparse.ArgumentParser()
p.add_argument('--manager',required=True)
p.add_argument('--image',required=True)
p.add_argument('--docker',default='docker')
a=p.parse_args()
assert re.fullmatch(r'sha256:[a-f0-9]{64}',a.image)
base=Path(tempfile.mkdtemp(prefix='exhibitos-manager-helper-reconcile-',dir='/private/tmp')).resolve()
batch=str(uuid.uuid4());created=[];checks=[]
def private(path,value):
    path.write_bytes(value);path.chmod(0o600)
def command(args,ok=True):
    r=subprocess.run(args,capture_output=True,timeout=120)
    if ok and r.returncode:raise RuntimeError('Owned synthetic integration command failed; candidates retained')
    return r
def docker(args):return command([a.docker,*args]).stdout.decode().strip()
def invoke(root,*args,success=True):
    r=command([a.manager,'--root',str(root),*args],ok=False)
    assert not r.stderr,'CLI emits only safe structured results'
    value=json.loads(r.stdout)
    assert (r.returncode==0)==success,value.get('code','unknown')
    assert 'original synthetic bytes' not in r.stdout.decode()
    return value
def step(name):checks.append(name);print('PASS '+name,flush=True)
def directory(path):path.mkdir(mode=0o700)
def helper(name,label,target,mounts,remove=False,user=None):
    args=['run','--pull=never','-d','--name',name,'--label',label+'='+target,'--label','com.exhibitos.reconciliation.test='+batch,'--network','none','--read-only','--cap-drop','ALL','--security-opt','no-new-privileges:true','--pids-limit','32','--memory','64m']
    if remove:args+=['--rm']
    if user:args+=['--user',user]
    for mount in mounts:args+=['--mount',mount]
    cid=docker([*args,'--entrypoint','node',a.image,'-e','setInterval(()=>{},1000)'])
    assert re.fullmatch(r'[a-f0-9]{64}',cid)
    created.append(cid);return cid
def inspect(cid):return json.loads(docker(['inspect',cid]))[0]
def job(root,kind,target,state):
    value={'id':target,'state':state,'stage':'authenticating' if kind=='restoration' else 'encrypting-and-authenticating','errorCode':'INTERRUPTED' if state=='interrupted' else 'ENGINE_TIMEOUT','createdAt':1,'updatedAt':2}
    if kind=='backup':value['operation']='create';path=root/('backup-creation-'+target)/'job.json'
    else:path=root/'restoration.json'
    private(path,(json.dumps(value)+'\n').encode());return path

def preserved(root,path):return (root/'synthetic-original').read_bytes(),path.read_bytes()
def states():
    result={}
    for cid in docker(['ps','--all','--no-trunc','--format','{{.ID}}']).splitlines():
        value=inspect(cid);result[cid]={k:value['State'][k] for k in ['Status','Running','Paused','Restarting']}
    return result
try:
    existing_states=states()
    assert json.loads(docker(['image','inspect',a.image]))[0]['Id']==a.image
    backup=base/'backup-root';directory(backup);bid=str(uuid.uuid4());directory(backup/('backup-creation-'+bid))
    private(backup/'synthetic-original',b'original synthetic bytes retained');bjob=job(backup,'backup',bid,'failed');initial=preserved(backup,bjob)
    volume='exhibitos-backup-work-'+bid;docker(['volume','create','--label','com.exhibitos.backup='+bid,'--label','com.exhibitos.reconciliation.test='+batch,volume])
    docker(['run','--pull=never','--rm','--network','none','--read-only','--user','0:0','--mount',f'type=volume,source={volume},target=/work','--entrypoint','node',a.image,'-e',"require('node:fs').writeFileSync('/work/witness','synthetic retained backup work',{mode:0o600})"])
    name='exhibitos-backup-'+bid;cid=helper(name,'com.exhibitos.backup',bid,[f'type=volume,source={volume},target=/work'])
    foreign=helper('exhibitos-reconciliation-foreign-'+str(uuid.uuid4()),'com.exhibitos.backup',str(uuid.uuid4()),[])
    assert inspect(cid)['State']['Running'] is True
    for args,code in [(['reconcile-helper','backup',bid,'--no-consent'],'RECONCILIATION_ACK_REQUIRED'),(['reconcile-helper','backup','../foreign','--preserve-candidates'],'RECONCILIATION_INPUT_INVALID')]:
        assert invoke(backup,*args,success=False)['code']==code
    with (backup/'operation.lock').open('r+b') as lock:
        fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
        assert invoke(backup,'reconcile-helper','backup',bid,'--preserve-candidates',success=False)['code']=='BUSY'
    for patch in [{'errorCode':None},{'createdAt':3,'updatedAt':2},{'stage':''}]:
        value=json.loads(initial[1]);value.update(patch);private(bjob,(json.dumps(value)+'\n').encode())
        assert invoke(backup,'reconcile-helper','backup',bid,'--preserve-candidates',success=False)['code']=='RECONCILIATION_TARGET_INVALID'
    private(bjob,initial[1])
    job(backup,'backup',bid,'completed');assert invoke(backup,'reconcile-helper','backup',bid,'--preserve-candidates',success=False)['code']=='RECONCILIATION_TARGET_INVALID';private(bjob,initial[1])
    assert inspect(cid)['State']['Running'] is True and not list(backup.glob('helper-reconciliation-*.json'))
    step('acknowledgement, unsafe ID, active root lock and completed job reject before helper mutation')
    docker(['rename',cid,'exhibitos-reconciliation-renamed-'+bid])
    try:assert invoke(backup,'reconcile-helper','backup',bid,'--preserve-candidates',success=False)['code']=='OWNERSHIP_CONFLICT';assert inspect(cid)['State']['Running'] is True
    finally:docker(['rename',cid,name])
    step('label/name conflict remains running and failed reconciliation never claims success')
    duplicate=helper('exhibitos-reconciliation-duplicate-'+bid,'com.exhibitos.backup',bid,[])
    assert invoke(backup,'reconcile-helper','backup',bid,'--preserve-candidates',success=False)['code']=='OWNERSHIP_CONFLICT';assert inspect(cid)['State']['Running'] is True and inspect(duplicate)['State']['Running'] is True
    # Docker labels are immutable; the duplicate must be stopped and retained. A stopped
    # duplicate still proves ambiguous ownership, so qualify a new uniquely-owned job.
    assert inspect(duplicate)['Config']['Labels']['com.exhibitos.reconciliation.test']==batch
    docker(['stop','--time','1',duplicate]);docker(['rename',duplicate,'exhibitos-reconciliation-duplicate-retained-'+bid])
    step('duplicate matching ownership IDs reject without stopping either running helper')
    # Preserve the failed ambiguous job/helper; use a new isolated job for success.
    success_id=str(uuid.uuid4());directory(backup/('backup-creation-'+success_id));success_job=job(backup,'backup',success_id,'interrupted');before=preserved(backup,success_job)
    success_volume='exhibitos-backup-work-'+success_id;docker(['volume','create','--label','com.exhibitos.backup='+success_id,'--label','com.exhibitos.reconciliation.test='+batch,success_volume])
    docker(['run','--pull=never','--rm','--network','none','--read-only','--user','0:0','--mount',f'type=volume,source={success_volume},target=/work','--entrypoint','node',a.image,'-e',"require('node:fs').writeFileSync('/work/witness','synthetic preserved success work',{mode:0o600})"])
    success_cid=helper('exhibitos-backup-'+success_id,'com.exhibitos.backup',success_id,[f'type=volume,source={success_volume},target=/work'])
    receipt=invoke(backup,'reconcile-helper','backup',success_id,'--preserve-candidates')
    assert receipt['operation']=='helper-reconciled' and receipt['helperState']=='stopped' and receipt['targetId']==success_id and receipt['dataPreserved'] is True and receipt['writersResumed'] is False
    assert inspect(success_cid)['State']['Running'] is False and inspect(cid)['State']['Running'] is True and inspect(foreign)['State']['Running'] is True
    witness=docker(['run','--pull=never','--rm','--network','none','--read-only','--user','0:0','--mount',f'type=volume,source={success_volume},target=/work,readonly','--entrypoint','node',a.image,'-e',"process.stdout.write(require('node:fs').readFileSync('/work/witness'))"])
    assert witness=='synthetic preserved success work' and preserved(backup,success_job)==before and preserved(backup,bjob)==initial
    step('exact failed-job helper stops; original jobs, other helpers and work-volume witness are preserved')
    again=invoke(backup,'reconcile-helper','backup',success_id,'--preserve-candidates');assert again['helperState']=='stopped' and preserved(backup,success_job)==before
    history=invoke(backup,'helper-reconciliations');assert len(history)==4 and sum(j['state']=='completed' for j in history)==2 and sum(j['state']=='failed' for j in history)==2
    assert all(j['targetId'] in (bid,success_id) for j in history)
    step('repeat inspection and persisted completed/failed helper history never rewrite original backup outcomes')
    restore=base/'restore-root';directory(restore);rid=str(uuid.uuid4());work=restore/('restore-'+rid);directory(work);private(work/'witness',b'synthetic restore work preserved');private(restore/'synthetic-original',b'original synthetic bytes retained');rjob=job(restore,'restoration',rid,'interrupted');rbefore=preserved(restore,rjob)
    rcid=helper('exhibitos-restore-'+rid,'com.exhibitos.restoration',rid,[f'type=bind,source={work},target=/work'],remove=True,user=f'{os.getuid()}:{os.getgid()}')
    auxiliary=next(m['Name'] for m in inspect(rcid)['Mounts'] if m['Destination']=='/var/lib/postgresql')
    docker(['run','--pull=never','--rm','--network','none','--read-only','--user','0:0','--mount',f'type=volume,source={auxiliary},target=/var/lib/postgresql,volume-nocopy','--entrypoint','node',a.image,'-e',"require('node:fs').writeFileSync('/var/lib/postgresql/retention-witness','synthetic auxiliary data preserved',{mode:0o600})"])
    restored=invoke(restore,'reconcile-helper','restoration',rid,'--preserve-candidates')
    assert restored['kind']=='restoration' and restored['helperState']=='absent' and restored['writersResumed'] is False
    assert not docker(['ps','--all','--no-trunc','--filter','label=com.exhibitos.restoration='+rid,'--format','{{.ID}}'])
    assert (work/'witness').read_bytes()==b'synthetic restore work preserved' and preserved(restore,rjob)==rbefore
    retained=json.loads((restore/('helper-retention-'+restored['id']+'.json')).read_bytes())
    anchor=inspect(retained['containerId'])
    assert retained['targetId']==rid and anchor['State']['Status']=='created' and anchor['State']['Running'] is False
    assert anchor['HostConfig']['AutoRemove'] is False and len(anchor['Mounts'])==1
    assert anchor['Mounts'][0]['Name']==auxiliary and anchor['Mounts'][0]['RW'] is False
    assert (restore/('helper-retention-'+restored['id']+'.json')).stat().st_mode & 0o777 == 0o600
    assert json.loads(docker(['volume','inspect',auxiliary]))[0]['Name']==auxiliary
    auxiliary_witness=docker(['run','--pull=never','--rm','--network','none','--read-only','--user','0:0','--mount',f'type=volume,source={auxiliary},target=/var/lib/postgresql,readonly,volume-nocopy','--entrypoint','node',a.image,'-e',"process.stdout.write(require('node:fs').readFileSync('/var/lib/postgresql/retention-witness'))"])
    assert auxiliary_witness=='synthetic auxiliary data preserved'
    step('auto-remove helper auxiliary volume and witness persist in never-started read-only retention anchor')
    step('restore helper stops and engine auto-removes only its --rm container; original job and bind workspace persist')
    absent=invoke(restore,'reconcile-helper','restoration',rid,'--preserve-candidates');assert absent['helperState']=='absent' and preserved(restore,rjob)==rbefore
    # Simulate a crashed checker with private metadata only, then reopen through actual CLI.
    audit_id=str(uuid.uuid4());audit={'id':audit_id,'targetId':rid,'kind':'restoration','state':'checking','helperState':None,'errorCode':None,'createdAt':1,'updatedAt':2};private(restore/('helper-reconciliation-'+audit_id+'.json'),(json.dumps(audit)+'\n').encode())
    recovered=invoke(restore,'helper-reconciliations');assert any(j['id']==audit_id and j['state']=='interrupted' and j['helperState'] is None for j in recovered);assert preserved(restore,rjob)==rbefore
    step('successful scoped absence and checker-crash recovery remain distinct from restoration completion')
    after_states=states();assert all(after_states.get(cid)==state for cid,state in existing_states.items());step('every pre-existing container retains its original running/paused/restarting state')
    report={'checks':checks,'receipts':[receipt,again,restored,absent],'limits':['owned synthetic helper stops only; not full DB restoration/cancellation/GUI/Windows/Podman qualification','engine auto-removes configured --rm helper; no rm/volume/data deletion command issued','ambiguous helpers/data/volumes retained; independent new job used for positive path'],'preservedRoots':[str(backup),str(restore)],'retention':retained}
    private(base/'helper-reconciliation-report.json',(json.dumps(report,indent=2)+'\n').encode());print('Report '+str(base/'helper-reconciliation-report.json'),flush=True)
finally:
    # Stop only newly created test helpers with the exact private batch label, retaining volumes.
    for cid in created:
        r=command([a.docker,'inspect',cid],ok=False)
        if r.returncode:continue
        value=json.loads(r.stdout)[0]
        assert value['Id']==cid and value['Config']['Labels'].get('com.exhibitos.reconciliation.test')==batch
        if value['State']['Running']:docker(['stop','--time','1',cid])
