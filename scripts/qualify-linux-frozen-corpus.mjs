// SPDX-License-Identifier: Apache-2.0
// Deployed public HTTP corpus. No injected app or direct database fixture writes.
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {createHash, randomUUID} from 'node:crypto';
import {createRequire} from 'node:module';
import {join,dirname} from 'node:path';
import {pathToFileURL} from 'node:url';
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
export async function frozenCorpus({platform,api,prefix,artist,rights,onStage=()=>{}}) {
  const load=path=>import(pathToFileURL(join(platform,path)));
  const {loadViewerFixtures}=await load('scripts/viewer-fixtures.mjs');
  const {exportedEntries}=await load('scripts/oex-test-zip.mjs');
  const {syntheticWav}=await load('scripts/experience-fixture.mjs');
  const {navigationFixture}=await load('scripts/navigation-fixture.mjs');
  const {oexFixture}=await load('scripts/oex-fixture.mjs');
  const {fixtureURL,validateExhibition,validateOex}=await import(pathToFileURL(join(dirname(createRequire(join(platform,'package.json')).resolve('@exhibitos/spec/package.json')),'index.mjs')));
  const {verifyFreezeBundle}=await load('packages/studio-contract/dist/index.js');
  const works=[],assets=[];
  for(const fixture of loadViewerFixtures()) {
    const artwork=await(await api('POST',prefix+'/cms/artworks',{artistId:artist.id,title:'Original OEX '+fixture.type,description:'Preserve original metadata and punctuation <script>literal</script>.',medium:'Original synthetic medium',creationYear:2024,dimensions:{width:1,height:1,depth:fixture.type==='image'?0.02:1,unit:'m'},rights,provenance:{source:'human-authored',sourceUnits:'m',scaleApplied:true,notes:'Synthetic preservation provenance'}},201)).json();
    const job=await(await api('POST',prefix+'/imports',{artworkId:artwork.id,idempotencyKey:randomUUID(),mime:fixture.type==='image'?'image/png':'model/gltf-binary',sha256:hash(fixture.bytes),bytes:fixture.bytes.length,scaleMeters:1,rights},201)).json();
    await api('PUT',prefix+'/imports/'+job.id+'/bytes',fixture.bytes);await api('POST',prefix+'/imports/'+job.id+'/complete');
    let asset;for(let i=0;i<240;i++){const done=await(await api('GET',prefix+'/imports/'+job.id)).json();assert.notEqual(done.state,'failed','CORPUS_IMPORT_FAILED');if(done.state==='approved'){asset=done.assetId;break;}await new Promise(r=>setTimeout(r,250));}assert(asset,'CORPUS_IMPORT_NOT_COMPLETED');
    await api('POST',prefix+'/cms/artworks/'+artwork.id+'/approve',{revision:1,assetId:asset});
    works.push((await(await api('GET',prefix+'/studio/artworks/'+artwork.id)).json()).artwork);
    assets.push({id:asset,sha256:hash(fixture.bytes),bytes:fixture.bytes.length});
  }
  const template=JSON.parse(await readFile(fixtureURL('oes/v1/examples/exhibition.json'),'utf8'));
  const base=navigationFixture(template,works).document,at=new Date().toISOString();
  const baseDraft={schemaVersion:'1.0.0-draft.1',kind:'exhibition-draft',id:randomUUID(),exhibitionId:base.id,editVersion:1,createdAt:at,updatedAt:at,candidate:base};
  const path=prefix+'/studio/exhibitions/'+base.id;
  const first=await(await api('POST',prefix+'/studio/exhibitions',{draft:baseDraft,requestId:randomUUID()},201)).json();
  const wave=syntheticWav(),audioPath=path+'/audio';
  const audio=await(await api('POST',audioPath,{requestId:randomUUID(),mime:'audio/wav',bytes:wave.length,sha256:hash(wave),rights},201)).json();
  await api('PUT',audioPath+'/'+audio.id+'/bytes',wave);
  const approved=await(await api('POST',audioPath+'/'+audio.id+'/approve',{revision:1})).json();
  assets.push({id:audio.id,sha256:hash(wave),bytes:wave.length,audio:true});
  const draft=oexFixture(template,works,approved.mediaAsset);draft.id=baseDraft.id;draft.exhibitionId=base.id;draft.candidate.id=base.id;draft.editVersion=2;
  assert.equal(validateExhibition(draft.candidate).valid,true,'CORPUS_SCENE_INVALID');
  const saved=await(await api('PUT',path,{draft,requestId:randomUUID()},200,{'if-match':first.etag})).json();
  const create=async()=> (await(await api('POST',path+'/freezes',{requestId:randomUUID()},201,{'if-match':saved.etag})).json());
  const active=await create(),revoked=await create(),activePath=path+'/freezes/'+active.id,revokedPath=path+'/freezes/'+revoked.id;
  await api('POST',revokedPath+'/revoke',{});
  const bundle=await(await api('POST',activePath+'/offline',{seconds:28800})).json();
  const trustedKeys=[bundle.manifest.authority.keyId];
  const verified=await verifyFreezeBundle(Buffer.from(JSON.stringify(bundle)),{trustedKeys});assert.equal(verified.bundle.manifest.id,active.id);
  assert.equal((await validateOex(Buffer.from(bundle.oex,'base64'))).valid,true,'CORPUS_OEX_INVALID');
  for(const asset of assets){if(asset.audio){await api('GET',prefix+'/assets/'+asset.id+'/bytes',undefined,403);const included=exportedEntries(Buffer.from(bundle.oex,'base64')).find(([name])=>name===approved.mediaAsset.path);assert(included,'CORPUS_AUDIO_MISSING');assert.equal(hash(included[1]),asset.sha256);}else{assert.equal(hash(Buffer.from(await(await api('GET',prefix+'/assets/'+asset.id+'/bytes')).arrayBuffer())),asset.sha256);}}
  const exported=Buffer.from(await(await api('POST',path+'/oex/export',{},200,{'if-match':saved.etag})).arrayBuffer());
  assert.equal((await validateOex(exported)).valid,true,'EXPORTED_OEX_INVALID');
  const edited=structuredClone(draft);edited.editVersion++;edited.updatedAt=new Date().toISOString();edited.candidate.title='Synthetic next revision after immutable freeze';
  const latest=await(await api('PUT',path,{draft:edited,requestId:randomUUID()},200,{'if-match':saved.etag})).json();
  const originalHistory=await(await api('GET',activePath)).json(),revokedHistory=await(await api('GET',revokedPath)).json();
  return {
    report:{exhibitionId:base.id,activeFreeze:active.id,revokedFreeze:revoked.id,manifestSha256:active.manifestSha256,exportedOexSha256:hash(exported),assets:assets.map(({bytes,sha256})=>({bytes,sha256}))},
    async changedOrigin(restoredApi) {
      const failure=await(await restoredApi('GET',activePath+'/check',undefined,403)).json();assert.equal(failure.code,'FREEZE_AUTHORITY_CHANGED');
      assert.deepEqual((await(await restoredApi('GET',activePath)).json()).manifest,active.manifest);
    },
    async sameOrigin(restoredApi) {
      assert.deepEqual((await(await restoredApi('GET',path)).json()),latest,'RESTORED_DRAFT_CHANGED');
      for(const work of works)assert.deepEqual((await(await restoredApi('GET',prefix+'/studio/artworks/'+work.id)).json()).artwork,work,'RESTORED_ARTWORK_CHANGED');
      for(const asset of assets){onStage(asset.audio?'restored-audio-access':'restored-artwork-bytes');if(asset.audio){assert.deepEqual(await(await restoredApi('GET',audioPath+'/'+asset.id)).json(),approved,'RESTORED_AUDIO_METADATA_CHANGED');await restoredApi('GET',prefix+'/assets/'+asset.id+'/bytes',undefined,403);}else{const bytes=Buffer.from(await(await restoredApi('GET',prefix+'/assets/'+asset.id+'/bytes')).arrayBuffer());assert.equal(bytes.length,asset.bytes);assert.equal(hash(bytes),asset.sha256);}}
      onStage('restored-freeze-history');
      assert.deepEqual(await(await restoredApi('GET',activePath)).json(),originalHistory,'RESTORED_FREEZE_HISTORY_CHANGED');
      assert.deepEqual(await(await restoredApi('GET',revokedPath)).json(),revokedHistory,'RESTORED_REVOCATION_CHANGED');
      assert.equal((await(await restoredApi('GET',revokedPath+'/check',undefined,403)).json()).code,'FREEZE_REVOKED');
      onStage('restored-offline-bytes');const fresh=await(await restoredApi('POST',activePath+'/offline',{seconds:28800})).json();
      const pcm=exportedEntries(Buffer.from(fresh.oex,'base64')).find(([name])=>name===approved.mediaAsset.path);assert(pcm,'RESTORED_AUDIO_BYTES_MISSING');assert.equal(pcm[1].length,wave.length);assert.equal(hash(pcm[1]),hash(wave),'RESTORED_AUDIO_BYTES_CHANGED');
      for(const field of ['manifest','signature','oex','runtimeFiles'])assert.deepEqual(fresh[field],bundle[field],'RESTORED_FROZEN_BYTES_CHANGED');
      await verifyFreezeBundle(Buffer.from(JSON.stringify(bundle)),{trustedKeys});await verifyFreezeBundle(Buffer.from(JSON.stringify(fresh)),{trustedKeys});
      const altered=structuredClone(fresh);altered.runtimeFiles[0].data=Buffer.from('tamper').toString('base64');await assert.rejects(verifyFreezeBundle(Buffer.from(JSON.stringify(altered)),{trustedKeys}),/FREEZE_RUNTIME_INTEGRITY/);
      await assert.rejects(verifyFreezeBundle(Buffer.from(JSON.stringify(fresh)),{trustedKeys:[]}),/FREEZE_AUTHORITY_UNTRUSTED/);
      onStage('restored-oex-import');const imported=await(await restoredApi('POST',prefix+'/oex/imports',{requestId:randomUUID(),bytes:exported.length,sha256:hash(exported)},201)).json();
      await restoredApi('PUT',prefix+'/oex/imports/'+imported.id+'/bytes',exported);await restoredApi('POST',prefix+'/oex/imports/'+imported.id+'/complete',{});
      let result;for(let i=0;i<240;i++){const job=await(await restoredApi('GET',prefix+'/oex/imports/'+imported.id)).json();assert.notEqual(job.state,'failed','RESTORED_OEX_IMPORT_FAILED');if(job.state==='complete'){result=job.result;break;}await new Promise(r=>setTimeout(r,250));}assert(result,'RESTORED_OEX_IMPORT_NOT_COMPLETED');
      const importedDraft=await(await restoredApi('GET',prefix+'/studio/exhibitions/'+result.exhibitionId)).json();assert.equal(importedDraft.draft.candidate.title,draft.candidate.title);assert.notEqual(result.exhibitionId,base.id);
      assert.deepEqual((await(await restoredApi('GET',path)).json()),latest,'OEX_IMPORT_CHANGED_ORIGINAL_DRAFT');
      assert.deepEqual((await(await restoredApi('GET',activePath)).json()).manifest,active.manifest,'OEX_IMPORT_CHANGED_ORIGINAL_FREEZE');
      return {...this.report,importedExhibitionId:result.exhibitionId,oldAndNewGrantsVerified:true,offlineRuntimeInventoryPreserved:true,portableBrowser:'NOT_RUN'};
    }
  };
}
