// SPDX-License-Identifier: Apache-2.0
import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,writeFile,readFile,rm,realpath,symlink} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {createHash} from 'node:crypto';
import {createFrontendProvenance,frontendProvenance,PROVENANCE_FILE} from './frontend-provenance.ts';
const hash=data=>createHash('sha256').update(data).digest('hex');
async function fixture(t){
 const root=await mkdtemp(join(await realpath(tmpdir()),'exhi-front-provenance-'));t.after(()=>rm(root,{recursive:true,force:true}));
 const packages={'':{name:'@exhibitos/manager',version:'0.1.0'}};
 for(const name of ['vite','rolldown','react','@scope/component']){
  const path='node_modules/'+name;packages[path]={version:'1.0.0',integrity:'sha512-synthetic',dev:name==='vite'||name==='rolldown'};
  await mkdir(join(root,path),{recursive:true});await writeFile(join(root,path,'package.json'),JSON.stringify({name,version:'1.0.0'}));await writeFile(join(root,path,'index.js'),'export const synthetic=1;');
 }
 await mkdir(join(root,'src'));await writeFile(join(root,'src/main.ts'),'export const main=1;');
 await writeFile(join(root,'package.json'),JSON.stringify(packages['']));await writeFile(join(root,'package-lock.json'),JSON.stringify({lockfileVersion:3,packages}));
 const module=(path,length=10)=>[join(root,path),{renderedLength:length,code:'synthetic rendered code'}];
 const bundle={
  'assets/main.js':{type:'chunk',fileName:'assets/main.js',code:'import("./lazy.js");const main=1;',modules:Object.fromEntries([module('src/main.ts'),module('node_modules/react/index.js')]),imports:[],dynamicImports:['assets/lazy.js'],isEntry:true,isDynamicEntry:false},
  'assets/lazy.js':{type:'chunk',fileName:'assets/lazy.js',code:'export const component=1;',modules:Object.fromEntries([module('node_modules/@scope/component/index.js')]),imports:[],dynamicImports:[],isEntry:false,isDynamicEntry:true},
  'assets/main.css':{type:'asset',fileName:'assets/main.css',source:'body{color:red}'},
  'icon.svg':{type:'asset',fileName:'icon.svg',source:Uint8Array.from([1,2,3])},
 };
 return{root,bundle};
}
test('emitted chunk/module graph binds exact local, npm, lock and asset bytes deterministically',async t=>{
 const {root,bundle}=await fixture(t),{report,encoded}=createFrontendProvenance(root,bundle);
 assert.equal(report.format,'exhibitos-frontend-provenance-v1');assert.equal(report.packages.length,2);assert.equal(report.tools.length,2);
 assert.deepEqual(report.packages.map(p=>p.name),['@scope/component','react']);assert.equal(report.chunks[1].sha256,hash(bundle['assets/main.js'].code));
 assert.deepEqual(report.chunks[1].dynamicImports,['assets/lazy.js']);assert.equal(report.assets[0].kind,'css');assert.equal(report.assets[1].sha256,hash(Buffer.from([1,2,3])));
 const local=report.chunks[1].modules.find(m=>m.kind==='local-source');assert.equal(local.id,'src/main.ts');assert.equal(local.sourceSHA256,hash(await readFile(join(root,'src/main.ts'))));
 const npm=report.packages.find(p=>p.name==='react');assert.equal(npm.packageJsonSHA256,hash(await readFile(join(root,'node_modules/react/package.json'))));
 assert.equal(report.packageLockSHA256,hash(await readFile(join(root,'package-lock.json'))));assert.equal(encoded.includes(root),false);
 const reversed=Object.fromEntries(Object.entries(bundle).reverse());reversed['assets/main.js']={...bundle['assets/main.js'],modules:Object.fromEntries(Object.entries(bundle['assets/main.js'].modules).reverse())};assert.equal(createFrontendProvenance(root,reversed).encoded,encoded);
});
test('tree-shaken entries do not invent npm membership; known virtual/proxy ids normalize without absolute paths',async t=>{
 const {root,bundle}=await fixture(t),chunk=bundle['assets/main.js'];
 chunk.modules['/outside/private-not-delivered']={renderedLength:0};chunk.modules['\0vite/modulepreload-polyfill.js']={renderedLength:7,code:'helper'};
 chunk.modules['\0'+join(root,'node_modules/react/index.js')+'?commonjs-proxy']={renderedLength:4,code:'proxy'};
 const {report,encoded}=createFrontendProvenance(root,bundle);assert.equal(report.chunks[1].excluded.zeroRenderedModules,1);assert.equal(encoded.includes('private-not-delivered'),false);assert.equal(encoded.includes(root),false);
 assert.ok(report.chunks[1].modules.some(m=>m.id==='generated:vite/modulepreload-polyfill'));assert.ok(report.chunks[1].modules.some(m=>m.kind==='generated-npm-proxy'&&m.transform==='commonjs-proxy'));
});
test('unknown owner/version/virtual/query or external source refuses with path-free codes',async t=>{
 for(const mode of ['owner','version','virtual','query','outside','missing','symlink']){
  const {root,bundle}=await fixture(t),chunk=bundle['assets/main.js'];
  if(mode==='owner'){await mkdir(join(root,'node_modules/unknown'));await writeFile(join(root,'node_modules/unknown/index.js'),'x');chunk.modules[join(root,'node_modules/unknown/index.js')]={renderedLength:1};}
  if(mode==='version')await writeFile(join(root,'node_modules/react/package.json'),JSON.stringify({name:'react',version:'2.0.0'}));
  if(mode==='virtual')chunk.modules['\0unknown/contains-private-value']={renderedLength:1};
  if(mode==='query')chunk.modules[join(root,'src/main.ts')+'?private-value']={renderedLength:1};
  if(mode==='outside')chunk.modules['/outside/private-value.js']={renderedLength:1};
  if(mode==='missing')chunk.modules[join(root,'src/private-value.ts')]={renderedLength:1};
  if(mode==='symlink'){await symlink(join(root,'src/main.ts'),join(root,'src/linked.ts'));chunk.modules[join(root,'src/linked.ts')]={renderedLength:1};}
  assert.throws(()=>createFrontendProvenance(root,bundle),error=>/^FRONTEND_PROVENANCE_[A-Z_]+$/.test(error.message)&&!error.message.includes(root)&&!error.message.includes('private-value'));
 }
});
test('unsupported chunks, unsafe filenames, external imports and self-output refuse',async t=>{
 for(const mode of ['empty','unsupported','name','import','self','rendered']){
  const {root,bundle}=await fixture(t);
  if(mode==='empty')bundle['assets/main.js'].modules={};
  if(mode==='unsupported')bundle['assets/main.js'].type='unknown';
  if(mode==='name'){bundle['../unsafe.js']=bundle['assets/main.js'];delete bundle['assets/main.js'];}
  if(mode==='import')bundle['assets/main.js'].imports=['https://private.invalid/module'];
  if(mode==='self')bundle[PROVENANCE_FILE]={type:'asset',fileName:PROVENANCE_FILE,source:'existing'};
  if(mode==='rendered')bundle['assets/main.js'].modules[join(root,'src/main.ts')].renderedLength=NaN;
  assert.throws(()=>createFrontendProvenance(root,bundle),/^Error: FRONTEND_PROVENANCE_/);
 }
});
test('actual plugin emits the asset and refuses written chunk changes after generation',async t=>{
 const {root,bundle}=await fixture(t),plugin=frontendProvenance(),out=join(root,'dist'),emitted=[];
 plugin.configResolved({root,build:{outDir:'dist'}});
 plugin.generateBundle.handler.call({emitFile:asset=>emitted.push(asset)},{},bundle);
 assert.equal(emitted.length,1);assert.equal(emitted[0].fileName,PROVENANCE_FILE);bundle[PROVENANCE_FILE]=emitted[0];
 for(const item of Object.values(bundle)){await mkdir(join(out,item.fileName,'..'),{recursive:true});await writeFile(join(out,item.fileName),item.type==='chunk'?item.code:item.source);}
 plugin.writeBundle.handler({},bundle);
 await writeFile(join(out,'assets/main.js'),'changed after generate');assert.throws(()=>plugin.writeBundle.handler({},bundle),/FRONTEND_PROVENANCE_WRITTEN_OUTPUT_CHANGED/);
});
test('installed Rolldown runtime.js producer ID is a specifically bound generated helper',async t=>{
 const {root,bundle}=await fixture(t);bundle['assets/main.js'].modules['\0rolldown/runtime.js']={renderedLength:12,code:'generated helper'};
 const {report,encoded}=createFrontendProvenance(root,bundle);assert.ok(report.chunks[1].modules.some(m=>m.id==='generated:rolldown/runtime'&&m.kind==='generated-tool-helper'));assert.equal(encoded.includes(root),false);
});
