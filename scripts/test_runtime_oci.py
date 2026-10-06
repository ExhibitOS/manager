# SPDX-License-Identifier: Apache-2.0
"""Small real tar/OCI graph tests; no image execution, extraction or DB writes."""
import hashlib,importlib.util,io,json,os,re,subprocess,sys,tarfile,tempfile,unittest
from pathlib import Path
from types import SimpleNamespace
spec=importlib.util.spec_from_file_location('runtime_oci',Path(__file__).with_name('verify-runtime-oci.py'))
oci=importlib.util.module_from_spec(spec);spec.loader.exec_module(oci)
def digest(data):return hashlib.sha256(data).hexdigest()
def encoded(v):return json.dumps(v,sort_keys=True,separators=(',',':')).encode()
def inventory(migrations,catalog='old catalog'):
 return {'schemaVersion':'1.0.0-draft.1','schemaDigest':digest(catalog.encode()),'migrations':[{'name':n,'sha256':digest(b)} for n,b in migrations]}
def schema(v):return digest(encoded(v))
class QualificationTests(unittest.TestCase):
 def setUp(self):self.temp=tempfile.TemporaryDirectory();self.root=Path(self.temp.name).resolve();self.old=[('001.sql',b'CREATE TABLE original(id bigint);')]
 def tearDown(self):self.temp.cleanup()
 def private(self,name,value):
  p=self.root/name;p.write_bytes(encoded(value));p.chmod(0o600);return p
 def artifact(self,migrations,later=None,duplicate=False):
  buffers=[]
  for entries in [migrations]+([later] if later is not None else []):
   out=io.BytesIO()
   with tarfile.open(fileobj=out,mode='w') as tar:
    for name,value in entries:
     e=tarfile.TarInfo('opt/exhibitos/database/migrations/'+name)
     if value is None:e.type=tarfile.SYMTYPE;e.linkname='/tmp/untrusted.sql';tar.addfile(e)
     else:e.size=len(value);tar.addfile(e,io.BytesIO(value))
     if duplicate:tar.addfile(e,io.BytesIO(value))
   buffers.append(out.getvalue())
  blobs={};layers=[];diffs=[]
  for b in buffers:
   h=digest(b);blobs[h]=b;layers.append({'mediaType':'application/vnd.oci.image.layer.v1.tar','digest':'sha256:'+h,'size':len(b)});diffs.append('sha256:'+h)
  cfg={'os':'linux','architecture':'arm64','config':{'Labels':{'org.opencontainers.image.revision':'a'*40,'org.opencontainers.image.source':'https://github.com/ExhibitOS/platform','org.opencontainers.image.licenses':'AGPL-3.0-or-later','org.opencontainers.image.version':'synthetic'},'Cmd':['node','scripts/local-runtime.mjs']},'rootfs':{'diff_ids':diffs}}
  c=encoded(cfg);ch=digest(c);blobs[ch]=c
  m=encoded({'schemaVersion':2,'config':{'mediaType':'application/vnd.oci.image.config.v1+json','digest':'sha256:'+ch,'size':len(c)},'layers':layers});mh=digest(m);blobs[mh]=m
  files={'oci-layout':encoded({'imageLayoutVersion':'1.0.0'}),'index.json':encoded({'schemaVersion':2,'manifests':[{'digest':'sha256:'+mh,'size':len(m),'platform':{'os':'linux','architecture':'arm64'}}]}),'manifest.json':encoded([{'Config':'blobs/sha256/'+ch,'RepoTags':None,'Layers':['blobs/sha256/'+x['digest'][7:] for x in layers]}])}
  files.update({'blobs/sha256/'+k:b for k,b in blobs.items()});p=self.root/'runtime.tar'
  with tarfile.open(p,mode='w') as tar:
   for n,b in files.items():e=tarfile.TarInfo(n);e.size=len(b);tar.addfile(e,io.BytesIO(b))
  p.chmod(0o600)
  return p,'sha256:'+mh
 def args(self,migrations,later=None,duplicate=False):
  p,image=self.artifact(migrations,later,duplicate);source=self.private('source.json',{'kind':'service-backup','schemaVersion':'1.0.0-draft.1','inventory':inventory(self.old)})
  return SimpleNamespace(archive=p,image=image,source_commit='a'*40,source_manifest=source,target_inventory=None,target_inventory_sha256=None,target_schema_sha256=None)
 def target(self,a,migrations):
  v=inventory(migrations,'new catalog');a.target_inventory=self.private('target.json',v);a.target_inventory_sha256=digest(a.target_inventory.read_bytes());a.target_schema_sha256=schema(v)
 def qualify(self,a):
  with a.archive.open('rb') as f:return oci.qualify(a,f)
 def test_unchanged_original_contract_and_bound_source_bytes(self):
  a=self.args(self.old);v=self.qualify(a);self.assertEqual(v['sourceSchemaSha256'],v['targetSchemaSha256']);self.assertEqual(v['migrationMode'],'unchanged');self.assertEqual(v['sourceManifestSha256'],digest(a.source_manifest.read_bytes()))
 def test_extended_sql_requires_explicit_target_catalog_pin(self):
  new=self.old+[('002.sql',b'CREATE TABLE additional(id bigint);')];a=self.args(new)
  with self.assertRaises(AssertionError):self.qualify(a)
  self.target(a,new);v=self.qualify(a);self.assertEqual(v['migrationMode'],'strict-migration-extension');self.assertNotEqual(v['sourceSchemaSha256'],v['targetSchemaSha256']);self.assertEqual(v['targetInventorySha256'],a.target_inventory_sha256)
 def test_target_bytes_schema_sql_and_partial_arguments_refuse(self):
  for fault in ['bytes','schema','sql','partial']:
   with self.subTest(fault=fault):
    new=self.old+[('002.sql',b'CREATE TABLE additional(id bigint);')];a=self.args(new);self.target(a,new)
    if fault=='bytes':a.target_inventory.write_bytes(a.target_inventory.read_bytes()+b' ')
    if fault=='schema':a.target_schema_sha256='f'*64
    if fault=='sql':self.target(a,self.old+[('002.sql',b'unrelated SQL')])
    if fault=='partial':a.target_inventory_sha256=None
    with self.assertRaises(AssertionError):self.qualify(a)
 def test_changed_removed_or_inserted_original_sql_refuses_even_bound_target(self):
  for rows in [[('001.sql',b'changed'),('002.sql',b'new')],[('002.sql',b'new')],[('000.sql',b'inserted')]+self.old]:
   with self.subTest(rows=rows):
    a=self.args(rows);self.target(a,rows)
    with self.assertRaises(AssertionError):self.qualify(a)
 def test_final_layer_symlink_duplicate_whiteout_and_non_sql_refuse(self):
  for fault in ['symlink','duplicate','whiteout','file']:
   with self.subTest(fault=fault):
    a=self.args(self.old,later=[('001.sql',None)] if fault=='symlink' else [('001.sql',b'original'),('.wh.001.sql',b'')] if fault=='whiteout' else [('notes.txt',b'data')] if fault=='file' else None,duplicate=fault=='duplicate')
    with self.assertRaises(AssertionError):self.qualify(a)
 def test_private_source_and_target_links_and_duplicate_json_refuse(self):
  for fault in ['source-public','source-link','target-hardlink','duplicate-json']:
   with self.subTest(fault=fault):
    a=self.args(self.old+[('002.sql',b'new')]);self.target(a,self.old+[('002.sql',b'new')])
    if fault=='source-public':a.source_manifest.chmod(0o644)
    if fault=='source-link':p=self.root/'alias.json';p.symlink_to(a.source_manifest);a.source_manifest=p
    if fault=='target-hardlink':os.link(a.target_inventory,self.root/'shared.json')
    if fault=='duplicate-json':a.source_manifest.write_bytes(b'{"kind":"service-backup","kind":"service-backup"}')
    with self.assertRaises(AssertionError):self.qualify(a)
    if (self.root/'alias.json').is_symlink():(self.root/'alias.json').unlink()
    if (self.root/'shared.json').exists():(self.root/'shared.json').unlink()
 def test_actual_rust_embedded_entry_preserves_retained_manifest_contract(self):
  a=self.args(self.old)
  rust=Path(__file__).parents[1]/'crates/lifecycle/src/prepared_oci.rs'
  entry=re.search(r'const ENTRY: &str = r#"(.*?)"#;',rust.read_text(),re.S).group(1)
  compiled="__name__='exhibitos_embedded_oci'\n"+Path(__file__).with_name('verify-runtime-oci.py').read_text()+"\n"+entry
  for pin in [digest(a.source_manifest.read_bytes()),'f'*64]:
   with a.archive.open('rb') as archive,a.source_manifest.open('rb') as manifest:
    r=subprocess.run([sys.executable,'-I','-c',compiled,str(a.archive),a.image,str(manifest.fileno()),a.source_commit,pin],stdin=archive,pass_fds=(manifest.fileno(),),capture_output=True)
   if pin=='f'*64:self.assertNotEqual(r.returncode,0)
   else:
    self.assertEqual(r.returncode,0,r.stderr.decode());self.assertEqual(json.loads(r.stdout)['sourceManifestSha256'],pin)
 def test_optimized_mode_cannot_disable_assertion_qualification(self):
  r=subprocess.run([sys.executable,'-O',str(Path(__file__).with_name('verify-runtime-oci.py')),'--help'],capture_output=True)
  self.assertNotEqual(r.returncode,0);self.assertIn(b'qualification requires assertions enabled',r.stderr)
 def test_migration_binding_strict_shape_order_names_and_schema(self):
  source=inventory(self.old);rows=inventory(self.old+[('002.sql',b'new')],'new')
  for fault in ['duplicate','unordered','path','unknown-field','empty','same-schema']:
   with self.subTest(fault=fault):
    target=json.loads(json.dumps(rows))
    if fault=='duplicate':target['migrations'].append(target['migrations'][-1])
    if fault=='unordered':target['migrations'].reverse()
    if fault=='path':target['migrations'][-1]['name']='../002.sql'
    if fault=='unknown-field':target['migrations'][-1]['extra']=True
    if fault=='empty':target['migrations']=[]
    if fault=='same-schema':target=source
    with self.assertRaises(AssertionError):oci.migration_binding(source,target['migrations'],target,schema(target))
if __name__=='__main__':unittest.main()
