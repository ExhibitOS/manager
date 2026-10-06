// SPDX-License-Identifier: Apache-2.0
// Compiled-in executor: actual isolated candidate inventory, never caller health flags.
import {mkdtemp,writeFile,rm,rmdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {main} from '/opt/exhibitos/scripts/service-backup.mjs';
const [sha,system]=process.argv.slice(1);let chunks=[],size=0;
for await(const chunk of process.stdin){size+=chunk.length;if(size>16*1024*1024)throw Error('UPDATE_INPUT_INVALID');chunks.push(chunk);}
const raw=Buffer.concat(chunks);
if(!/^[a-f0-9]{64}$/.test(sha??'')||createHash('sha256').update(raw).digest('hex')!==sha||! /^[1-9][0-9]{0,19}$/.test(system??''))throw Error('UPDATE_INPUT_INVALID');
const dir=await mkdtemp('/tmp/exhibitos-candidate-health-'),file=dir+'/manifest.json';
await writeFile(file,raw,{mode:0o600,flag:'wx'});
try{
 const proof=await main(['check-restored-inventory','--manifest-file',file,'--manifest-sha256',sha,'--quiesced','--snapshot-system-identifier',system],process.env);
 if(proof.currentInventoryVerified!==true||proof.preflightVerified!==false||proof.updateExecuted!==false)throw Error('UPDATE_INVENTORY_UNVERIFIED');
 await rm(file);await rmdir(dir);
 console.log(JSON.stringify(proof));
}catch{process.stderr.write('UPDATE_CANDIDATE_HEALTH_FAILED\n');process.exitCode=1;}
