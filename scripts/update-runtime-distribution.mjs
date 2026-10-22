// SPDX-License-Identifier: Apache-2.0
// Qualification for one checksum-pinned archive/notice selection, not a license policy API.
import {readFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
export const QUALIFIED_REDISTRIBUTION='pinned-notices-qualified-v1';
export const DISTRIBUTION_FILES=Object.freeze(JSON.parse(readFileSync(new URL('./update-runtime-notices/distribution-files.json',import.meta.url),'utf8')));
export const DISTRIBUTION_NOTICE=readFileSync(new URL('./update-runtime-notices/DISTRIBUTION.txt',import.meta.url));
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
export function matchesDistributionFiles(files){
 if(!files||typeof files!=='object'||Array.isArray(files))return false;
 const notices=Object.keys(files).filter(name=>name.startsWith('notices/')).sort();
 if(JSON.stringify(notices)!==JSON.stringify(Object.keys(DISTRIBUTION_FILES).filter(name=>name.startsWith('notices/')).sort()))return false;
 return Object.entries(DISTRIBUTION_FILES).every(([name,pin])=>{const entry=files[name];return entry&&Object.keys(entry).sort().join(',')==='bytes,mode,sha256'&&entry.sha256===pin.sha256&&entry.bytes===pin.bytes&&entry.mode===pin.mode;});
}
export function qualifyRuntimeSelection(selected){
 const result=new Map(selected);
 if(result.has('notices/DISTRIBUTION.txt'))throw Error('UPDATE_RUNTIME_DISTRIBUTION_CHANGED');
 result.set('notices/DISTRIBUTION.txt',{data:Buffer.from(DISTRIBUTION_NOTICE),mode:0o644});
 const entries=Object.fromEntries([...result].filter(([name])=>name==='metadata/PYTHON.json'||name.startsWith('notices/')).map(([name,file])=>[name,{sha256:hash(file.data),bytes:file.data.length,mode:file.mode}]));
 if(!matchesDistributionFiles(entries))throw Error('UPDATE_RUNTIME_DISTRIBUTION_CHANGED');
 return result;
}
