// SPDX-License-Identifier: Apache-2.0
import test from 'node:test';import assert from 'node:assert/strict';
import {inspectTar,selectRuntime,safeName} from './update-runtime-archive.mjs';
import {SOURCE,NOTICE_SOURCE,completeRuntimeNotices,prepareUpdateRuntime} from './prepare-update-runtime.mjs';
import {readFileSync} from 'node:fs';
import {DISTRIBUTION_FILES,DISTRIBUTION_NOTICE,matchesDistributionFiles,qualifyRuntimeSelection} from './update-runtime-distribution.mjs';
import {createHash} from 'node:crypto';
function archive(rows){const parts=[];for(const row of rows){const data=Buffer.from(row.data??''),h=Buffer.alloc(512);h.write(row.name);h.write('0000755\0',100);h.write('0000000\0',108);h.write('0000000\0',116);h.write(data.length.toString(8).padStart(11,'0')+'\0',124);h.fill(32,148,156);h[156]=(row.type??'0').charCodeAt();if(row.link)h.write(row.link,157);h.write('ustar\0',257);const sum=h.reduce((a,b)=>a+b,0);h.write(sum.toString(8).padStart(6,'0')+'\0 ',148);parts.push(h,data,Buffer.alloc((512-data.length%512)%512));}return Buffer.concat([...parts,Buffer.alloc(1024)]);}
const core=[{name:'python/PYTHON.json',data:'{}'},{name:'python/licenses/LICENSE.cpython.txt',data:'synthetic notice'},{name:'python/install/bin/python3.11',data:'synthetic executable'},{name:'python/install/lib/python3.11/encodings/__init__.py',data:'synthetic stdlib'}];
test('whole stdlib is retained while fixed embedding artifacts and duplicate aliases alone are excluded',()=>{const tree=inspectTar(archive([...core,{name:'python/install/bin/python3',type:'2',link:'python3.11'},{name:'python/install/lib/python3.11/future/newmodule.py',data:'retained'},{name:'python/install/lib/libpython3.11.a',data:'embedding'},{name:'python/install/lib/python3.11/config-3.11-darwin/libpython3.11.a',data:'embedding'}]));const s=selectRuntime(tree);assert.equal(s.get('python/lib/python3.11/future/newmodule.py').data.toString(),'retained');assert.ok(!s.has('python/bin/python3'));assert.ok(!s.has('python/lib/libpython3.11.a'));assert.ok(s.has('notices/LICENSE.cpython.txt'));});
test('absolute/traversal/duplicate entries and invalid tar checksums are rejected',()=>{for(const name of ['/python/a','python/../a','python/./a','python//a'])assert.throws(()=>safeName(name),/ARCHIVE_UNSAFE/);assert.throws(()=>inspectTar(archive([{name:'python/a'},{name:'python/a'}])),/ARCHIVE_UNSAFE/);const bad=archive(core);bad[0]^=1;assert.throws(()=>inspectTar(bad),/ARCHIVE_UNSAFE/);});
test('escaping links, cycles, special files and expansion limits are refused',()=>{for(const rows of [[{name:'python/a',type:'2',link:'../../escape'}],[{name:'python/a',type:'2',link:'a'}],[{name:'python/a',type:'6'}]])assert.throws(()=>inspectTar(archive(rows)),/ARCHIVE_UNSAFE/);assert.throws(()=>inspectTar(archive(core),{maxRegularBytes:2}),/ARCHIVE_UNSAFE/);});
test('archive source pin is fixed and input cannot silently become a host Python',async()=>{assert.equal(SOURCE.bytes,42796019);assert.equal(SOURCE.sha256,'4e2b20716a080e9141ea80029dabbdc9457e20760f3fd3b16a17abd48ea6bf4a');await assert.rejects(prepareUpdateRuntime({archive:'relative',output:'relative'}),/HOST_UNSUPPORTED|ABSOLUTE_INPUT_REQUIRED/);});

