// Copyright 2026 ExhibitOS contributors. SPDX-License-Identifier: Apache-2.0
// Unkeyed BLAKE3-256 for Cloudflare Pages content addressing (not a new credential system).
// Algorithm: https://github.com/BLAKE3-team/BLAKE3-specs/blob/master/blake3.pdf
import {extname} from 'node:path';
const IV=Uint32Array.from([0x6a09e667,0xbb67ae85,0x3c6ef372,0xa54ff53a,0x510e527f,0x9b05688c,0x1f83d9ab,0x5be0cd19]);
const P=[2,6,3,10,7,0,4,13,1,11,12,5,9,14,15,8];
const rotr=(x,n)=>(x>>>n)|(x<<(32-n));
function compress(cv,words,counter,length,flags){
 const v=new Uint32Array(16);v.set(cv);v.set(IV.subarray(0,4),8);v[12]=counter>>>0;v[13]=Math.floor(counter/4294967296);v[14]=length;v[15]=flags;
 let m=words;
 const g=(a,b,c,d,x,y)=>{v[a]=(v[a]+v[b]+x)>>>0;v[d]=rotr(v[d]^v[a],16);v[c]=(v[c]+v[d])>>>0;v[b]=rotr(v[b]^v[c],12);v[a]=(v[a]+v[b]+y)>>>0;v[d]=rotr(v[d]^v[a],8);v[c]=(v[c]+v[d])>>>0;v[b]=rotr(v[b]^v[c],7);};
 for(let round=0;round<7;round++){g(0,4,8,12,m[0],m[1]);g(1,5,9,13,m[2],m[3]);g(2,6,10,14,m[4],m[5]);g(3,7,11,15,m[6],m[7]);g(0,5,10,15,m[8],m[9]);g(1,6,11,12,m[10],m[11]);g(2,7,8,13,m[12],m[13]);g(3,4,9,14,m[14],m[15]);m=P.map(i=>m[i]);}
 for(let i=0;i<8;i++){v[i]^=v[i+8];v[i+8]^=cv[i];}return v;
}
const chaining=o=>compress(o.cv,o.words,o.counter,o.length,o.flags).slice(0,8);
function parent(a,b){const words=new Uint32Array(16);words.set(a);words.set(b,8);return {cv:IV,words,counter:0,length:64,flags:4};}
function chunk(bytes,counter){let cv=IV;let last;const blocks=Math.max(1,Math.ceil(bytes.length/64));for(let i=0;i<blocks;i++){const part=bytes.subarray(i*64,(i+1)*64),pad=Buffer.alloc(64);pad.set(part);const words=new Uint32Array(16);for(let j=0;j<16;j++)words[j]=pad.readUInt32LE(j*4);last={cv,words,counter,length:part.length,flags:(i===0?1:0)|(i===blocks-1?2:0)};if(i<blocks-1)cv=chaining(last);}return last;}
export function blake3(bytes){const n=Math.max(1,Math.ceil(bytes.length/1024)),stack=[];for(let i=0;i<n-1;i++){let cv=chaining(chunk(bytes.subarray(i*1024,(i+1)*1024),i)),total=i+1;while(total%2===0){cv=chaining(parent(stack.pop(),cv));total/=2;}stack.push(cv);}let output=chunk(bytes.subarray((n-1)*1024),n-1);while(stack.length)output=parent(stack.pop(),chaining(output));const words=compress(output.cv,output.words,0,output.length,output.flags|8),result=Buffer.alloc(32);for(let i=0;i<8;i++)result.writeUInt32LE(words[i],i*4);return result;}
// Exact Wrangler Pages algorithm; extension is case-sensitive and excludes the dot.
export function pagesAssetHash(bytes,path){return blake3(Buffer.from(bytes.toString('base64')+extname(path).slice(1))).toString('hex').slice(0,32);}
