// SPDX-License-Identifier: Apache-2.0
// Executes the actual fixed extraction script with synthetic, in-memory I/O.
// This is not crypto, Docker, DB restore or native GUI qualification.
import test from 'node:test';import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';import {createHash} from 'node:crypto';
const source=await readFile(new URL('../crates/lifecycle/src/restoration.rs',import.meta.url),'utf8');
const script=source.split('const AUTHENTICATE: &str = r#"')[1].split('"#;')[0].replace(/^import .*;$/gm,'');
const execute=new (Object.getPrototypeOf(async()=>{}).constructor)('main','createHash','readFile','mkdir','copyFile','lstat','console',script);
async function fixture({third=false,basename='maintenance-image.tar',extra='image-2.tar',oversize=false}={}){
 const bundle={images:[{reference:'sha256:'+'a'.repeat(64)},{reference:'sha256:'+'b'.repeat(64)},...(third?[{reference:'sha256:'+'c'.repeat(64),archive:{path:basename}}]:[])]};
 const names=['manager-bundle-manifest.json','images/image-0.tar','images/image-1.tar',...(extra?['images/'+extra]:[])];
 const manifest=Buffer.from(JSON.stringify({id:'synthetic-backup',files:names.map((name,i)=>({role:i?'deployment':'configuration',name,path:`files/${String(i).padStart(8,'0')}.gcm`}))}));
 const encoded=Buffer.from(JSON.stringify(bundle)),before=Buffer.from(encoded),copied=[],outputs=[];
 const run=()=>execute(async()=>({files:names.length}),createHash,async path=>{
  if(path==='/work/authenticated/manifest.json')return manifest;
  assert.equal(path,'/work/authenticated/00000000.gcm.plain');return encoded;
 },async()=>{},async(from,to,flags)=>{assert.equal(flags,1);copied.push({from,to});},async()=>({isFile:()=>true,isSymbolicLink:()=>false,nlink:1,size:oversize?1024*1024+1:encoded.length}),{log:value=>outputs.push(JSON.parse(value))});
 return {run,copied,outputs,encoded,before};
}
test('legacy two-image and fixed-helper extraction keep the authenticated index paths and originals',async()=>{
 for(const options of [{extra:null},{third:true},{third:true,basename:'image-2.tar'}]){
  const f=await fixture(options);await f.run();assert.equal(f.outputs.length,1);assert.equal(f.outputs[0].operation,'authenticated-installation');
  assert.equal(f.copied.filter(item=>item.to.startsWith('/work/deployment/')).length,options.third?3:2);assert.deepEqual(f.encoded,f.before);
 }
});
test('undeclared or arbitrary extra images and oversized original metadata cannot be copied as the helper',async()=>{
 for(const options of [{},{third:true,basename:'arbitrary.tar'},{third:true,extra:'image-3.tar'},{third:true,oversize:true}]){
  const f=await fixture(options);await assert.rejects(f.run(),/RESTORE_LAYOUT_UNSUPPORTED/);assert.equal(f.outputs.length,0);
  assert(!f.copied.some(item=>item.to.endsWith('/image-2.tar')||item.to.endsWith('/image-3.tar')));assert.deepEqual(f.encoded,f.before);
 }
});
