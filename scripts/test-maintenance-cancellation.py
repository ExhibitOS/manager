#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Actual in-flight CLI recovery cancellation of a new synthetic owned helper.
No original data/backup deletion; retained candidates and volumes are not pruned.
"""
import argparse,hashlib,json,os,socket,subprocess,tempfile,time,uuid
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--mode',choices=['confirmed','crash','ownership-conflict'],default='confirmed');p.add_argument('--kind',choices=['backup','restoration'],default='restoration');p.add_argument('--manager',required=True);p.add_argument('--fixture',required=True);p.add_argument('--docker',default='docker');a=p.parse_args()
base=Path(tempfile.mkdtemp(prefix='exhibitos-manager-active-cancel-',dir='/private/tmp')).resolve();base.chmod(0o700);root=base/'new-manager';root.mkdir(mode=0o700)
fixture=Path(a.fixture).resolve();report=json.loads((fixture/'backup-creation-report.json').read_bytes());image=report['receipt']['image'];original=fixture/'retained-source-manager';key=fixture/'key.bin';archive=original/('backup-creation-'+report['receipt']['id'])/'archive'
def digest(path):
 h=hashlib.sha256()
 with path.open('rb') as f:
  for b in iter(lambda:f.read(1048576),b''):h.update(b)
 return h.hexdigest()
def hashes(path):return {str(f.relative_to(path)):digest(f) for f in path.rglob('*') if f.is_file()}
def command(args,ok=True):
 r=subprocess.run(args,capture_output=True,timeout=180)
 if ok:assert r.returncode==0,'Owned synthetic command failed; candidates retained'
 return r
def docker(*args):return command([a.docker,*args]).stdout
def invoke(action,*args,ok=True):
 r=command([a.manager,'--root',str(root),action,*args],False);assert not r.stderr;v=json.loads(r.stdout);assert (r.returncode==0)==ok,(action,v.get('code','unknown') if isinstance(v,dict) else 'invalid');return v
checks=[]
def step(v):checks.append(v);print('PASS '+v,flush=True)
source_before=hashes(archive);key_before=digest(key);protected_before={name:digest(original/name) for name in ['runtime.env','installed.json','engine.json','bundle/manifest.json','bundle/compose.yaml']};source_project=json.loads((original/'bundle/manifest.json').read_bytes())['projectName']
def source_states():return docker('ps','--all','--no-trunc','--filter','label=com.docker.compose.project='+source_project,'--format','{{.ID}} {{.State}}')
states_before=source_states();process=None;cid=None;job_id=None;started=False
if a.kind=='backup':root=original
try:
 if a.kind=='restoration':
  assert invoke('maintenance-context') is None;assert not(root/'maintenance-cancel.lock').exists()
  step('read-only cancellation context leaves fresh destination eligible')
 else:
  assert all(line.endswith('exited') for line in states_before.decode().splitlines())
  invoke('start');started=True
  step('start only retained synthetic source for new backup; original archive and external key retained')
 with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
 operation=['restore-backup',image,str(key),str(archive),str(port),'--fresh-installation'] if a.kind=='restoration' else ['create-backup',image,str(key),'--external-writers-quiesced']
 process=subprocess.Popen([a.manager,'--root',str(root),*operation],stdout=subprocess.PIPE,stderr=subprocess.PIPE)
 deadline=time.monotonic()+90
 while time.monotonic()<deadline:
  if process.poll() is not None:
   out,errors=process.communicate();assert not errors
   safe=json.loads(out);raise AssertionError('Operation ended before helper observation: '+str(safe.get('code',safe.get('errorCode',safe.get('operation','unknown')))))
  try:
   active_id=json.loads((root/'maintenance-active.json').read_bytes());job=json.loads((root/('maintenance-'+active_id+'.json')).read_bytes());job_id=job['id']
   ids=docker('ps','--no-trunc','--filter','label=com.exhibitos.'+a.kind+'='+job_id,'--format','{{.ID}}').decode().splitlines()
   if ids:
    assert len(ids)==1;cid=ids[0];v=json.loads(docker('inspect',cid))[0]
    assert v['Name']==('/exhibitos-restore-' if a.kind=='restoration' else '/exhibitos-backup-')+job_id and v['Config']['Labels']['com.exhibitos.'+a.kind]==job_id and v['Image']==image and v['State']['Running'] is True
    # Freeze only this new synthetic Node PID to make the active helper checkpoint deterministic.
    docker('kill','--signal','STOP',cid);break
  except FileNotFoundError:pass
  time.sleep(0.05)
 assert cid and job_id and process.poll() is None
 auxiliary_path=root/('restoration-aux-'+job_id+'.json') if a.kind=='restoration' else root/('backup-creation-'+job_id)/'job.json'
 auxiliary_bytes=auxiliary_path.read_bytes();volume=json.loads(auxiliary_bytes)['volume'] if a.kind=='restoration' else 'exhibitos-backup-work-'+job_id
 target='/var/lib/postgresql' if a.kind=='restoration' else '/work'
 docker('exec','--user','0:0',cid,'node','-e',f"require('node:fs').writeFileSync('{target}/cancel-witness','synthetic candidate retained',{{mode:0o600}})")
 step('actual running authentication helper observed by exact ID/image/job; new candidate volume witness written')
 assert invoke('cancel-maintenance',a.kind,job_id,'--no-consent',ok=False)['code']=='CANCEL_ACK_REQUIRED'
 assert invoke('cancel-maintenance','backup' if a.kind=='restoration' else 'restoration',job_id,'--preserve-candidates',ok=False)['code']=='CANCEL_TARGET_INVALID'
 assert invoke('cancel-maintenance',a.kind,str(uuid.uuid4()),'--preserve-candidates',ok=False)['code']=='CANCEL_TARGET_INVALID'
 if a.mode=='ownership-conflict':docker('rename',cid,'exhibitos-cancel-conflict-'+job_id)
 requested=invoke('cancel-maintenance',a.kind,job_id,'--preserve-candidates');assert requested['state']=='requested'
 step('request needs acknowledgement and exact active kind/UUID; response remains requested rather than confirmed')
 if a.mode=='crash':
  assert process.poll() is None;process.kill()
 stdout,stderr=process.communicate(timeout=150);assert process.returncode!=0 and not stderr
 expected_state={'confirmed':'confirmed','crash':'interrupted','ownership-conflict':'uncertain'}[a.mode]
 expected_code={'confirmed':'CANCELLED','crash':'INTERRUPTED','ownership-conflict':'CANCEL_UNCERTAIN'}[a.mode]
 if a.mode!='crash':assert json.loads(stdout)['code']==expected_code
 context=invoke('maintenance-context');assert context['state']==expected_state and context['errorCode']==expected_code
 job=invoke('restoration-status') if a.kind=='restoration' else next(j for j in invoke('backup-jobs') if j['id']==job_id)
 assert job['id']==job_id and job['state']==('failed' if a.mode=='ownership-conflict' else 'interrupted') and job['errorCode']==expected_code
 assert not(root/(('restore-' if a.kind=='restoration' else 'backup-creation-')+job_id)/'receipt.json').exists()
 if a.kind=='restoration':assert not(root/'installed.json').exists()
 if a.mode=='ownership-conflict':
  assert json.loads(docker('inspect',cid))[0]['State']['Running'] is True
  docker('rename',cid,('exhibitos-restore-' if a.kind=='restoration' else 'exhibitos-backup-')+job_id)
 if a.mode!='confirmed':
  job_path=root/'restoration.json' if a.kind=='restoration' else root/('backup-creation-'+job_id)/'job.json'
  job_bytes=job_path.read_bytes();reconciled=invoke('reconcile-helper',a.kind,job_id,'--preserve-candidates')
  assert reconciled['helperState'] in ['absent','stopped'] and job_path.read_bytes()==job_bytes
  assert invoke('maintenance-context')['state']==expected_state
 if a.kind=='restoration':assert not docker('ps','--all','--no-trunc','--filter','label=com.exhibitos.'+a.kind+'='+job_id,'--format','{{.ID}}').strip()
 else:assert json.loads(docker('inspect',cid))[0]['State']['Running'] is False
 assert (a.kind=='backup' or auxiliary_path.read_bytes()==auxiliary_bytes) and auxiliary_path.stat().st_mode&0o777==0o600
 witness=docker('run','--pull=never','--rm','--network','none','--read-only','--user','0:0','--mount',f'type=volume,source={volume},target={target},readonly,volume-nocopy','--entrypoint','node',image,'-e',f"process.stdout.write(require('node:fs').readFileSync('{target}/cancel-witness'))")
 assert witness==b'synthetic candidate retained'
 step('actual '+a.mode+' outcome and guarded stop preserve private journal/volume witness without success receipt or false confirmation')
 assert invoke('cancel-maintenance',a.kind,job_id,'--preserve-candidates',ok=False)['code']=='CANCEL_TARGET_INVALID'
 if a.kind=='restoration':assert invoke('start',ok=False)['errorCode']=='RESTORE_RECOVERY_REQUIRED'
 else:
  ids=docker('ps','--all','--no-trunc','--filter','label=com.docker.compose.project='+source_project,'--format','{{.ID}}').decode().splitlines()
  values=[json.loads(docker('inspect',i))[0] for i in ids]
  assert next(v for v in values if v['Config']['Labels']['com.docker.compose.service']=='database')['State']['Running'] is True
  assert next(v for v in values if v['Config']['Labels']['com.docker.compose.service']=='platform')['State']['Running'] is False
  invoke('stop');started=False
 step('late request rejects; no automatic writer resumption; reopened journal preserves the original terminal outcome')
 assert hashes(archive)==source_before and digest(key)==key_before and source_states()==states_before and all(digest(original/name)==h for name,h in protected_before.items())
 step('original encrypted source/key and original service states remain unchanged')
 output={'format':1,'checks':checks,'mode':a.mode,'context':context,'job':job,'helperId':cid,'auxiliaryVolume':volume,'preservedRoot':str(root),'limits':['actual synthetic '+a.kind+' helper cancellation, not native GUI/Windows/Podman/cold-engine/full corpus','original source credentials/data preserved; all candidates and volumes retained','SIGSTOP controlled only new test helper Node PID; product exact-owned stop verified']}
 path=base/'cancellation-report.json';path.write_text(json.dumps(output,indent=2)+'\n');path.chmod(0o600);print('Report '+str(path),flush=True);print('SHA256 '+digest(path),flush=True)
finally:
 if cid and job_id:
  r=command([a.docker,'inspect',cid],False)
  if r.returncode==0:
   v=json.loads(r.stdout)[0]
   if v['Config']['Labels'].get('com.exhibitos.'+a.kind)==job_id and v['State']['Running']:
    command([a.docker,'kill','--signal','CONT',cid],False);command([a.docker,'stop','--time','30',cid],False)
 if process and process.poll() is None:
  try:process.communicate(timeout=120)
  except subprocess.TimeoutExpired:process.terminate();process.communicate(timeout=30)

 if started:invoke('stop')

 if a.kind=='restoration' and (root/'bundle/manifest.json').exists():
  candidate=json.loads((root/'bundle/manifest.json').read_bytes())
  assert candidate['projectName']!=source_project
  invoke('stop')
