# SPDX-License-Identifier: Apache-2.0
"""Observe full inventory on retained isolated probe volumes; no installation apply."""
import argparse,json,subprocess,uuid,hashlib
from pathlib import Path
if not __debug__: raise RuntimeError('assertions require Python without -O')
p=argparse.ArgumentParser();p.add_argument('--probe',type=Path,required=True);p.add_argument('--docker',type=Path,required=True);a=p.parse_args()
assert a.probe.is_absolute() and a.probe.resolve()==a.probe
prior=json.loads((a.probe/'report.json').read_text());assert prior['originalFilesStatesIntentPreserved'] and not prior['updateExecuted']
project=prior['project'];assert project.startswith('exhibitos-compatibility-');uuid.UUID(project.removeprefix('exhibitos-compatibility-'))
def docker(*args):
 r=subprocess.run([str(a.docker),*args],capture_output=True,timeout=300)
 assert r.returncode==0,'inventory helper failed; retained private diagnostics';assert len(r.stdout)+len(r.stderr)<16*1024*1024
 return r.stdout
before=sorted(docker('ps','-a','--no-trunc','--format','{{.ID}} {{.State}}').decode().splitlines())
ids=docker('ps','-aq','--filter','label=com.exhibitos.compatibility='+project).decode().split();assert ids
assert all(not x['State']['Running'] for x in json.loads(docker('inspect',*ids)))
for kind in ['database','objects']:
 v=json.loads(docker('volume','inspect',project+'_'+kind))[0];assert v['Driver']=='local' and not v.get('Options') and v['Labels']['com.exhibitos.compatibility']==project
code=Path('crates/lifecycle/src/source_inventory_reader.mjs').read_text()
code=code.replace("import {main} from '/opt/exhibitos/scripts/service-backup.mjs';", "import pg from '/opt/exhibitos/node_modules/pg/lib/index.js'; const {Pool}=pg;\nimport {FileBlobStore,collectServiceInventory} from '/opt/exhibitos/packages/storage/dist/index.js';")
start=code.index(" result=await main(");end=code.index('\n}catch(e)',start)
code=code[:start]+''' const pool=new Pool({connectionString:'postgresql://exhibitos@localhost/exhibitos?host='+encodeURIComponent(socket),connectionTimeoutMillis:15000,statement_timeout:60000});
 try{
  const c=await pool.connect();
  try{
   await c.query('BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY');
   await c.query('SELECT pg_advisory_xact_lock(82002)');
   const identity=(await c.query("SELECT (pg_control_system()).system_identifier::text AS system_identifier,(SELECT oid::text FROM pg_database WHERE datname=current_database()) AS database_oid,current_database() AS database")).rows[0];
   const inventory=await collectServiceInventory(c,new FileBlobStore('/blobs'),{migrationDirectory:'/opt/exhibitos/database/migrations'});
   await c.query('ROLLBACK');result={identity,inventory};
  }finally{c.release();}
 }finally{await pool.end();}
''' +code[end:]
name=project+'-inventory-'+str(uuid.uuid4())
out=docker('run','--pull','never','--name',name,'--label','com.exhibitos.compatibility='+project,'--network','none','--user','0:0','--read-only','--cap-drop','ALL','--cap-add','DAC_OVERRIDE','--cap-add','CHOWN','--cap-add','SETUID','--cap-add','SETGID','--cap-add','KILL','--security-opt','no-new-privileges:true','--memory','512m','--pids-limit','64','--tmpfs','/tmp:rw,nosuid,nodev,size=256m,mode=1777','--tmpfs','/var/lib/postgresql:rw,nosuid,nodev,size=1m','--mount','type=volume,source='+project+'_database,target=/snapshot,volume-nocopy','--mount','type=volume,source='+project+'_objects,target=/blobs,readonly,volume-nocopy','--entrypoint','node','sha256:8f0e7b042ff0b93a646b919f5a8a5ee2f41cc22debcd5bd9ef49eacd06537e06','--input-type=module','-e',code)
result=json.loads(out);assert not json.loads(docker('inspect',name))[0]['State']['Running']
assert set(before)<=set(docker('ps','-a','--no-trunc','--format','{{.ID}} {{.State}}').decode().splitlines())
path=a.probe/('inventory-observation-'+str(uuid.uuid4())+'.json');path.write_text(json.dumps(result,indent=2)+'\n');path.chmod(0o600)
print('OBSERVED',len(result['inventory']['tables']),'tables/sequences',len(result['inventory']['objects']),'objects',len(result['inventory']['issues']),'issues')
print('PrivateReport',path)
