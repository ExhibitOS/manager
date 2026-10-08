# SPDX-License-Identifier: Apache-2.0
"""Explicit development cache import. No install/apply/trust mutation or image execution."""
import argparse,importlib.util,json,os,stat,subprocess,threading,hashlib,time,uuid
from pathlib import Path
from types import SimpleNamespace
spec=importlib.util.spec_from_file_location('oci',Path(__file__).with_name('verify-runtime-oci.py'));oci=importlib.util.module_from_spec(spec);spec.loader.exec_module(oci)
p=argparse.ArgumentParser()
for name in ['cli','docker','policy','release','archive','source-manifest','output-parent']:p.add_argument('--'+name,type=Path,required=True)
p.add_argument('--source-commit',required=True);p.add_argument('--development-cache-import',action='store_true',required=True);a=p.parse_args()
assert a.archive.is_absolute() and a.archive.resolve()==a.archive
assert a.output_parent.is_absolute() and a.output_parent.resolve()==a.output_parent
pm=a.output_parent.stat();assert stat.S_IMODE(pm.st_mode)==0o700 and pm.st_uid==os.getuid()
root=a.output_parent/('oci-import-'+str(uuid.uuid4()));root.mkdir(mode=0o700)
def command(args,stdin=None):
 proc=subprocess.Popen([str(x) for x in args],stdin=stdin if stdin is not None else subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
 chunks=[];failure=[]
 def read():
  try:
   total=0
   while True:
    b=proc.stdout.read(65536)
    if not b:break
    total+=len(b)
    if total>1024*1024:failure.append('output limit');proc.kill();break
    chunks.append(b)
  except Exception:failure.append('read failure');proc.kill()
 reader=threading.Thread(target=read,daemon=True);reader.start()
 try:code=proc.wait(timeout=300)
 except subprocess.TimeoutExpired:proc.kill();proc.wait();raise RuntimeError('command timeout; Engine import status may be uncertain, preserve cache and inspect before retry')
 reader.join(timeout=5);assert not reader.is_alive() and not failure and code==0,'bounded local command refused'
 return b''.join(chunks)
def docker(*args):return command([a.docker,*args])
def snapshot():
 ids=docker('ps','--all','--quiet','--no-trunc').decode().split();assert len(ids)<=10000
 containers=[]
 if ids:
  for v in json.loads(docker('inspect',*ids)):
   containers.append({'id':v['Id'],'image':v['Image'],'name':v['Name'],'running':v['State']['Running'],'status':v['State']['Status'],'mounts':sorted((m.get('Type'),m.get('Name',''),m.get('Source',''),m.get('Destination'),m.get('RW')) for m in v['Mounts'])})
 images=[json.loads(line) for line in docker('image','ls','--no-trunc','--format','{{json .}}').decode().splitlines()]
 tags=sorted((v['Repository'],v['Tag'],v['ID']) for v in images if v['Repository']!='<none>' and v['Tag']!='<none>')
 return {'containers':sorted(containers,key=lambda v:v['id']),'volumes':sorted(docker('volume','ls','--quiet').decode().splitlines()),'tags':tags}
def bounded_json(path,limit):
 fd=os.open(path,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK)
 with os.fdopen(fd,'rb') as f:
  m=os.fstat(f.fileno());assert stat.S_ISREG(m.st_mode) and m.st_nlink==1 and m.st_size<=limit
  b=f.read(limit+1);assert len(b)<=limit and oci.identity(m)==oci.identity(path.stat())
  return json.loads(b,object_pairs_hook=oci.unique),b,oci.identity(m)
envelope,envelope_bytes,envelope_id=bounded_json(a.release,32768)
policy,policy_bytes,policy_id=bounded_json(a.policy,12288)
release=json.loads(envelope['payload'],object_pairs_hook=oci.unique)
assert release['channel']=='development' and release['target']=='linux-arm64'
def authenticate():
 assert bounded_json(a.policy,12288)[1:]==(policy_bytes,policy_id)
 assert bounded_json(a.release,32768)[1:]==(envelope_bytes,envelope_id)
 receipt=json.loads(command([a.cli,'verify','--policy',a.policy,'--release',a.release,'--artifact',a.archive]))
 assert receipt['signatureVerified'] and receipt['artifactVerified'] and not receipt['activated']
 assert receipt['artifactSha256']==release['artifact']['sha256'] and receipt['artifactBytes']==release['artifact']['bytes']
 return receipt
fd=os.open(a.archive,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK)
with os.fdopen(fd,'rb') as f:
 identity=os.fstat(f.fileno());assert stat.S_IMODE(identity.st_mode)==0o400 and identity.st_nlink==1 and identity.st_uid==os.getuid()
 parent=a.archive.parent.stat();assert stat.S_IMODE(parent.st_mode)==0o700 and parent.st_uid==os.getuid()
 receipt=authenticate()
 proof=oci.qualify(SimpleNamespace(archive=a.archive,image='sha256:'+release['artifact']['runtimeImageSha256'],source_manifest=a.source_manifest,source_commit=a.source_commit),f)
 assert proof['artifactSha256']==release['artifact']['sha256'] and proof['artifactBytes']==release['artifact']['bytes']
 assert proof['targetSchemaSha256']==release['artifact']['schemaSha256'] and proof['version']==release['version']
 before=snapshot();existing_images=set(docker('image','ls','--all','--quiet','--no-trunc').decode().split());authenticate()
 assert oci.identity(identity)==oci.identity(os.fstat(f.fileno()))==oci.identity(a.archive.stat())
 f.seek(0);output=command([a.docker,'image','load','--quiet'],stdin=f)
 # Client timeout/failure cannot undo asynchronous daemon work. Never remove cache
 # automatically; retain diagnostic output and observe immutable identity on success.
 assert oci.identity(identity)==oci.identity(os.fstat(f.fileno()))==oci.identity(a.archive.stat())
 f.seek(0);assert hashlib.file_digest(f,'sha256').hexdigest()==release['artifact']['sha256']
 after=snapshot();current_images=set(docker('image','ls','--all','--quiet','--no-trunc').decode().split());assert existing_images<=current_images
 assert current_images-existing_images <= {'sha256:'+proof['runtimeImageSha256']}
 assert before==after,'persistent services/volume names/tag mapping changed; retain artifacts for diagnosis'
 observation=json.loads(docker('image','inspect','sha256:'+proof['runtimeImageSha256']))[0]
 assert observation['Id']=='sha256:'+proof['runtimeImageSha256'] and observation['Os']=='linux' and observation['Architecture']=='arm64'
 assert not observation.get('RepoTags') and observation['Config']['Labels']['org.opencontainers.image.revision']==a.source_commit
 assert observation['RootFS']['Layers']==proof['layerDiffIds']
 authenticate()
 report={'format':1,'operation':'development-tagless-oci-cache-import-not-activated','proof':proof,'signatureReceipt':receipt,'imported':True,'activated':False,'containersVolumesTagsPreserved':True,'imageAlreadyPresent':('sha256:'+proof['runtimeImageSha256']) in existing_images,'engineImageId':observation['Id'],'loadOutputSha256':hashlib.sha256(output).hexdigest(),'limits':['local Docker development cache only, not candidate health/apply/migration/rollback','same retained fd for structure/hash/import;0400 does not exclude same-user/privileged writers','policy snapshot does not establish durable latest floors/revocations or installation fences','existing image may already be cached; no cold-engine qualification','caller source manifest must already be independently backup-authenticated']}
 with (root/'report.json').open('x') as out:json.dump(report,out,indent=2);out.write('\n')
 (root/'report.json').chmod(0o600)
 print('PASS signed same-handle tagless OCI cache import; persistent containers/volumes/tags unchanged')
 print('Report '+str(root/'report.json'))
