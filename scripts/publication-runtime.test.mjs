// SPDX-License-Identifier: Apache-2.0
import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,writeFile,readFile,rm,lstat,copyFile,symlink} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {preparePublicationRuntime,allowedViewerPath} from './prepare-publication-runtime.mjs';
test('Viewer allowlist excludes CMS, secret and executable deployment files',()=>{
 for(const p of ['static.html','THIRD_PARTY_NOTICES.txt','voice-pcm-worklet.js','assets/runtime.js','basis/basis_transcoder.wasm'])assert(allowedViewerPath(p),p);
 for(const p of ['index.html','_worker.js','.env','assets/private.json','assets/nested/code.js','assets/../secret.js'])assert(!allowedViewerPath(p),p);
});
const platformRoot=process.env.EXHIBITOS_PUBLICATION_TEST_PLATFORM;
test('isolated locked production closure runs original exporter success, corruption, withdrawal and preservation regressions',{skip:!platformRoot},async t=>{
 const temporary=await mkdtemp(join(tmpdir(),'exhibitos-manager-publication-package-'));t.after(()=>rm(temporary,{recursive:true,force:true}));
 const viewer=join(temporary,'synthetic-viewer'),output=join(temporary,'runtime');await mkdir(viewer);
 await writeFile(join(viewer,'static.html'),'<meta content="STATIC_INDEX_SHA256">');
 await writeFile(join(viewer,'THIRD_PARTY_NOTICES.txt'),'Synthetic test runtime; no browser dependencies.');await writeFile(join(viewer,'voice-pcm-worklet.js'),'// synthetic');
 const receipt=await preparePublicationRuntime({platformRoot,output,viewerRoot:viewer});assert(receipt.prepared);assert.equal(receipt.networkAcquisition,false);
 t.diagnostic(JSON.stringify(receipt));
 const manifest=JSON.parse(await readFile(join(output,'manifest.json'),'utf8'));assert.equal(manifest.entry,'exporter.mjs');
 assert.deepEqual(manifest.links,{});assert.equal(receipt.workspaceLinks,0);
 for(const [path,pin]of Object.entries(manifest.files)){assert(!(await lstat(join(output,path))).isSymbolicLink(),path);const bytes=await readFile(join(output,path));assert.equal(bytes.length,pin.bytes,path);assert.equal(createHash('sha256').update(bytes).digest('hex'),pin.sha256,path);}
 for(const workspace of ['storage','studio-contract']){const s=await lstat(join(output,'platform/node_modules/@exhibitos',workspace));assert(s.isDirectory());assert(!s.isSymbolicLink());}
 assert(!Object.keys(manifest.files).some(p=>p.includes('/operations/')||p.includes('/Capture/')||p.includes('/.local/')));
 // The original tests and fixture decoder now resolve only inside the packaged module tree.
 const packagedTest=join(output,'platform/scripts/export-static-publication.test.mjs');
 await copyFile(join(platformRoot,'scripts/export-static-publication.test.mjs'),packagedTest);
 const result=execFileSync(process.execPath,['--test','--test-reporter=tap',packagedTest],{cwd:temporary,encoding:'utf8',timeout:60000,env:{PATH:process.env.PATH}});
 assert.match(result,/# fail 0/);assert.match(result,/# pass 10/);
 await assert.rejects(preparePublicationRuntime({platformRoot,output,viewerRoot:viewer}),/PUBLICATION_OUTPUT_EXISTS/);
 await writeFile(join(viewer,'_worker.js'),'private');const rejected=join(temporary,'rejected');
 await assert.rejects(preparePublicationRuntime({platformRoot,output:rejected,viewerRoot:viewer}),/PUBLICATION_VIEWER_UNLISTED/);await assert.rejects(lstat(rejected),{code:'ENOENT'});
 await rm(join(viewer,'_worker.js'));await symlink(join(viewer,'static.html'),join(viewer,'assets.js'));
 await assert.rejects(preparePublicationRuntime({platformRoot,output:join(temporary,'symlink-rejected'),viewerRoot:viewer}),/PUBLICATION_SYMLINK/);
});
