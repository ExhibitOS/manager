# SPDX-License-Identifier: Apache-2.0
"""Read-only qualification of a tagless Docker OCI export; never extracts/loads it."""
import argparse,hashlib,json,re,tarfile,gzip,os,stat,sys
from pathlib import Path,PurePosixPath
if sys.flags.optimize:
 raise RuntimeError("qualification requires assertions enabled")
def identity(m):return (m.st_dev,m.st_ino,m.st_size,m.st_mtime_ns,m.st_ctime_ns)
def unique(pairs):
 d={}
 for k,v in pairs:
  assert k not in d;d[k]=v
 return d
def private_json(path,expected_sha=None):
 # No extraction and no alias/hardlink adoption. Bind metadata before/after the
 # read; hashes are caller trust inputs, not authentication by this inspector.
 assert path.is_absolute() and path.resolve()==path
 fd=os.open(path,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK)
 with os.fdopen(fd,'rb') as f:
  before=os.fstat(f.fileno())
  assert stat.S_ISREG(before.st_mode) and before.st_nlink==1 and before.st_uid==os.getuid() and stat.S_IMODE(before.st_mode) in (0o400,0o600) and 0<before.st_size<=16*1024**2
  raw=f.read(16*1024**2+1);assert len(raw)==before.st_size
  digest=hashlib.sha256(raw).hexdigest()
  if expected_sha is not None:assert re.fullmatch(r'[a-f0-9]{64}',expected_sha) and digest==expected_sha
  assert identity(before)==identity(os.fstat(f.fileno()))==identity(path.stat())
 return json.loads(raw,object_pairs_hook=unique),digest

def migration_binding(source,current,target=None,target_schema=None):
 canonical=lambda v:json.dumps(v,sort_keys=True,separators=(',',':'),ensure_ascii=False)
 sha=lambda v:hashlib.sha256(canonical(v).encode()).hexdigest()
 def valid(inventory):
  assert inventory['schemaVersion']=='1.0.0-draft.1' and re.fullmatch(r'[a-f0-9]{64}',inventory['schemaDigest'])
  rows=inventory['migrations'];assert isinstance(rows,list) and 0<len(rows)<=10000
  for i,m in enumerate(rows):
   assert set(m)=={'name','sha256'} and isinstance(m['name'],str) and len(m['name'])<=1024 and re.fullmatch(r'[0-9][a-zA-Z0-9_.-]*\.sql',m['name']) and re.fullmatch(r'[a-f0-9]{64}',m['sha256'])
   assert i==0 or rows[i-1]['name']<m['name']
 def schema(v):return sha({k:v[k] for k in ('schemaDigest','schemaVersion','migrations')})
 valid(source);source_hash=schema(source)
 if target is None:
  assert target_schema is None and current==source['migrations']
  return source_hash,source_hash,'unchanged',None
 valid(target)
 assert isinstance(target_schema,str) and re.fullmatch(r'[a-f0-9]{64}',target_schema) and schema(target)==target_schema and target_schema!=source_hash
 assert len(current)>len(source['migrations']) and current[:len(source['migrations'])]==source['migrations'] and current==target['migrations']
 # This binding proves embedded SQL identities only, not actual target catalog,
 # arbitrary data transformations, migration execution, health or recovery.
 return source_hash,target_schema,'strict-migration-extension',sha(target['migrations'])

