#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Actual orphan boundary on a retained synthetic test-backup-creation fixture."""
from pathlib import Path
import argparse
import json,subprocess,uuid,time,hashlib
p=argparse.ArgumentParser();p.add_argument('--fixture',required=True);p.add_argument('--manager',required=True);p.add_argument('--docker',default='docker');args=p.parse_args()
base=Path(args.fixture);assert base.is_absolute() and base.resolve()==base and base.name.startswith('exhibitos-manager-backup-create-')
root=base/'retained-source-manager';manager=Path(args.manager);docker=args.docker
old=json.loads((base/'backup-creation-report.json').read_text())
volume='exhibitos-backup-work-'+old['receipt']['id']
work=json.loads(subprocess.check_output([docker,'volume','inspect',volume],text=True))[0]
assert work['Labels']['com.exhibitos.backup']==old['receipt']['id'] and work['Driver']=='local' and not work['Options']
id=str(uuid.uuid4());workspace=root/('backup-creation-'+id);workspace.mkdir(mode=0o700)
job={'id':id,'operation':'create','state':'running','stage':'synthetic-orphan','errorCode':None,'createdAt':int(time.time()*1000),'updatedAt':int(time.time()*1000)}
path=workspace/'job.json';path.write_text(json.dumps(job));path.chmod(0o600)
container='exhibitos-backup-'+id
image=old['receipt']['image']
subprocess.run([docker,'run','--pull=never','-d','--name',container,'--label','com.exhibitos.backup='+id,'--network','none','--read-only','--cap-drop','ALL','--security-opt','no-new-privileges:true','--entrypoint','node',image,'-e','setInterval(()=>{},1000)'],check=True,capture_output=True)
try:
 result=subprocess.run([str(manager),'--root',str(root),'create-backup',image,str(base/'key.bin'),'--external-writers-quiesced'],capture_output=True,timeout=180)
 value=json.loads(result.stdout);assert result.returncode and value['code']=='BACKUP_ORPHAN_PENDING' and not result.stderr
 jobs=json.loads(subprocess.check_output([str(manager),'--root',str(root),'backup-jobs'],text=True))
 oldjob=next(v for v in jobs if v['id']==id);assert oldjob['state']=='interrupted' and oldjob['stage']=='synthetic-orphan'
 assert not (workspace/'receipt.json').exists()
 project=json.loads((root/'bundle/manifest.json').read_text())['projectName']
 states=lambda:subprocess.check_output([docker,'ps','--all','--filter','label=com.docker.compose.project='+project,'--format','{{.ID}} {{.State}}'],text=True).splitlines()
 states_before=sorted(states())
 before={name:hashlib.sha256((root/name).read_bytes()).hexdigest() for name in ['engine.json','installed.json','runtime.env','bundle/manifest.json','bundle/compose.yaml']}
 def blocked(action,code):
  result=subprocess.run([str(manager),'--root',str(root),action],capture_output=True,timeout=180)
  value=json.loads(result.stdout);assert result.returncode and value['state']=='failed' and value['errorCode']==code and not result.stderr,(action,value)
 for action in ['start','restart','install','retry']:blocked(action,'BACKUP_ORPHAN_PENDING')
 renamed=container+'-renamed'
 subprocess.run([docker,'rename',container,renamed],check=True,capture_output=True)
 try:
  blocked('start','OWNERSHIP_CONFLICT')
 finally:
  subprocess.run([docker,'rename',renamed,container],check=True,capture_output=True)
 info=json.loads(subprocess.check_output([docker,'inspect',container],text=True))[0]
 assert info['Config']['Labels']['com.exhibitos.backup']==id
 subprocess.run([docker,'stop','--time','1',container],check=True,capture_output=True)
 positive=subprocess.run([str(manager),'--root',str(root),'install'],capture_output=True,timeout=300)
 assert positive.returncode==0 and json.loads(positive.stdout)['state']=='completed' and not positive.stderr
 after={name:hashlib.sha256((root/name).read_bytes()).hexdigest() for name in before}
 assert before==after and sorted(states())==states_before
 report={'format':1,'checks':['existing actual backup work volume has exact ownership/local-driver/no-options','actual orphan helper blocks new creation before writer changes','reopened durable running job becomes interrupted, never successful','actual start/restart/install/retry refuse active helper','renamed owned helper fails closed','stopped correctly owned helper permits actual install','installed metadata/original credentials and actual service states unchanged'],'limits':['only new synthetic orphan helper; full creation preceding guard tested separately']}
 p=base/'writer-guard-proof-report.json';p.write_text(json.dumps(report,indent=2)+'\n');p.chmod(0o600)
 print('PASS actual orphan boundary and work-volume ownership; report SHA256 '+hashlib.sha256(p.read_bytes()).hexdigest())
finally:
 info=json.loads(subprocess.check_output([docker,'inspect',container],text=True))[0]
 assert info['Config']['Labels']['com.exhibitos.backup']==id
 subprocess.run([docker,'rm','--force',container],check=True,capture_output=True)
