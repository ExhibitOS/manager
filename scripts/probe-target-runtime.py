# SPDX-License-Identifier: Apache-2.0
"""Development target execution on new synthetic volume copies; no installation apply."""
import argparse,json,subprocess,hashlib,uuid,os,socket,time,shutil,stat
if not __debug__:
 raise RuntimeError("development assertions require Python without -O")
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--fixture',type=Path,required=True);p.add_argument('--cli',type=Path,required=True);p.add_argument('--docker',type=Path,required=True);p.add_argument('--interrupt-target',action='store_true');p.add_argument('--plan-file',type=Path);a=p.parse_args()
assert a.fixture.resolve()==a.fixture and a.fixture.is_absolute()
binding=json.loads((a.fixture/'genuine-release-binding.json').read_text());plan=binding['plan']
plan_file_proof=None
if a.plan_file:
 assert a.plan_file.is_absolute() and a.plan_file.resolve()==a.plan_file
 def unique(pairs):
  value={}
  for key,item in pairs:
   assert key not in value,'duplicate private plan field'
   value[key]=item
  return value
 def private_plan(path):
  fd=os.open(path,os.O_RDONLY|os.O_NOFOLLOW)
  try:
   m=os.fstat(fd);assert stat.S_ISREG(m.st_mode) and m.st_nlink==1 and m.st_uid==os.getuid() and stat.S_IMODE(m.st_mode) in (0o600,0o400) and m.st_size<=16384
   raw=os.read(fd,16385);after=os.fstat(fd);current=path.lstat()
   identity=lambda v:(v.st_dev,v.st_ino,v.st_size,v.st_mode,v.st_mtime_ns,v.st_ctime_ns,v.st_nlink,v.st_uid)
   assert len(raw)==m.st_size and identity(m)==identity(after)==identity(current)
   return json.loads(raw,object_pairs_hook=unique),hashlib.sha256(raw).hexdigest()
  finally:os.close(fd)
 plan,plan_hash=private_plan(a.plan_file);assert set(plan)==set(binding['plan'])
 plan_file_proof={'path':str(a.plan_file),'sha256':plan_hash,'boundToCurrentPreparedIntent':True}
def intent():
 r=subprocess.run([str(a.cli),'update-intent','--profile',str(a.fixture/'profile-1'),'--installation','default','--apps-closed'],capture_output=True,text=True,timeout=30);assert r.returncode==0;return json.loads(r.stdout)
before_intent=intent();assert before_intent['intent']['update']['plan']==plan and before_intent['intent']['update']['stage']=='prepared'
source=a.fixture/'profile-1/installations'/plan['targetInstance'];manifest=json.loads((source/'bundle/manifest.json').read_text());compose=json.loads((source/'bundle/compose.yaml').read_text());assert hashlib.sha256((source/'bundle/compose.yaml').read_bytes()).hexdigest()==manifest['composeSha256']
assert json.loads((source/'restoration.json').read_text())['state']=='completed'
root=a.fixture/('target-probe-'+str(uuid.uuid4()));root.mkdir(mode=0o700);project='exhibitos-compatibility-'+str(uuid.uuid4());label='com.exhibitos.compatibility='+project
helper='sha256:8f0e7b042ff0b93a646b919f5a8a5ee2f41cc22debcd5bd9ef49eacd06537e06'
def docker(*args):
 r=subprocess.run([str(a.docker),*args],capture_output=True,timeout=300);assert r.returncode==0,'bounded Docker command refused; private copies retained';assert len(r.stdout)+len(r.stderr)<1024*1024;return r.stdout

