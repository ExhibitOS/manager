// Copyright 2026 ExhibitOS contributors. SPDX-License-Identifier: Apache-2.0
// Small bounded JSON reader: duplicate keys are refused before information is lost.
export function boundedJSON(text,maxBytes=16384){
 if(typeof text!=='string'||Buffer.byteLength(text)>maxBytes)throw Error('INVALID_JSON');
 let i=0,nodes=0;const ws=()=>{while(/[ \t\r\n]/.test(text[i]??'!'))i++;};
 const string=()=>{const start=i;if(text[i++]!=='"')throw Error('INVALID_JSON');let escaped=false;for(;i<text.length;i++){const c=text[i];if(!escaped&&c==='"'){i++;return JSON.parse(text.slice(start,i));}if(!escaped&&c==='\\'){escaped=true;continue;}escaped=false;}throw Error('INVALID_JSON');};
 const value=depth=>{
  if(depth>12||++nodes>2048)throw Error('INVALID_JSON');ws();const c=text[i];
  if(c==='"')return string();
  if(c==='{'){i++;ws();const o=Object.create(null),seen=new Set();if(text[i]==='}'){i++;return o;}for(;;){ws();const k=string();if(seen.has(k)||['__proto__','constructor','prototype'].includes(k))throw Error('INVALID_JSON');seen.add(k);ws();if(text[i++]!==':')throw Error('INVALID_JSON');o[k]=value(depth+1);ws();if(text[i]==='}'){i++;return o;}if(text[i++]!==',')throw Error('INVALID_JSON');}}
  if(c==='['){i++;ws();const a=[];if(text[i]===']'){i++;return a;}for(;;){a.push(value(depth+1));ws();if(text[i]===']'){i++;return a;}if(text[i++]!==',')throw Error('INVALID_JSON');}}
  for(const [token,result]of [['true',true],['false',false],['null',null]])if(text.startsWith(token,i)){i+=token.length;return result;}
  const m=text.slice(i).match(/^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?/);if(!m)throw Error('INVALID_JSON');i+=m[0].length;const number=Number(m[0]);if(!Number.isFinite(number))throw Error('INVALID_JSON');return number;
 };
 const result=value(0);ws();if(i!==text.length)throw Error('INVALID_JSON');return result;
}
