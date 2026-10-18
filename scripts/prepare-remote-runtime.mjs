// SPDX-License-Identifier: Apache-2.0
import {readFile,writeFile,mkdir,lstat,chmod} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import {join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {homedir} from 'node:os';
const pins={
 'darwin-arm64':'6239d4cf92d864487ec8cd3615038f7b67e7f58b77b21cd2f09ea9fbd68065fe',
 'darwin-x64':'0ae5a24c24bb7d015cd816c5036b3f90f2945aa872fcf54e58da054753b3a299',
 'linux-x64':'fd8e59d5a511510f6a298afb548f18c7d2b1be404d8b4a27d94fbe49f56cb2d6',
 'linux-arm64':'6ad1325edbdb5649c379b75a237147a666c95d4f9ae8d340fef2d1575d289ad2'};
const sha=b=>createHash('sha256').update(b).digest('hex'),platform=process.platform+'-'+process.arch,pin=pins[platform];
if(!pin){console.log(JSON.stringify({prepared:false,remoteSupported:false,manualFallback:true}));process.exit(0);}
const archiveName='node-v24.21.0-'+platform+'.tar.xz',archive=process.env.EXHIBITOS_NODE_ARCHIVE??join(homedir(),'.nvm','.cache','bin','node-v24.21.0-'+platform,archiveName);
const s=await lstat(archive);if(!s.isFile()||s.isSymbolicLink()||s.size>100*1024**2)throw Error('REMOTE_NODE_ARCHIVE_UNSAFE');
const bytes=await readFile(archive);if(sha(bytes)!==pin)throw Error('REMOTE_NODE_ARCHIVE_CHECKSUM');
const out=new URL('../src-tauri/remote-runtime/generated/',import.meta.url);await mkdir(out,{recursive:true,mode:0o700});
const node=execFileSync('/usr/bin/tar',['-xOf',archive,'node-v24.21.0-'+platform+'/bin/node'],{maxBuffer:150*1024**2,timeout:30000});
const license=execFileSync('/usr/bin/tar',['-xOf',archive,'node-v24.21.0-'+platform+'/LICENSE'],{maxBuffer:1024**2,timeout:30000});
await writeFile(new URL('node',out),node,{mode:0o700});await chmod(new URL('node',out),0o700);await writeFile(new URL('NODE-LICENSE',out),license,{mode:0o600});
const source=JSON.parse(await readFile(new URL('../vendor/deployment/source.json',import.meta.url),'utf8'));
for(const [name,hash]of Object.entries(source.files))if(sha(await readFile(new URL('../vendor/deployment/src/'+name,import.meta.url)))!==hash)throw Error('REMOTE_ADAPTER_SOURCE_DRIFT');
const version=execFileSync(fileURLToPath(new URL('node',out)),['--version'],{encoding:'utf8',timeout:5000}).trim();if(version!=='v24.21.0')throw Error('REMOTE_NODE_VERSION');
const manifest={version:1,nodeVersion:'24.21.0',platform,archiveName,archiveSha256:pin,nodeSha256:sha(node),deploymentCommit:source.commit,files:Object.fromEntries(await Promise.all([...Object.keys(source.files).map(n=>'vendor/deployment/src/'+n),'vendor/deployment/LICENSE','scripts/remote-runner.mjs','scripts/hosted-runner.mjs','scripts/hosted-recovery-runner.mjs','scripts/publication-runner.mjs','scripts/publication-client.mjs','src-tauri/remote-runtime/generated/NODE-LICENSE'].map(async name=>[name.replace('src-tauri/',''),sha(await readFile(new URL('../'+name,import.meta.url)))])))};
await writeFile(new URL('manifest.json',out),JSON.stringify(manifest)+'\n',{mode:0o600});console.log(JSON.stringify({prepared:true,...manifest,networkAcquisition:false}));