def states():return sorted(docker('ps','--all','--no-trunc','--format','{{.ID}} {{.State}}').decode().splitlines())
before_states=states();assert intent()==before_intent
existing=docker('ps','--all','--quiet','--filter','label=com.docker.compose.project='+manifest['projectName']).decode().split();assert existing
for cid in existing:assert not json.loads(docker('inspect',cid))[0]['State']['Running']
files=['installed.json','engine.json','runtime.env','bundle/manifest.json','bundle/compose.yaml','restoration.json'];before_files={n:hashlib.sha256((source/n).read_bytes()).hexdigest() for n in files}
assert shutil.disk_usage(root).free>=6*1024**3
copies={};source_volumes={};new_volumes=[]
copy_script=Path(__file__).with_name('compatibility-volume-copy.mjs').read_text()
for kind in ['database','objects','configuration']:
 old=manifest['projectName']+'_'+kind;volume=json.loads(docker('volume','inspect',old))[0];assert volume['Driver']=='local' and not volume.get('Options') and volume['Labels']['com.exhibitos.project']==manifest['projectName']
 source_volumes[kind]=old;new=project+'_'+kind;assert new not in docker('volume','ls','--quiet').decode().split();docker('volume','create','--label',label,new);new_volumes.append(new)
 name=project+'-copy-'+kind
 out=docker('run','--pull','never','--name',name,'--label',label,'--network','none','--read-only','--user','0:0','--cap-drop','ALL','--cap-add','DAC_OVERRIDE','--cap-add','CHOWN','--cap-add','FOWNER','--security-opt','no-new-privileges:true','--pids-limit','32','--memory','256m','--tmpfs','/var/lib/postgresql:rw,nosuid,nodev,size=1m','--mount','type=volume,src='+old+',dst=/source,readonly,volume-nocopy','--mount','type=volume,src='+new+',dst=/snapshot,volume-nocopy','--env','EXHIBITOS_COPY_KIND='+kind,'--entrypoint','node',helper,'--input-type=module','-e',copy_script)
 copies[kind]=json.loads(out);assert copies[kind]['kind']==kind
 print('PASS bounded readonly source '+kind+' copied/hash/mode/owner matched',flush=True)
