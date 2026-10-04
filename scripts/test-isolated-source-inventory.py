# SPDX-License-Identifier: Apache-2.0
"""Qualify the isolated reader against an explicitly retained synthetic snapshot.
Not the final current-source fenced adapter; never mounts the original DB writable.
"""
import argparse,json,subprocess,uuid,hashlib
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--snapshot-result',type=Path,required=True);p.add_argument('--candidate',type=Path,required=True);a=p.parse_args()
v=json.loads(a.snapshot_result.read_text());obs=v['observation'];assert v['physicalDatabaseCopied'] and not v['preflightVerified'] and obs['proof']['cleanShutdown']
profile=a.candidate/'profile-1';root=profile/'installations'/obs['targetInstance'];job=json.loads((root/'restoration.json').read_text());assert job['state']=='completed'
manifest=root/('restore-'+job['id'])/'authenticated/manifest.json';assert hashlib.sha256(manifest.read_bytes()).hexdigest()==obs['authenticatedManifestSha256']
source=profile/'local-runtime';bundle=json.loads((source/'bundle/manifest.json').read_text());checks=[]
def docker(*args,allow=False):
 r=subprocess.run(['docker',*args],capture_output=True,timeout=180)
 if not allow:assert r.returncode==0,'Synthetic Docker operation failed'
 return r
before=docker('ps','-a','--format','{{.ID}} {{.State}}').stdout
volume=obs['snapshotVolume'];info=json.loads(docker('volume','inspect',volume).stdout)[0];nonce=volume.removeprefix('exhibitos-source-db-copy-');assert str(uuid.UUID(nonce))==nonce and info['Labels']['com.exhibitos.source.database']==nonce and info['Driver']=='local'
ids=docker('ps','-a','--filter','label=com.docker.compose.project='+bundle['projectName'],'--format','{{.ID}}').stdout.decode().split();containers=json.loads(docker('inspect',*ids).stdout)
assert len(containers)==2 and all(c['State']['Running']==False for c in containers)
app=next(c for c in containers if c['Config']['Labels']['com.docker.compose.service']=='platform')
blob=next(m['Name'] for m in app['Mounts'] if m['Destination']=='/data/blobs' and m['Type']=='volume')
image=obs['maintenanceImage'];code=Path('crates/lifecycle/src/source_inventory_reader.mjs').read_text();tag=str(uuid.uuid4());name='exhibitos-source-inventory-test-'+tag
args=['create','--pull','never','--name',name,'--label','com.exhibitos.source.inventory.test='+tag,'--network','none','--user','0:0','--read-only','--cap-drop','ALL']
for cap in ['DAC_OVERRIDE','CHOWN','SETUID','SETGID','KILL']:args+=['--cap-add',cap]
args+=['--security-opt','no-new-privileges:true','--memory','512m','--pids-limit','64','--tmpfs','/tmp:rw,nosuid,nodev,size=256m,mode=1777','--tmpfs','/var/lib/postgresql:rw,nosuid,nodev,size=1m','--mount','type=volume,source='+volume+',target=/snapshot,volume-nocopy','--mount','type=volume,source='+blob+',target=/blobs,readonly,volume-nocopy','--mount','type=bind,source='+str(manifest)+',target=/manifest.json,readonly','--env','EXHIBITOS_MANIFEST_SHA256='+obs['authenticatedManifestSha256'],'--entrypoint','node',image,'--input-type=module','-e',code]
id=docker(*args).stdout.decode().strip();r=docker('start','--attach',id,allow=True)
stopped=json.loads(docker('inspect',id).stdout)[0];assert stopped['Name']=='/'+name and stopped['Config']['Labels']['com.exhibitos.source.inventory.test']==tag and not stopped['State']['Running']
if r.returncode!=0:
 print('REFUSED',r.stderr.decode().strip());raise SystemExit(1)
result=json.loads(r.stdout);assert result['currentInventoryVerified'] and not result['preflightVerified'] and result['backupId']==obs['backupId'] and result['authenticatedManifestSha256']==obs['authenticatedManifestSha256']
assert result['inventorySha256']==v['intent']['update']['plan']['sourceInventory'];assert result['schemaSha256']==v['intent']['update']['plan']['sourceSchema']
docker('rm',id);assert docker('ps','-a','--format','{{.ID}} {{.State}}').stdout==before
out=a.candidate/'isolated-source-inventory-report.json';report={'format':1,'comparison':result,'snapshotVolume':volume,'checks':['independent native snapshot only writable; original DB not mounted','actual original DB identity/schema/tables/sequences and current original blob inventory matched authenticated backup','isolated PostgreSQL shutdown and original container states preserved'],'limits':['component reader qualification only; full borrowed-fence/current-source snapshot-repeat integration remains','snapshot DB legitimately changed by its own startup/shutdown; physical copy receipt predates that use','native PostgreSQL18/same cached maintenance image; no signed apply/migration/rollback or GUI/Windows/device gate']};out.write_text(json.dumps(report,indent=2)+'\n');out.chmod(0o600);print('PASS isolated snapshot database plus readonly original blobs matched authenticated full inventory');print('Report',out)
