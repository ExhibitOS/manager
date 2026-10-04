# SPDX-License-Identifier: Apache-2.0
"""Read-only qualification of a tagless Docker OCI export; never extracts/loads it."""
import argparse,hashlib,json,re,tarfile,gzip,os,stat,sys
if sys.flags.optimize:
 raise RuntimeError("qualification requires assertions enabled")
from pathlib import Path,PurePosixPath
p=argparse.ArgumentParser();p.add_argument('--archive',type=Path,required=True);p.add_argument('--image',required=True);p.add_argument('--source-manifest',type=Path,required=True);p.add_argument('--source-commit',required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
assert re.fullmatch(r'sha256:[a-f0-9]{64}',a.image) and re.fullmatch(r'[a-f0-9]{40}',a.source_commit)
assert a.archive.is_absolute() and a.archive.resolve()==a.archive
before=a.archive.stat();assert stat.S_ISREG(before.st_mode) and before.st_nlink==1 and stat.S_IMODE(before.st_mode)==0o600 and before.st_uid==os.getuid() and 0<before.st_size<=2*1024**3
sha=lambda b:hashlib.sha256(b).hexdigest()
def unique(pairs):
 d={}
 for k,v in pairs:
  assert k not in d;d[k]=v
 return d
with a.archive.open('rb') as f:
 artifact=hashlib.file_digest(f,'sha256').hexdigest();f.seek(0)
 with tarfile.open(fileobj=f,mode='r:') as archive:
  members=archive.getmembers();assert len(members)<=100000
  records={};total=0
  for m in members:
   path=PurePosixPath(m.name);assert not path.is_absolute() and '..' not in path.parts and m.name not in records
   assert m.isdir() or m.isfile();records[m.name]=m
   if m.isfile():
    total+=m.size;assert total<=2*1024**3
    if m.name.startswith('blobs/sha256/'):
     assert re.fullmatch(r'blobs/sha256/[a-f0-9]{64}',m.name)
     assert hashlib.file_digest(archive.extractfile(m),'sha256').hexdigest()==m.name.split('/')[-1]
    else:assert m.name in ('index.json','manifest.json','oci-layout')
  def read(name):
   m=records[name];assert m.isfile() and m.size<=4*1024**2
   return json.loads(archive.extractfile(m).read(),object_pairs_hook=unique)
  assert read('oci-layout')=={'imageLayoutVersion':'1.0.0'}
  index=read('index.json');assert index['schemaVersion']==2 and len(index['manifests'])==1 and index['manifests'][0]['digest']==a.image
  reached=set();runnable=[];attestations=[]
  def visit(desc,depth=0):
   assert depth<=8 and re.fullmatch(r'sha256:[a-f0-9]{64}',desc['digest'])
   name='blobs/sha256/'+desc['digest'][7:];assert records[name].size==desc['size'];reached.add(name)
   value=read(name)
   if 'manifests' in value:
    assert value['schemaVersion']==2 and 1<=len(value['manifests'])<=16
    for child in value['manifests']:visit(child,depth+1)
   else:
    assert value['schemaVersion']==2
    config=value['config'];assert re.fullmatch(r'sha256:[a-f0-9]{64}',config['digest']);cfgname='blobs/sha256/'+config['digest'][7:];assert records[cfgname].size==config['size'];reached.add(cfgname);cfg=read(cfgname)
    layers=value['layers'];assert len(layers)<=128
    for layer in layers:
     assert re.fullmatch(r'sha256:[a-f0-9]{64}',layer['digest']);path='blobs/sha256/'+layer['digest'][7:];assert records[path].size==layer['size'];reached.add(path)
    if cfg.get('os')=='linux' and cfg.get('architecture')=='arm64':
     assert desc.get('platform')=={'architecture':'arm64','os':'linux'}
     runnable.append((cfg,layers,desc['digest']))
    else:
     # Tagless BuildKit exports have an empty attestation config. Treat this as
     # metadata only, never as an alternate executable image or trusted provenance.
     assert cfg=={} and desc.get('platform')=={'architecture':'unknown','os':'unknown'}
     annotations=desc.get('annotations',{})
     assert annotations.get('vnd.docker.reference.type')=='attestation-manifest'
     assert re.fullmatch(r'sha256:[a-f0-9]{64}',annotations.get('vnd.docker.reference.digest',''))
     assert 1<=len(layers)<=16
     for layer in layers:
      assert layer['mediaType']=='application/vnd.in-toto+json'
      statement=read('blobs/sha256/'+layer['digest'][7:])
      assert statement['_type']=='https://in-toto.io/Statement/v1'
      assert statement['predicateType']=='https://slsa.dev/provenance/v1'
      assert layer.get('annotations',{}).get('in-toto.io/predicate-type')==statement['predicateType']
      assert statement['subject']==[]
     attestations.append(annotations['vnd.docker.reference.digest'])
  visit(index['manifests'][0]);assert len(runnable)==1
  assert reached=={n for n in records if n.startswith('blobs/sha256/') and records[n].isfile()}
  cfg,layers,runtime_digest=runnable[0];assert all(d==runtime_digest for d in attestations);assert len(cfg['rootfs']['diff_ids'])==len(layers)
  labels=cfg['config']['Labels'];assert labels['org.opencontainers.image.revision']==a.source_commit and labels['org.opencontainers.image.source']=='https://github.com/ExhibitOS/platform' and labels['org.opencontainers.image.licenses']=='AGPL-3.0-or-later'
  assert cfg['config']['Cmd']==['node','scripts/local-runtime.mjs']
  migrations={};expanded=0
  for layer,diff in zip(layers,cfg['rootfs']['diff_ids']):
   path='blobs/sha256/'+layer['digest'][7:];raw=archive.extractfile(records[path]);media=layer['mediaType'];assert media in ('application/vnd.oci.image.layer.v1.tar','application/vnd.oci.image.layer.v1.tar+gzip')
   if media.endswith('+gzip'):raw=gzip.GzipFile(fileobj=raw)
   # No layer is extracted to disk. Bounded decoded bytes and content digest.
   h=hashlib.sha256();parts=[];size=0
   while True:
    b=raw.read(65536)
    if not b:break
    size+=len(b);expanded+=len(b);assert expanded<=2*1024**3;h.update(b);parts.append(b)
   assert 'sha256:'+h.hexdigest()==diff
   import io
   with tarfile.open(fileobj=io.BytesIO(b''.join(parts)),mode='r:') as tar:
    for entry in tar:
     name=str(PurePosixPath(entry.name));prefix='opt/exhibitos/database/migrations/'
     path=PurePosixPath(name);assert not path.is_absolute() and '..' not in path.parts
     protected={'opt','opt/exhibitos','opt/exhibitos/database','opt/exhibitos/database/migrations'}
     # Do not silently reconstruct migrations across replacement or opaque layers.
     assert not (path.parent.as_posix() in {'.',*protected} and path.name.startswith('.wh.'))
     if name in protected:assert entry.isdir()
     if name.startswith(prefix):
      relative=name[len(prefix):];assert relative and '/' not in relative and not relative.startswith('.wh.')
      if entry.isfile():assert relative.endswith('.sql');migrations[relative]=hashlib.file_digest(tar.extractfile(entry),'sha256').hexdigest()
   del parts
  compatibility=read('manifest.json');assert len(compatibility)==1 and not compatibility[0].get('RepoTags')
  expected=json.loads(a.source_manifest.read_text())['inventory'];current=[{'name':k,'sha256':v} for k,v in sorted(migrations.items())];assert current==expected['migrations']
  canonical=lambda v:json.dumps(v,sort_keys=True,separators=(',',':'),ensure_ascii=False)
  schema=sha(canonical({'schemaDigest':expected['schemaDigest'],'schemaVersion':expected['schemaVersion'],'migrations':expected['migrations']}).encode())
 after=a.archive.stat();assert (before.st_dev,before.st_ino,before.st_size,before.st_mtime_ns,before.st_ctime_ns)==(after.st_dev,after.st_ino,after.st_size,after.st_mtime_ns,after.st_ctime_ns)
 report={'format':1,'artifactBytes':before.st_size,'artifactSha256':artifact,'runtimeImageSha256':a.image[7:],'target':'linux-arm64','version':labels['org.opencontainers.image.version'],'sourceCommit':a.source_commit,'sourceSchemaSha256':schema,'targetSchemaSha256':schema,'embeddedMigrations':current,'ociBlobs':len(reached),'limits':['read-only genuine artifact qualification, no load/start/migration/health/rollback','unchanged embedded migrations are not actual runtime compatibility or image-only rollback proof','BuildKit attestation metadata has empty subjects; no authenticated source provenance claim','development source labels, not production signing authority or Windows/amd64 qualification']}
 assert not a.output.exists();a.output.write_text(json.dumps(report,indent=2)+'\n');a.output.chmod(0o600);print('PASS bounded tagless OCI graph/blob/layer/config/migration identity; no artifact execution')