# A component checkpoint is explicitly distinct from the completed probe report.
assert before_files=={n:hashlib.sha256((source/n).read_bytes()).hexdigest() for n in files}
assert intent()['intent']==before_intent['intent'] and set(before_states)<=set(states())
checkpoint=root/'inventory-checkpoint.json'
checkpoint.write_text(json.dumps({'kind':'isolated-volume-copy-checkpoint','project':project,'copies':copies,'originalFilesStatesIntentPreserved':True,'updateExecuted':False}));checkpoint.chmod(0o600)
inventories={}
def observe(phase):
 result=subprocess.run(['python3',str(Path(__file__).with_name('inspect-target-inventory.py')),'--probe',str(root),'--docker',str(a.docker),'--checkpoint'],capture_output=True,text=True,timeout=300)
 assert result.returncode==0,'step inventory failed; private helper/copies retained'
 name=next(line.removeprefix('PrivateReport ') for line in result.stdout.splitlines() if line.startswith('PrivateReport '))
 path=Path(name);assert path.parent==root
 value=json.loads(path.read_text());assert not value['inventory']['issues']
 inventories[phase]={'path':name,'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'value':value}
 print('PASS full stopped-copy inventory '+phase,flush=True)
 return value
baseline=observe('before_target')
def compare(previous,current):
 assert previous['identity']==current['identity']
 left=previous['inventory'];right=current['inventory']
 for key in ['schemaVersion','schemaDigest','migrations','objects','references','issues']:assert left[key]==right[key],key
 old={t['name']:t for t in left['tables']};new={t['name']:t for t in right['tables']};assert old.keys()==new.keys()
 for name in old:
  if name!='auth_sessions':assert old[name]==new[name],name
 assert new['auth_sessions']['rowCount']==old['auth_sessions']['rowCount']+1
 assert len(current['authSessions'])==len(previous['authSessions'])+1
 assert len(set(current['authSessions']))==len(current['authSessions'])
 assert set(previous['authSessions'])<set(current['authSessions'])

with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
env=(source/'runtime.env').read_text();lines=env.splitlines();lines=[('EXHIBITOS_PORT='+str(port) if line.startswith('EXHIBITOS_PORT=') else line) for line in lines];(root/'runtime.env').write_text('\n'.join(lines)+'\n');(root/'runtime.env').chmod(0o600)
for v in compose['services'].values():v['labels']={'com.exhibitos.compatibility':project};v['restart']='no'
compose['services']['platform']['image']='sha256:'+plan['targetImage'];compose['services']['platform']['env_file']=['runtime.env']
compose['networks']['default']['labels']={'com.exhibitos.compatibility':project}
compose['volumes']={k:{'external':True,'name':project+'_'+k} for k in copies}
(root/'compose.json').write_text(json.dumps(compose));(root/'compose.json').chmod(0o600)
cmd=['compose','--env-file',str(root/'runtime.env'),'--project-name',project,'--file',str(root/'compose.json')]
url='http://127.0.0.1:'+str(port);ready=None;checks=[]
try:
 docker(*cmd,'up','--detach','--pull','never')
 import urllib.request,http.cookiejar
 for n in range(120):
  try:
   with urllib.request.urlopen(url+'/api/v1/readiness',timeout=2) as response:ready=json.load(response)
   if ready.get('ready'):break
  except Exception:pass
  time.sleep(1)
 assert ready and ready['ready'] and ready['protocolVersion']=='1'
 app=docker(*cmd,'ps','--quiet','platform').decode().strip();db=docker(*cmd,'ps','--quiet','database').decode().strip();obs=json.loads(docker('inspect',app))[0];assert obs['Image']=='sha256:'+plan['targetImage'] and obs['RestartCount']==0
 witness=docker('exec',db,'psql','-U','exhibitos','-d','exhibitos','-Atc','SELECT witness FROM synthetic_manager_backup WHERE id=1').decode().strip();assert witness=='Manager backup original data'
 blob=docker('exec',app,'node','--input-type=module','-e',"import{readFile}from'node:fs/promises';process.stdout.write(await readFile('/data/blobs/synthetic/manager-proof'))");assert blob==b'synthetic Manager blob'
 settings=dict(line.split('=',1) for line in lines if '=' in line)
 request=urllib.request.Request(url+'/api/v1/auth/login',data=json.dumps({'subject':settings['ADMIN_SUBJECT'],'password':settings['ADMIN_PASSWORD'],'tenantId':settings['TENANT_ID']}).encode(),headers={'Content-Type':'application/json','Origin':url},method='POST')
 with urllib.request.urlopen(request,timeout=5) as response:assert response.status==200 and response.headers.get('Set-Cookie')
 with urllib.request.urlopen(url,timeout=5) as response:assert response.status==200 and len(response.read())>100
 checks=['actual target image/protocol readiness with zero restarts','synthetic existing database witness and blob preserved in copies','preserved administrator login/web on target Runtime']
 print('PASS actual target Runtime readiness/data/login/web on independent copies',flush=True)
 if a.interrupt_target:
  # Only exact observed containers in this new synthetic project may be killed.
  for cid,service in [(app,'platform'),(db,'database')]:
   owned=json.loads(docker('inspect',cid))[0]
   assert owned['Config']['Labels']['com.docker.compose.project']==project
   assert owned['Config']['Labels']['com.docker.compose.service']==service
   assert owned['Config']['Labels']['com.exhibitos.compatibility']==project
  docker('kill','--signal','KILL',app)
  killed=json.loads(docker('inspect',app))[0];assert not killed['State']['Running'] and killed['State']['ExitCode']==137
  docker('start',app)
  def recovered():
   for n in range(120):
    try:
     with urllib.request.urlopen(url+'/api/v1/readiness',timeout=2) as response:value=json.load(response)
     if value.get('ready') and value.get('protocolVersion')=='1':return
    except Exception:pass
    time.sleep(1)
   raise RuntimeError('isolated Runtime interruption recovery failed')
  recovered()
  docker(*cmd,'stop','--timeout','30','platform')
  docker('kill','--signal','KILL',db)
  killed_db=json.loads(docker('inspect',db))[0];assert not killed_db['State']['Running'] and killed_db['State']['ExitCode']==137
  docker('start',db);docker('start',app);recovered()
  assert docker('exec',db,'psql','-U','exhibitos','-d','exhibitos','-Atc','SELECT witness FROM synthetic_manager_backup WHERE id=1').decode().strip()=='Manager backup original data'
  checks.append('explicit target process and copied PostgreSQL SIGKILL observed137; manual same-container restart/readiness/witness recovered')
  print('PASS actual isolated target and database abrupt interruption/restart',flush=True)

 # Preserve the target container; add a separate previous-version container rather
 # than replacing an installed runtime. Only the copied database is shared.
 docker(*cmd,'stop','--timeout','30')
 target_inventory=observe('after_target_login');compare(baseline,target_inventory)
 docker(*cmd,'up','--detach','--no-deps','--pull','never','database')
 (root/'compose-target.json').write_text(json.dumps(compose));(root/'compose-target.json').chmod(0o600)
 previous=json.loads(json.dumps(compose['services']['platform']));previous['image']='sha256:'+plan['sourceImage'];compose['services']['previous-platform']=previous
 (root/'compose.json').write_text(json.dumps(compose))
 docker(*cmd,'up','--detach','--no-deps','--pull','never','previous-platform')
 old_ready=None
 for n in range(120):
  try:
   with urllib.request.urlopen(url+'/api/v1/readiness',timeout=2) as response:old_ready=json.load(response)
   if old_ready.get('ready'):break
  except Exception:pass
  time.sleep(1)
 assert old_ready and old_ready['ready'] and old_ready['protocolVersion']=='1'
 oldapp=docker(*cmd,'ps','--quiet','previous-platform').decode().strip();oldobs=json.loads(docker('inspect',oldapp))[0];assert oldobs['Image']=='sha256:'+plan['sourceImage'] and oldobs['RestartCount']==0
 assert docker('exec',db,'psql','-U','exhibitos','-d','exhibitos','-Atc','SELECT witness FROM synthetic_manager_backup WHERE id=1').decode().strip()=='Manager backup original data'
 with urllib.request.urlopen(request,timeout=5) as response:assert response.status==200 and response.headers.get('Set-Cookie')
 with urllib.request.urlopen(url,timeout=5) as response:assert response.status==200
 checks.append('previous Runtime readiness/witness/login/web after target startup/login writes on copies')
 print('PASS previous Runtime reads target-used copied data and login/web',flush=True)

finally:
 docker(*cmd,'stop','--timeout','30')
previous_inventory=observe('after_previous_login');compare(target_inventory,previous_inventory)
checks.append('full identity/schema/migrations/49non-session entries/blobs/references preserved; each login adds one unchanged-existing session row')
assert before_files=={n:hashlib.sha256((source/n).read_bytes()).hexdigest() for n in files};assert intent()['intent']==before_intent['intent']
after=set(states());assert set(before_states)<=after
for cid in existing:assert not json.loads(docker('inspect',cid))[0]['State']['Running']
if a.plan_file:
 final_plan,final_hash=private_plan(a.plan_file);assert final_plan==plan and final_hash==plan_file_proof['sha256']
report={'format':1,'plan':plan,'planFileProof':plan_file_proof,'checks':checks,'targetImage':plan['targetImage'],'targetSchemaDeclaration':plan['targetSchema'],'project':project,'copies':copies,'inventoryObservations':inventories,'readiness':ready,'interruptionInjected':a.interrupt_target,'originalFilesStatesIntentPreserved':True,'updateExecuted':False,'preflightVerified':False,'limits':['isolated development target probe; not registered target health receipt or installation apply','full inventory observed before/after each Runtime login; failures/full corpus and actual installation rollback remain unqualified','new copied DB changes during target startup/login; original volumes readonly during copy and source stays stopped','all private files/helper containers/copied volumes retained; no production trust/data/volume mutation']}
(root/'report.json').write_text(json.dumps(report,indent=2)+'\n');(root/'report.json').chmod(0o600);print('Report '+str(root/'report.json'),flush=True)