const notice=readFileSync(new URL('./update-runtime-notices/LICENSE.zlib-ng.txt',import.meta.url));
const descriptor=JSON.parse(readFileSync(new URL('./update-runtime-notices/source.json',import.meta.url),'utf8'));
const noticeMetadata={build_info:{extensions:{zlib:[{links:[{name:'z',system:true}],license_paths:['licenses/LICENSE.cpython.txt','licenses/LICENSE.zlib-ng.txt']}]}}};
function noticeFixture(metadata=noticeMetadata,extra=[]){const tree=inspectTar(archive([{...core[0],data:JSON.stringify(metadata)},...core.slice(1),...extra]));return{tree,selected:selectRuntime(tree)};}
test('exact commit-pinned upstream notice is delivered and every declared path maps to selected unchanged bytes',()=>{
 assert.equal(NOTICE_SOURCE.commit,'a502e047655c31db29601def76ad6539acacf43d');
 assert.equal(createHash('sha256').update(notice).digest('hex'),NOTICE_SOURCE.sha256);
 const {tree,selected}=noticeFixture(),before=new Map([...selected].map(([name,e])=>[name,Buffer.from(e.data)]));
 const result=completeRuntimeNotices(tree,selected,notice,descriptor);
 assert.deepEqual(result.selected.get('notices/LICENSE.zlib-ng.txt').data,notice);
 assert.equal(selected.has('notices/LICENSE.zlib-ng.txt'),false);
 for(const [name,bytes]of before){assert.deepEqual(selected.get(name).data,bytes);assert.deepEqual(result.selected.get(name).data,bytes);}
 assert.deepEqual(result.discrepancy.declaredMissingInOriginalArchive,['licenses/LICENSE.zlib-ng.txt']);
 assert.equal(result.discrepancy.metadataPreserved,true);
 assert.equal(result.discrepancy.declaredNoticeFiles.length,2);
 for(const entry of result.discrepancy.declaredNoticeFiles){const bytes=result.selected.get(entry.selectedPath).data;assert.equal(bytes.length,entry.bytes);assert.equal(createHash('sha256').update(bytes).digest('hex'),entry.sha256);}
 assert.equal(result.discrepancy.status,'upstream-missing-notice-explicitly-supplemented');
});
test('changed, empty or substituted supplement and changed upstream descriptor are refused without changing selection',()=>{
 for(const bytes of [Buffer.alloc(0),Buffer.from(' '),Buffer.from(notice.toString().replace('1995-2024','1995-2025'))]){
  const {tree,selected}=noticeFixture();assert.throws(()=>completeRuntimeNotices(tree,selected,bytes,descriptor),/NOTICE_BYTES_CHANGED/);assert.equal(selected.has('notices/LICENSE.zlib-ng.txt'),false);
 }
 for(const change of [{commit:'0'.repeat(40)},{sha256:'0'.repeat(64)},{url:NOTICE_SOURCE.url.replace(NOTICE_SOURCE.commit,NOTICE_SOURCE.tag)},{extra:true}]){
  const {tree,selected}=noticeFixture();assert.throws(()=>completeRuntimeNotices(tree,selected,notice,{...descriptor,...change}),/NOTICE_SOURCE_CHANGED/);
 }
});
test('unknown or changed upstream missing-license set remains refused',()=>{
 for(const paths of [['licenses/LICENSE.cpython.txt'],['licenses/LICENSE.zlib-ng.txt','licenses/LICENSE.unknown.txt']]){
  const {tree,selected}=noticeFixture({license_paths:paths});assert.throws(()=>completeRuntimeNotices(tree,selected,notice,descriptor),/LICENSE_METADATA_CHANGED/);
 }
 const {tree,selected}=noticeFixture(noticeMetadata,[{name:'python/licenses/LICENSE.zlib-ng.txt',data:notice}]);
 assert.throws(()=>completeRuntimeNotices(tree,selected,notice,descriptor),/LICENSE_METADATA_CHANGED/);
});
test('every declared existing notice must be nonempty and byte-identical to archive selection',()=>{
 for(const mode of ['changed','missing','empty','metadata']){
  const {tree,selected}=noticeFixture();
  if(mode==='changed')selected.set('notices/LICENSE.cpython.txt',{data:Buffer.from('changed'),mode:0o644});
  if(mode==='missing')selected.delete('notices/LICENSE.cpython.txt');
  if(mode==='empty'){tree.entries.get('python/licenses/LICENSE.cpython.txt').data=Buffer.alloc(0);selected.get('notices/LICENSE.cpython.txt').data=Buffer.alloc(0);}
  if(mode==='metadata')selected.set('metadata/PYTHON.json',{data:Buffer.from('{}'),mode:0o644});
  assert.throws(()=>completeRuntimeNotices(tree,selected,notice,descriptor),mode==='metadata'?/LICENSE_METADATA_CHANGED/:/NOTICE_CORRESPONDENCE_CHANGED/);
 }
});
test('unsafe or malformed declared license paths do not become supplemental allowlist entries',()=>{
 for(const paths of [null,[],['licenses/../bad'],['licenses//bad'],['licenses/./bad'],['licenses/bad\\name'],['licenses/bad\0name']]){
  const {tree,selected}=noticeFixture({license_paths:paths});assert.throws(()=>completeRuntimeNotices(tree,selected,notice,descriptor),/LICENSE_METADATA_UNSAFE/);
 }
});

test('qualification is bound to exact original metadata and every preserved notice, not a public label',()=>{
 assert.equal(matchesDistributionFiles(DISTRIBUTION_FILES),true);
 assert.equal(Object.keys(DISTRIBUTION_FILES).filter(name=>name.startsWith('notices/LICENSE.')).length,20);
 assert.equal(createHash('sha256').update(DISTRIBUTION_NOTICE).digest('hex'),DISTRIBUTION_FILES['notices/DISTRIBUTION.txt'].sha256);
 for(const mutate of [v=>delete v['notices/LICENSE.zlib-ng.txt'],v=>v['notices/LICENSE.bdb.txt'].bytes++,v=>v['metadata/PYTHON.json'].sha256='0'.repeat(64),v=>v['notices/extra.txt']={bytes:1,mode:420,sha256:'0'.repeat(64)}]){
  const changed=structuredClone(DISTRIBUTION_FILES);mutate(changed);assert.equal(matchesDistributionFiles(changed),false);
 }
 const {selected}=noticeFixture();assert.throws(()=>qualifyRuntimeSelection(selected),/DISTRIBUTION_CHANGED/);assert.equal(selected.has('notices/DISTRIBUTION.txt'),false);
});