def qualify(a,f):
 assert re.fullmatch(r'sha256:[a-f0-9]{64}',a.image) and re.fullmatch(r'[a-f0-9]{40}',a.source_commit)
 source,source_manifest_hash=private_json(a.source_manifest)
 assert source.get('kind')=='service-backup' and source.get('schemaVersion') in ('1.0.0-draft.1','1.0.0-draft.2')
 target_path=getattr(a,'target_inventory',None);target_pin=getattr(a,'target_inventory_sha256',None);target_schema=getattr(a,'target_schema_sha256',None)
 assert (target_path is None and target_pin is None and target_schema is None) or (target_path is not None and target_pin is not None and target_schema is not None)
 target,target_inventory_hash=private_json(target_path,target_pin) if target_path is not None else (None,None)
 assert a.archive.is_absolute() and a.archive.resolve()==a.archive
 before=a.archive.stat();assert stat.S_ISREG(before.st_mode) and before.st_nlink==1 and stat.S_IMODE(before.st_mode) in (0o400,0o600) and before.st_uid==os.getuid() and 0<before.st_size<=2*1024**3
 sha=lambda b:hashlib.sha256(b).hexdigest()
 with __import__('contextlib').nullcontext(f) as f:
  assert (before.st_dev,before.st_ino,before.st_size,before.st_mtime_ns,before.st_ctime_ns)==identity(os.fstat(f.fileno()))
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
   index=read('index.json');assert not index.get('annotations');assert index['schemaVersion']==2 and len(index['manifests'])==1 and index['manifests'][0]['digest']==a.image
   reached=set();runnable=[];attestations=[]
   def visit(desc,depth=0):
    assert not desc.get('urls') and not desc.get('data')
    assert set(desc.get('annotations',{})) <= {'vnd.docker.reference.digest','vnd.docker.reference.type'}
    assert depth<=8 and re.fullmatch(r'sha256:[a-f0-9]{64}',desc['digest'])
    name='blobs/sha256/'+desc['digest'][7:];assert records[name].size==desc['size'];reached.add(name)
    value=read(name);assert not value.get('annotations')
    if 'manifests' in value:
     assert value['schemaVersion']==2 and 1<=len(value['manifests'])<=16
     for child in value['manifests']:visit(child,depth+1)
    else:
     assert value['schemaVersion']==2
     config=value['config'];assert not config.get('urls');assert re.fullmatch(r'sha256:[a-f0-9]{64}',config['digest']);cfgname='blobs/sha256/'+config['digest'][7:];assert records[cfgname].size==config['size'];reached.add(cfgname);cfg=read(cfgname)
     layers=value['layers'];assert len(layers)<=128
     for layer in layers:
      assert not layer.get('urls') and not layer.get('data')
      assert re.fullmatch(r'sha256:[a-f0-9]{64}',layer['digest']);path='blobs/sha256/'+layer['digest'][7:];assert records[path].size==layer['size'];reached.add(path)
     if cfg.get('os')=='linux' and cfg.get('architecture')=='arm64':
      assert config['mediaType']=='application/vnd.oci.image.config.v1+json' and not config.get('data')
      assert desc.get('platform')=={'architecture':'arm64','os':'linux'}
      runnable.append((cfg,layers,desc['digest'],cfgname))
     else:
      # Tagless BuildKit exports have an empty attestation config. Treat this as
      # metadata only, never as an alternate executable image or trusted provenance.
      assert config['mediaType']=='application/vnd.oci.empty.v1+json' and config.get('data') in (None,'e30=')
      if config.get('data'):assert config['size']==2 and archive.extractfile(records[cfgname]).read()==b'{}'
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
   cfg,layers,runtime_digest,cfgname=runnable[0];assert all(d==runtime_digest for d in attestations);assert len(cfg['rootfs']['diff_ids'])==len(layers)
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
     seen=set()
     for entry in tar:
      name=str(PurePosixPath(entry.name));prefix='opt/exhibitos/database/migrations/'
      path=PurePosixPath(name);assert not path.is_absolute() and '..' not in path.parts
      protected={'opt','opt/exhibitos','opt/exhibitos/database','opt/exhibitos/database/migrations'}
      # Do not silently reconstruct migrations across replacement or opaque layers.
      assert not (path.parent.as_posix() in {'.',*protected} and path.name.startswith('.wh.'))
      if name in protected:assert entry.isdir()
      if name in protected or name.startswith(prefix):assert name not in seen;seen.add(name)
      if name.startswith(prefix):
       relative=name[len(prefix):];assert relative and '/' not in relative and not relative.startswith('.wh.')
       assert entry.isfile() and re.fullmatch(r'[0-9][a-zA-Z0-9_.-]*\.sql',relative)
       migrations[relative]=hashlib.file_digest(tar.extractfile(entry),'sha256').hexdigest()
    del parts
   compatibility=read('manifest.json');assert len(compatibility)==1 and compatibility[0].get('RepoTags') in (None,[])
   assert compatibility[0]['Config']==cfgname
   assert compatibility[0]['Layers']==['blobs/sha256/'+layer['digest'][7:] for layer in layers]
   assert set(compatibility[0])=={'Config','RepoTags','Layers'}
   current=[{'name':k,'sha256':v} for k,v in sorted(migrations.items())]
   source_schema,target_schema,mode,migration_hash=migration_binding(source['inventory'],current,target,target_schema)
  after=a.archive.stat();assert (before.st_dev,before.st_ino,before.st_size,before.st_mtime_ns,before.st_ctime_ns)==(after.st_dev,after.st_ino,after.st_size,after.st_mtime_ns,after.st_ctime_ns)
  report={'format':1,'artifactBytes':before.st_size,'artifactSha256':artifact,'runtimeImageSha256':a.image[7:],'target':'linux-arm64','version':labels['org.opencontainers.image.version'],'sourceCommit':a.source_commit,'sourceSchemaSha256':source_schema,'targetSchemaSha256':target_schema,'migrationMode':mode,'sourceManifestSha256':source_manifest_hash,'targetInventorySha256':target_inventory_hash,'targetMigrationsSha256':migration_hash,'embeddedMigrations':current,'ociBlobs':len(reached),'runtimeConfigSha256':cfgname.split('/')[-1],'layerDiffIds':cfg['rootfs']['diff_ids'],'limits':['read-only genuine artifact qualification, no load/start/migration/health/rollback','embedded migration binding is not actual catalog observation, migration compatibility, data preservation or rollback proof','target inventory/schema hash must originate from trusted independent catalog qualification and signed release; caller-selected hashes are not authentication','BuildKit attestation metadata has empty subjects; no authenticated source provenance claim','development source labels, not production signing authority or Windows/amd64 qualification']}
  return report

def main():
 p=argparse.ArgumentParser();p.add_argument('--archive',type=Path,required=True);p.add_argument('--image',required=True);p.add_argument('--source-manifest',type=Path,required=True);p.add_argument('--source-commit',required=True);p.add_argument('--target-inventory',type=Path);p.add_argument('--target-inventory-sha256');p.add_argument('--target-schema-sha256');p.add_argument('--output',type=Path,required=True);a=p.parse_args()
 with a.archive.open('rb') as f:report=qualify(a,f)
 with a.output.open('x') as out:out.write(json.dumps(report,indent=2)+'\n')
 a.output.chmod(0o600);print('PASS bounded tagless OCI graph/blob/layer/config/migration identity; no artifact execution')
if __name__=='__main__':main()
