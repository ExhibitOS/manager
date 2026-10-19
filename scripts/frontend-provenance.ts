// SPDX-License-Identifier: Apache-2.0
import {createHash} from 'node:crypto';
import {constants,openSync,closeSync,readSync,lstatSync,fstatSync,realpathSync,existsSync} from 'node:fs';
import {resolve,relative,dirname,sep,posix} from 'node:path';
import type {Plugin} from 'vite';
import type {Stats} from 'node:fs';

export const PROVENANCE_FILE='frontend-provenance.json';
const digest=(bytes:Uint8Array|string)=>createHash('sha256').update(bytes).digest('hex');
function fail(code:string):never{throw Error('FRONTEND_PROVENANCE_'+code);}
const same=(a:Stats,b:Stats)=>['dev','ino','size','mtimeMs','ctimeMs','mode','nlink'].every(k=>a[k as keyof typeof a]===b[k as keyof typeof b]);
function bytes(file:string,limit:number):Buffer{
 try{
 const before=lstatSync(file);if(!before.isFile()||before.isSymbolicLink()||realpathSync(file)!==file||before.size>limit)fail('INPUT_UNSAFE');
 const fd=openSync(file,constants.O_RDONLY|constants.O_NOFOLLOW);try{
  if(!same(before,fstatSync(fd)))fail('INPUT_CHANGED');
  const out=Buffer.alloc(before.size+1);let count=0;
  while(count<out.length){const n=readSync(fd,out,count,out.length-count,null);if(!n)break;count+=n;}
  if(count!==before.size||!same(before,fstatSync(fd))||!same(before,lstatSync(file)))fail('INPUT_CHANGED');return out.subarray(0,count);
 }finally{closeSync(fd);}
 }catch(error){if(error instanceof Error&&/^FRONTEND_PROVENANCE_[A-Z_]+$/.test(error.message))throw error;fail('INPUT_UNSAFE');}
}
function outputPath(name:string):string{
 if(!name||name.length>512||name.includes('\\')||name.includes('\0')||name.includes('?')||name.startsWith('/')||name.split('/').some(x=>!x||x==='.'||x==='..')||posix.normalize(name)!==name)fail('OUTPUT_PATH_UNSAFE');return name;
}
function relativePath(root:string,file:string):string{
 const name=relative(root,file).split(sep).join('/');outputPath(name);return name;
}
function parse(file:string,limit:number):{data:any;sha256:string}{
 const raw=bytes(file,limit);try{return{data:JSON.parse(raw.toString('utf8')),sha256:digest(raw)};}catch{fail('INPUT_JSON_INVALID');}
}
interface Rendered {renderedLength:number;code?:string|null}
export interface Chunk {type:'chunk';fileName:string;code:string;modules:Record<string,Rendered>;imports:string[];dynamicImports:string[];isEntry:boolean;isDynamicEntry:boolean}
export interface Asset {type:'asset';fileName:string;source:string|Uint8Array}
export type Bundle=Record<string,Chunk|Asset>;
const virtualHelpers=new Map([
 ['\0vite/modulepreload-polyfill.js','vite/modulepreload-polyfill'],['\0vite/preload-helper.js','vite/preload-helper'],
 ['\0commonjsHelpers.js','commonjs/helpers'],['\0rolldown/runtime','rolldown/runtime'],['\0rolldown/runtime.js','rolldown/runtime'],['rolldown:runtime','rolldown/runtime'],
]);
const queries=new Set(['commonjs-proxy','commonjs-es-import','commonjs-entry','commonjs-external','url','raw','inline','no-inline','used']);
export function createFrontendProvenance(rootInput:string,bundle:Bundle){
 let root:string;try{root=realpathSync(resolve(rootInput));}catch{fail('ROOT_UNSAFE');}
 const lock=parse(resolve(root,'package-lock.json'),16*1024**2),project=parse(resolve(root,'package.json'),1024**2);
 if(!/^(@[a-z0-9_.-]+\/)?[a-z0-9_.-]+$/i.test(project.data.name??'')||!/^\d+\.\d+\.\d+(?:[-+][a-z0-9.-]+)?$/i.test(project.data.version??''))fail('PROJECT_IDENTITY_INVALID');
 if(lock.data.lockfileVersion!==3||!lock.data.packages||project.data.name!==lock.data.packages['']?.name||project.data.version!==lock.data.packages['']?.version)fail('LOCK_BINDING_INVALID');
 const inputs=new Map<string,string>([['package-lock.json',lock.sha256],['package.json',project.sha256]]);
 const packages=new Map<string,Record<string,unknown>>();
 function owner(file:string){
  const path=relativePath(root,file),parts=path.split('/'),index=parts.lastIndexOf('node_modules');
  if(index<0)fail('NPM_OWNER_UNKNOWN');
  const count=parts[index+1]?.startsWith('@')?2:1,packageParts=parts.slice(index+1,index+1+count);
  if(packageParts.length!==count||packageParts.some(s=>!/^[@a-zA-Z0-9_.-]+$/.test(s)))fail('NPM_OWNER_UNKNOWN');
  const name=packageParts.join('/'),packagePath=parts.slice(0,index+1+count).join('/'),manifestPath=packagePath+'/package.json';
  const installed=parse(resolve(root,manifestPath),1024**2),entry=lock.data.packages[packagePath];
  if(!entry||entry.link||installed.data.name!==name||typeof installed.data.version!=='string'||!/^\d+\.\d+\.\d+(?:[-+][a-z0-9.-]+)?$/i.test(installed.data.version)||installed.data.version!==entry.version||(entry.name!==undefined&&entry.name!==name))fail('NPM_LOCK_MISMATCH');
  const record={path:packagePath,name,version:installed.data.version,packageJsonSHA256:installed.sha256,lockEntrySHA256:digest(JSON.stringify(entry)),...(typeof entry.integrity==='string'?{integrity:entry.integrity}:{})};
  packages.set(packagePath,record);inputs.set(manifestPath,installed.sha256);return packagePath;
 }
 // Tool bindings describe generated helpers, not packages inferred as delivered.
 const tools=['vite','rolldown'].map(name=>{const file=resolve(root,'node_modules',name,'package.json');if(!existsSync(file))fail('TOOL_BINDING_MISSING');const path=owner(file);return{...packages.get(path),role:'build-tool'};});
 packages.clear();
 let moduleCount=0,totalBytes=0;
 const chunks:Record<string,unknown>[]=[],assets:Record<string,unknown>[]=[];
 const names=Object.keys(bundle).sort();if(names.length>4096||names.includes(PROVENANCE_FILE))fail('OUTPUT_SET_UNSUPPORTED');
 for(const key of names){
  const item=bundle[key]!;if(item.fileName!==key)fail('OUTPUT_NAME_MISMATCH');outputPath(key);
  if(item.type==='asset'){
   if(typeof item.source!=='string'&&!(item.source instanceof Uint8Array))fail('ASSET_UNSUPPORTED');
   const raw=Buffer.from(item.source);totalBytes+=raw.length;assets.push({file:key,kind:key.endsWith('.css')?'css':'asset',bytes:raw.length,sha256:digest(raw)});continue;
  }
  if(item.type!=='chunk'||typeof item.code!=='string'||!item.modules||typeof item.modules!=='object'||!Object.keys(item.modules).length||!Array.isArray(item.imports)||!Array.isArray(item.dynamicImports))fail('CHUNK_UNSUPPORTED');
  const raw=Buffer.from(item.code);if(raw.length>32*1024**2)fail('CHUNK_TOO_LARGE');totalBytes+=raw.length;
  const dependencies=(list:string[])=>list.map(name=>{outputPath(name);if(bundle[name]?.type!=='chunk')fail('EXTERNAL_IMPORT_UNSUPPORTED');return name;}).sort();
  const modules:Record<string,unknown>[]=[],excluded:{zeroRenderedModules:number}={zeroRenderedModules:0};
  for(const id of Object.keys(item.modules).sort()){
   if(++moduleCount>4096)fail('MODULE_LIMIT');const rendered=item.modules[id]!;
   if(!Number.isSafeInteger(rendered.renderedLength)||rendered.renderedLength<0)fail('MODULE_UNSUPPORTED');
   if(rendered.renderedLength===0){excluded.zeroRenderedModules++;continue;}
   const helper=virtualHelpers.get(id);let record:Record<string,unknown>;
   if(helper)record={id:'generated:'+helper,kind:'generated-tool-helper'};
   else{
    const virtual=id.startsWith('\0'),plain=virtual?id.slice(1):id,index=plain.indexOf('?'),file=index<0?plain:plain.slice(0,index),query=index<0?undefined:plain.slice(index+1);
    if(query!==undefined&&!queries.has(query))fail('MODULE_QUERY_UNSUPPORTED');
    if(virtual&&(!query||!query.startsWith('commonjs-')))fail('VIRTUAL_MODULE_UNKNOWN');
    if(!file.startsWith('/')||file.includes('\0')||file.includes('#'))fail('MODULE_ID_UNSUPPORTED');
    const path=relativePath(root,file),source=bytes(file,8*1024**2);inputs.set(path,digest(source));
    if(path.split('/').includes('node_modules'))record={id:path,...(query?{transform:query}:{}),kind:virtual?'generated-npm-proxy':'npm',package:owner(file),sourceSHA256:digest(source)};
    else{if(!(path.startsWith('src/')||path==='index.html')||path.split('/').some(s=>s.startsWith('.')))fail('LOCAL_SOURCE_UNSUPPORTED');record={id:path,...(query?{transform:query}:{}),kind:'local-source',sourceSHA256:digest(source)};}
   }
   modules.push({...record,renderedLength:rendered.renderedLength,...(typeof rendered.code==='string'?{renderedCodeSHA256:digest(rendered.code)}:{})});
  }
  chunks.push({file:key,bytes:raw.length,sha256:digest(raw),entry:item.isEntry===true,dynamicEntry:item.isDynamicEntry===true,imports:dependencies(item.imports),dynamicImports:dependencies(item.dynamicImports),modules,excluded});
 }
 if(totalBytes>64*1024**2||!chunks.length)fail('OUTPUT_LIMIT_OR_NO_CHUNKS');
 // Re-read exactly observed inputs so a mid-hook change cannot bless mixed bytes.
 for(const [path,hash]of inputs)if(digest(bytes(resolve(root,path),path==='package-lock.json'?16*1024**2:8*1024**2))!==hash)fail('INPUT_CHANGED');
 const report={format:'exhibitos-frontend-provenance-v1',project:{name:project.data.name,version:project.data.version},packageLockSHA256:lock.sha256,packageJsonSHA256:project.sha256,tools,packages:[...packages.values()].sort((a,b)=>String(a.path)<String(b.path)?-1:String(a.path)>String(b.path)?1:0),chunks,assets,coverage:{observed:'emitted frontend chunks and positive rendered-length modules',assets:'CSS and other emitted assets have byte hashes; source/package membership is not inferred from asset bytes',generated:'recognized build-tool helpers and file proxies are labelled separately',excluded:'zero-rendered modules are counted, not claimed delivered',limits:['not Rust/C/native/Node/Python dependency coverage','not proof a dependency executes','not complete application SBOM','provenance asset excludes its own bytes']}};
 const encoded=JSON.stringify(report)+'\n';if(Buffer.byteLength(encoded)>2*1024**2)fail('REPORT_TOO_LARGE');return{report,encoded};
}
export function frontendProvenance():Plugin{
 let root='',outDir='',expected:ReturnType<typeof createFrontendProvenance>|undefined;
 return{name:'exhibitos-frontend-provenance',apply:'build',enforce:'post',configResolved(config){root=config.root;outDir=resolve(root,config.build.outDir);},
  generateBundle:{order:'post',handler(_options,bundle){expected=createFrontendProvenance(root,bundle as unknown as Bundle);this.emitFile({type:'asset',fileName:PROVENANCE_FILE,source:expected.encoded});}},
  writeBundle:{order:'post',handler(_options,bundle){
   if(!expected)fail('REPORT_MISSING');
   const actual=Object.keys(bundle).sort(),wanted=[...expected.report.chunks,...expected.report.assets].map(x=>String(x.file)).concat(PROVENANCE_FILE).sort();
   if(JSON.stringify(actual)!==JSON.stringify(wanted))fail('POST_HOOK_OUTPUT_CHANGED');
   for(const row of [...expected.report.chunks,...expected.report.assets])if(digest(bytes(resolve(outDir,String(row.file)),64*1024**2))!==row.sha256)fail('WRITTEN_OUTPUT_CHANGED');
   if(bytes(resolve(outDir,PROVENANCE_FILE),2*1024**2).toString('utf8')!==expected.encoded)fail('WRITTEN_REPORT_CHANGED');
  }},
 };
}
