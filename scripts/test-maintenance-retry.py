#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Actual crash/owned-helper fencing and new-job retry; all candidate data retained."""
import argparse,hashlib,json,socket,subprocess,tempfile,time,uuid
from pathlib import Path
from urllib.request import Request,urlopen
p=argparse.ArgumentParser();p.add_argument('--pre-child-crash',action='store_true');p.add_argument('--reuse-backup-target');p.add_argument('--kind',choices=['backup','restoration'],required=True);p.add_argument('--manager',required=True);p.add_argument('--fixture',required=True);p.add_argument('--docker',default='docker');a=p.parse_args()
base=Path(tempfile.mkdtemp(prefix='exhibitos-manager-maintenance-retry-',dir='/private/tmp')).resolve();base.chmod(0o700);fixture=Path(a.fixture).resolve();proof=json.loads((fixture/'backup-creation-report.json').read_bytes());image=proof['receipt']['image'];original=fixture/'retained-source-manager';key=fixture/'key.bin';archive=original/('backup-creation-'+proof['receipt']['id'])/'archive';old=original if a.kind=='backup' else base/'failed-manager';dst=base/'fresh-manager';dst.mkdir(mode=0o700)
if a.kind=='restoration':old.mkdir(mode=0o700)
def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def hashes(path):return {str(f.relative_to(path)):digest(f) for f in path.rglob('*') if f.is_file()}
def command(args,ok=True,timeout=900):
 r=subprocess.run([str(x) for x in args],capture_output=True,timeout=timeout)
 if ok:assert r.returncode==0,'Owned synthetic command failed; all candidates retained'
 return r
def docker(*args):return command([a.docker,*args]).stdout
def invoke(root,action,*args,ok=True):
 r=command([a.manager,'--root',root,action,*args],False);assert not r.stderr;v=json.loads(r.stdout);assert (r.returncode==0)==ok,(action,v.get('code',v.get('errorCode','unknown')) if isinstance(v,dict) else 'invalid');return v
def port():
 with socket.socket() as s:s.bind(('127.0.0.1',0));return s.getsockname()[1]
