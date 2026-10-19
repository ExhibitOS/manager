// SPDX-License-Identifier: Apache-2.0
// Bounded non-extracting tar reader for one checksum-pinned upstream distribution.
import {posix} from 'node:path';
import {TextDecoder} from 'node:util';
const decode=new TextDecoder('utf-8',{fatal:true});
const fail=()=>{throw Error('UPDATE_RUNTIME_ARCHIVE_UNSAFE');};
export function safeName(name){if(typeof name!=='string'||!name||name.length>4096||name.startsWith('/')||name.includes('\\')||name.includes('\0')||name.split('/').some(s=>s==='..'||s==='.'||!s)||!name.startsWith('python/'))fail();return name;}
function text(b){const i=b.indexOf(0);return decode.decode(i<0?b:b.subarray(0,i));}
function number(b){const s=text(b).trim();if(!/^[0-7]*$/.test(s))fail();const n=s?parseInt(s,8):0;if(!Number.isSafeInteger(n)||n<0)fail();return n;}
function pax(b){const values={};let at=0;while(at<b.length){const end=b.indexOf(32,at);if(end<0)fail();const token=decode.decode(b.subarray(at,end));if(!/^[1-9][0-9]*$/.test(token))fail();const n=Number(token);if(n>8192||at+n>b.length||b[at+n-1]!==10)fail();const row=decode.decode(b.subarray(end+1,at+n-1)),eq=row.indexOf('=');if(eq<1)fail();const key=row.slice(0,eq);if(!['path','linkpath','mtime','atime','ctime','uid','gid','uname','gname'].includes(key)||Object.hasOwn(values,key))fail();values[key]=row.slice(eq+1);at+=n;}return values;}
export function inspectTar(bytes,{maxEntries=30000,maxRegularBytes=256*1024**2}={}){
 if(!Buffer.isBuffer(bytes)||bytes.length>256*1024**2)fail();const entries=new Map();let at=0,total=0,pending={},longName=null,longLink=null,ended=false,headers=0;
 while(at+512<=bytes.length){const h=bytes.subarray(at,at+512);at+=512;if(h.every(x=>x===0)){ended=true;if(bytes.subarray(at).some(x=>x!==0))fail();break;}
  if(++headers>maxEntries*2)fail();let checksum=0;for(let i=0;i<512;i++)checksum+=i>=148&&i<156?32:h[i];if(number(h.subarray(148,156))!==checksum)fail();const size=number(h.subarray(124,136));if(size>maxRegularBytes||at+Math.ceil(size/512)*512>bytes.length)fail();const data=bytes.subarray(at,at+size);at+=Math.ceil(size/512)*512;
  const type=String.fromCharCode(h[156]||48);if(type==='x'){if(size>8192||Object.keys(pending).length)fail();pending=pax(data);continue;}if(type==='L'||type==='K'){if(size>4096)fail();if(type==='L'){if(longName!==null)fail();longName=text(data);}else{if(longLink!==null)fail();longLink=text(data);}continue;}
  let name=pending.path??longName??[text(h.subarray(345,500)),text(h.subarray(0,100))].filter(Boolean).join('/');name=name.replace(/\/$/,'');name=safeName(name);const link=pending.linkpath??longLink??text(h.subarray(157,257));pending={};longName=null;longLink=null;
  if(entries.has(name)||entries.size>=maxEntries||!['0','1','2','5'].includes(type))fail();const mode=number(h.subarray(100,108));if(mode&0o6000)fail();if(type==='0'){total+=size;if(total>maxRegularBytes)fail();}else if(size)fail();let target=null;
  if(type==='1'||type==='2'){if(!link||link.startsWith('/')||link.includes('\\')||link.includes('\0'))fail();target=safeName(posix.normalize(type==='2'?posix.join(posix.dirname(name),link):link));}
  entries.set(name,{name,type,mode,data,target});
 }
 if(!ended||Object.keys(pending).length||longName!==null||longLink!==null)fail();
 function regular(name,seen=new Set()){const e=entries.get(name);if(!e||seen.has(name)||seen.size>=32)fail();seen.add(name);if(e.target)return regular(e.target,seen);if(e.type!=='0')fail();return e;}
 for(const e of entries.values())if(e.target)regular(e.name);return{entries,total,regular};
}
export function selectRuntime(tree){const selected=new Map();for(const [name,e]of tree.entries){let dest;if(name==='python/PYTHON.json')dest='metadata/PYTHON.json';else if(name.startsWith('python/licenses/')&&e.type==='0')dest='notices/'+name.slice('python/licenses/'.length);else if(name.startsWith('python/install/')){
   const rel=name.slice('python/install/'.length);if(['bin/python','bin/python3','lib/libpython3.11.a'].includes(rel)||rel.startsWith('lib/python3.11/config-3.11-darwin/'))continue;dest='python/'+rel;
  }else continue;if(e.type==='5')continue;const file=tree.regular(name);if(selected.has(dest))fail();selected.set(dest,{data:file.data,mode:(file.mode&0o111)?0o755:0o644});}
 if(!selected.has('python/bin/python3.11')||!selected.has('python/lib/python3.11/encodings/__init__.py')||!selected.has('metadata/PYTHON.json')||!selected.has('notices/LICENSE.cpython.txt'))fail();return selected;
}
