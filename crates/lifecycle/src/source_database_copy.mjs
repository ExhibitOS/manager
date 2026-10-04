// SPDX-License-Identifier: Apache-2.0
// Runs only in the isolated helper: native source read-only, unique empty target.
import assert from 'node:assert/strict';
import {open,lstat,realpath,readdir,mkdir,chown,chmod,statfs,readFile} from 'node:fs/promises';
import {constants} from 'node:fs';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
const source='/source',target='/snapshot',limit=2n*1024n*1024n*1024n;
const digest=b=>createHash('sha256').update(b).digest('hex');
const keys=['dev','ino','mode','uid','gid','nlink','size','mtimeNs','ctimeNs'];
const identity=s=>Object.fromEntries(keys.map(k=>[k,s[k].toString()]));
const same=(a,b)=>JSON.stringify(identity(a))===JSON.stringify(identity(b));
async function root(path){assert.equal(await realpath(path),path);assert.ok((await lstat(path)).isDirectory());assert.notEqual((await statfs(path,{bigint:true})).type,0x65735546n);}
async function census(base){
 let bytes=0n;const rows=[];
 async function walk(rel,depth){
  assert.ok(depth<=64 && rows.length<200000);
  const path=base+(rel?'/'+rel:''),s=await lstat(path,{bigint:true});
  assert.equal(await realpath(path),path);assert.equal(s.mode&0o6000n,0n);
  if(s.isDirectory()){
   rows.push({path:rel,kind:'directory',mode:Number(s.mode&0o1777n),uid:Number(s.uid),gid:Number(s.gid),identity:identity(s)});
   for(const name of (await readdir(path)).sort((a,b)=>Buffer.compare(Buffer.from(a),Buffer.from(b)))){
    assert.ok(name!=='.'&&name!=='..'&&!name.includes('/')&&!name.includes('\0'));
    await walk(rel?rel+'/'+name:name,depth+1);
   }
  }else{
   assert.ok(s.isFile()&&s.nlink===1n);assert.equal(s.mode&0o1000n,0n);bytes+=s.size;assert.ok(bytes<=limit);
   const f=await open(path,constants.O_RDONLY|constants.O_NOFOLLOW|constants.O_NONBLOCK);const h=createHash('sha256');const buffer=Buffer.alloc(65536);let count=0n;
   try{assert.ok(same(s,await f.stat({bigint:true})));for(;;){const r=await f.read(buffer,0,buffer.length,null);if(!r.bytesRead)break;h.update(buffer.subarray(0,r.bytesRead));count+=BigInt(r.bytesRead);buffer.fill(0);assert.ok(count<=s.size);}assert.equal(count,s.size);assert.ok(same(s,await f.stat({bigint:true}))&&same(s,await lstat(path,{bigint:true})));}
   finally{buffer.fill(0);await f.close();}
   rows.push({path:rel,kind:'file',mode:Number(s.mode&0o777n),uid:Number(s.uid),gid:Number(s.gid),bytes:Number(s.size),sha256:h.digest('hex'),identity:identity(s)});
  }
 }
 await root(base);await walk('',0);return {rows,bytes:Number(bytes)};
}
const content=c=>c.rows.map(({identity,...r})=>r);
let phase='source-census';
try{
 await root(source);await root(target);assert.deepEqual(await readdir(target),[]);
 // This first supported layout is the pinned PostgreSQL18 Docker volume layout.
 const before=await census(source);
 assert.equal((await readFile(source+'/18/docker/PG_VERSION','utf8')).trim(),'18');
 assert.ok(!before.rows.some(r=>r.path.endsWith('/postmaster.pid')));
 assert.ok(before.rows.some(r=>r.path==='18/docker/global/pg_control'&&r.kind==='file'&&r.bytes>0));
 phase='space';const space=await statfs(target,{bigint:true});assert.ok(space.bavail*space.bsize>=BigInt(before.bytes)+limit);
 const dirs=before.rows.filter(r=>r.kind==='directory'&&r.path);
 phase='copy';for(const r of dirs)await mkdir(target+'/'+r.path,{mode:0o700});
 for(const r of before.rows.filter(r=>r.kind==='file')){
  const p=source+'/'+r.path,src=await open(p,constants.O_RDONLY|constants.O_NOFOLLOW|constants.O_NONBLOCK);
  let dest;const buffer=Buffer.alloc(65536),h=createHash('sha256');let count=0;
  try{
   assert.deepEqual(identity(await src.stat({bigint:true})),r.identity);
   dest=await open(target+'/'+r.path,constants.O_WRONLY|constants.O_CREAT|constants.O_EXCL|constants.O_NOFOLLOW,0o600);
   for(;;){const n=(await src.read(buffer,0,buffer.length,null)).bytesRead;if(!n)break;count+=n;assert.ok(count<=r.bytes);h.update(buffer.subarray(0,n));let at=0;while(at<n){const written=(await dest.write(buffer,at,n-at,null)).bytesWritten;assert.ok(written>0);at+=written;}buffer.fill(0);}
   assert.equal(count,r.bytes);assert.equal(h.digest('hex'),r.sha256);assert.deepEqual(identity(await src.stat({bigint:true})),r.identity);assert.deepEqual(identity(await lstat(p,{bigint:true})),r.identity);
   await dest.chown(r.uid,r.gid);await dest.chmod(r.mode);await dest.sync();
  }finally{buffer.fill(0);if(dest)await dest.close();await src.close();}
 }
 for(const r of [...dirs].reverse()){await chown(target+'/'+r.path,r.uid,r.gid);await chmod(target+'/'+r.path,r.mode);}
 const rootRecord=before.rows[0];await chown(target,rootRecord.uid,rootRecord.gid);await chmod(target,rootRecord.mode);
 phase='comparison';const after=await census(source),copied=await census(target);
 assert.deepEqual(after,before);assert.deepEqual(content(copied),content(before));
 phase='control';const control=execFileSync('pg_controldata',[target+'/18/docker'],{encoding:'utf8',env:{...process.env,LC_ALL:'C'},timeout:10000,maxBuffer:16384});
 assert.ok(/^Database cluster state:\s+shut down\s*$/m.test(control));
 console.log(JSON.stringify({cleanShutdown:true,files:before.rows.filter(r=>r.kind==='file').length,entries:before.rows.length,bytes:before.bytes,contentSha256:digest(JSON.stringify(content(before))),postgresMajor:18,pgdata:'18/docker'}));
}catch{console.error('SOURCE_DATABASE_COPY_REFUSED:'+phase);process.exitCode=1;}