checks=[]
def step(v):checks.append(v);print('PASS '+v,flush=True)
source_project=json.loads((original/'bundle/manifest.json').read_bytes())['projectName']
def states():return docker('ps','--all','--no-trunc','--filter','label=com.docker.compose.project='+source_project,'--format','{{.ID}} {{.State}}')
before_states=states();assert before_states and all(l.endswith('exited') for l in before_states.decode().splitlines());before_archive=hashes(archive);before_key=digest(key);protected={n:digest(original/n) for n in ['runtime.env','installed.json','engine.json','bundle/manifest.json','bundle/compose.yaml']};worker=None;cid=None;job_id=None;renamed=False;started=False
try:
 if a.reuse_backup_target:
  assert a.kind=='backup' and str(uuid.UUID(a.reuse_backup_target))==a.reuse_backup_target
  manifest=json.loads((original/'bundle/manifest.json').read_bytes());compose=['compose','--env-file',str(original/'runtime.env'),'--project-name',manifest['projectName'],'--file',str(original/'bundle/compose.yaml')];db=docker(*compose,'ps','--all','--quiet','database').decode().strip();value=json.loads(docker('inspect',db))[0];assert value['Config']['Labels']['com.exhibitos.project']==source_project and value['Config']['Labels']['com.docker.compose.service']=='database'
  docker('start',db);started=True
  deadline=time.monotonic()+60
  while time.monotonic()<deadline:
   if json.loads(docker('inspect',db))[0]['State'].get('Health',{}).get('Status')=='healthy':break
   time.sleep(.2)
  else:raise AssertionError('Owned synthetic database health did not recover')
 elif a.kind=='backup':invoke(original,'start');started=True
 operation=['create-backup',image,key,'--external-writers-quiesced'] if a.kind=='backup' else ['restore-backup',image,key,archive,port(),'--fresh-installation']
 if a.reuse_backup_target:
  job_id=a.reuse_backup_target;ids=docker('ps','--all','--no-trunc','--filter','label=com.exhibitos.backup='+job_id,'--format','{{.ID}}').decode().splitlines();assert len(ids)==1;cid=ids[0];assert json.loads(docker('inspect',cid))[0]['State']['Running'] is False
 else:
  worker=subprocess.Popen([str(v) for v in [a.manager,'--root',old,*operation]],stdout=subprocess.PIPE,stderr=subprocess.PIPE)
  deadline=time.monotonic()+100
  while time.monotonic()<deadline:
   if worker.poll() is not None:
    out,errors=worker.communicate();assert not errors;v=json.loads(out);raise AssertionError('Original synthetic attempt ended before observation: '+str(v.get('code','unknown')))
   try:
    job_id=json.loads((old/'maintenance-active.json').read_bytes())
    ids=docker('ps','--no-trunc','--filter','label=com.exhibitos.'+a.kind+'='+job_id,'--format','{{.ID}}').decode().splitlines()
    if ids:
     assert len(ids)==1;cid=ids[0];value=json.loads(docker('inspect',cid))[0];assert value['Image']==image and value['State']['Running'] is True
     docker('kill','--signal','STOP',cid);break
   except FileNotFoundError:pass
   time.sleep(.05)
  assert cid and job_id and worker.poll() is None
 auxiliary=json.loads((old/('restoration-aux-'+job_id+'.json')).read_bytes()) if a.kind=='restoration' else None
 volume=auxiliary['volume'] if auxiliary else 'exhibitos-backup-work-'+job_id;volume_target='/var/lib/postgresql' if auxiliary else '/work'
 if worker:
  docker('exec','--user','0:0',cid,'node','-e',f"require('node:fs').writeFileSync('{volume_target}/retry-witness','candidate data preserved',{{mode:0o600}})")
  worker.kill();out,errors=worker.communicate(timeout=30);assert not errors and worker.returncode!=0
 invoke(old,'maintenance-context');job_path=old/'restoration.json' if a.kind=='restoration' else old/('backup-creation-'+job_id)/'job.json';old_bytes=job_path.read_bytes();assert json.loads(old_bytes)['state']=='interrupted';initial_running=json.loads(docker('inspect',cid))[0]['State']['Running'];assert initial_running==(worker is not None)
 step('actual worker crash leaves interrupted journal and a live paused exact-owned helper with volume witness' if worker else 'retained actual interrupted backup job and stopped helper reused; only owned database started with platform writer kept stopped')
 retry_args=[job_id,image,key,'--preserve-candidates','--external-writers-quiesced'] if a.kind=='backup' else [job_id,dst,image,key,archive,port(),'--preserve-candidates','--fresh-installation'];action='retry-'+a.kind
 initial_history=invoke(old,'maintenance-retries')
 no_ack=list(retry_args);no_ack[-2]='--no-consent';assert invoke(old,action,*no_ack,ok=False)['code']=='RETRY_ACK_REQUIRED';assert invoke(old,'maintenance-retries')==initial_history
 wrong=list(retry_args);wrong[0]=str(uuid.uuid4());invoke(old,action,*wrong,ok=False);assert invoke(old,'maintenance-retries')==initial_history;assert job_path.read_bytes()==old_bytes
 step('missing preserve acknowledgement and wrong target UUID refuse retry and keep original journal')
 docker('rename',cid,'exhibitos-retry-conflict-'+job_id);renamed=True
 assert invoke(old,action,*retry_args,ok=False)['code']=='OWNERSHIP_CONFLICT';assert json.loads(docker('inspect',cid))[0]['State']['Running']==initial_running;assert job_path.read_bytes()==old_bytes
 history=[v for v in invoke(old,'maintenance-retries') if v['id'] not in {h['id'] for h in initial_history}];assert len(history)==1 and history[0]['state']=='failed' and history[0]['newJobId'] is None
 docker('rename',cid,('exhibitos-backup-' if a.kind=='backup' else 'exhibitos-restore-')+job_id);renamed=False
 step('real name/label ownership conflict never stops helper or starts a child; safe failed retry record retained')
 if a.pre_child_crash:
  assert a.kind=='restoration' and initial_running is True
  known={v['id'] for v in invoke(old,'maintenance-retries')}
  worker=subprocess.Popen([str(v) for v in [a.manager,'--root',old,action,*retry_args]],stdout=subprocess.PIPE,stderr=subprocess.PIPE)
  deadline=time.monotonic()+30;pending=None
  while time.monotonic()<deadline:
   assert worker.poll() is None,'Retry ended before pre-child observation'
   for path in old.glob('maintenance-retry-*.json'):
    v=json.loads(path.read_bytes())
    if v['id'] not in known and v['state']=='preparing' and v['newJobId'] is None and not (old/('retry-child-intent-'+v['id']+'.json')).exists():pending=v;break
   if pending:break
   time.sleep(.005)
  assert pending and worker.poll() is None
  worker.kill();out,errors=worker.communicate(timeout=30);assert not errors and worker.returncode!=0
  assert not (old/('retry-child-intent-'+pending['id']+'.json')).exists()
  parent_bytes=(old/('maintenance-retry-'+pending['id']+'.json')).read_bytes()
  proof_recovery=invoke(old,'diagnose-retry',pending['id'],dst);assert proof_recovery['outcome']=='no-child-created' and proof_recovery['canReconcile']
  assert (old/('maintenance-retry-'+pending['id']+'.json')).read_bytes()==parent_bytes and job_path.read_bytes()==old_bytes
  recovered=invoke(old,'reconcile-retry',pending['id'],dst,'--preserve-candidates');assert recovered['newJobId'] is None and recovered['dataPreserved']
  clearance=json.loads((old/('retry-clearance-'+pending['id']+'-'+recovered['diagnosisId']+'.json')).read_bytes());assert clearance['auditSha256']==digest(old/('maintenance-retry-'+pending['id']+'.json'))
  # The dead worker's already-dispatched helper-stop can finish; current guarded reconciliation confirms exact identity and stop again.
  invoke(old,'reconcile-helper','restoration',job_id,'--preserve-candidates')
  step('actual retry worker killed before child reservation; pure diagnosis and durable clearance restore explicit retry eligibility with original journal preserved')
 receipt=invoke(old,action,*retry_args);assert receipt['targetId']==job_id and receipt['dataPreserved'] is True and receipt['newJobId']!=job_id and receipt['result']['id']==receipt['newJobId']
 assert job_path.read_bytes()==old_bytes;history=invoke(old,'maintenance-retries');record=next(v for v in history if v['id']==receipt['id']);assert record['state']=='completed' and record['newJobId']==receipt['newJobId'] and record['originalJobSha256']==hashlib.sha256(old_bytes).hexdigest()
 status=command([a.docker,'inspect',cid],False)
 if status.returncode==0:assert json.loads(status.stdout)[0]['State']['Running'] is False
 preparation_path=old/('retry-preparation-'+receipt['id']+'.json');preparation=json.loads(preparation_path.read_bytes());reservation=json.loads((old/('retry-child-intent-'+receipt['id']+'.json')).read_bytes());assert preparation['formatVersion']==1 and preparation['targetId']==job_id and preparation['originalJobSha256']==hashlib.sha256(old_bytes).hexdigest() and reservation['formatVersion']==1 and reservation['retryId']==receipt['id'] and reservation['jobId']==receipt['newJobId'] and reservation['preparationSha256']==digest(preparation_path)
 witness=docker('run','--pull=never','--rm','--network','none','--read-only','--user','0:0','--mount',f'type=volume,source={volume},target={volume_target},readonly,volume-nocopy','--entrypoint','node',image,'-e',f"process.stdout.write(require('node:fs').readFileSync('{volume_target}/retry-witness'))");assert witness==b'candidate data preserved'
 step('guarded old helper stop and preserved candidate witness precede new UUID operation; durable child reservation and exact preparation hash verified')
 if a.kind=='restoration':
  result=receipt['result'];assert result['operation']=='restored-and-running' and result['backupId']==proof['receipt']['backupId']
  m=json.loads((dst/'bundle/manifest.json').read_bytes());assert m['projectName']!=source_project
  compose=['compose','--env-file',str(dst/'runtime.env'),'--project-name',m['projectName'],'--file',str(dst/'bundle/compose.yaml')]
  db=docker(*compose,'ps','--all','--quiet','database').decode().strip();app=docker(*compose,'ps','--all','--quiet','platform').decode().strip()
  assert docker('exec',db,'psql','-U','exhibitos','-d','exhibitos','-Atc','SELECT witness FROM synthetic_manager_backup WHERE id=1').decode().strip()=='Manager backup original data'
  assert docker('exec',app,'node','-e',"process.stdout.write(require('node:fs').readFileSync('/data/blobs/synthetic/manager-proof'))")==b'synthetic Manager blob'
  env=dict(l.split('=',1) for l in (original/'runtime.env').read_text().splitlines());login={k:env[v] for k,v in [('subject','ADMIN_SUBJECT'),('password','ADMIN_PASSWORD'),('tenantId','TENANT_ID')]};url=result['openUrl']
  with urlopen(Request(url+'/api/v1/auth/login',data=json.dumps(login).encode(),headers={'Content-Type':'application/json','Origin':url},method='POST'),timeout=15) as response:assert response.status==200 and response.headers.get('Set-Cookie')
  with urlopen(url,timeout=15) as response:assert response.status==200 and len(response.read())>100
  invoke(dst,'stop');assert invoke(dst,'status')['state']=='stopped'
  step('fresh restored candidate reaches readiness with original database/blob/admin login/web, then actual stop succeeds')
 else:
  result=receipt['result'];assert result['operation']=='created-and-authenticated' and result['writersPaused'] is True
  new_archive=old/('backup-creation-'+result['id'])/'archive';assert new_archive.is_dir() and (new_archive/'complete.json').exists();assert invoke(old,'maintenance-context')['state']=='completed'
  invoke(original,'stop');started=False
  step('new backup is encrypted/authenticated with separate archive and does not resume original writers')
 assert hashes(archive)==before_archive and digest(key)==before_key and all(digest(original/n)==h for n,h in protected.items()) and states()==before_states
 step('original encrypted source/key/installation metadata and prior service identities/states preserved')
 output={'format':1,'kind':a.kind,'reusedInterruptedTarget':a.reuse_backup_target,'checks':checks,'retry':receipt,'retainedRoot':str(old),'newRoot':str(dst),'limits':['Actual synthetic Docker CLI, no native GUI/Windows/Podman/cold-engine/full corpus qualification','All old/new candidate volumes, journals, keys and archives retained; no production dataset','Paused only this newly owned helper; only tracked original worker process killed']}
 path=base/'retry-report.json';path.write_text(json.dumps(output,indent=2)+'\n');path.chmod(0o600);print('Report '+str(path));print('SHA256 '+digest(path))
finally:
 if cid and job_id:
  v=command([a.docker,'inspect',cid],False)
  if v.returncode==0:
   value=json.loads(v.stdout)[0];assert value['Config']['Labels'].get('com.exhibitos.'+a.kind)==job_id
   if renamed:docker('rename',cid,('exhibitos-backup-' if a.kind=='backup' else 'exhibitos-restore-')+job_id)
   if value['State']['Running']:command([a.docker,'kill','--signal','CONT',cid],False);command([a.docker,'stop','--time','30',cid],False)
 if worker and worker.poll() is None:worker.kill();worker.communicate(timeout=30)
 if started:invoke(original,'stop')
 if a.kind=='restoration' and (dst/'bundle/manifest.json').exists():invoke(dst,'stop')
