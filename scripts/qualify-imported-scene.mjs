// SPDX-License-Identifier: Apache-2.0
import assert from 'node:assert/strict';
const uuid=/^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
// Public entity/reference field contract. Authored text is never rewritten.
const references=new Set(['id','revisionId','primaryAssetId','sourceAssetIds','appliedToAssetIds','roomId','surfaceId','connectsToOpeningId','artworkRevisionId','assetId','targetPlacementId','viaOpeningId','routeIds','placementId','targetId','annotationId','zoneId','routeId']);
/** Compare the complete rich fixture, inferring identity pairs from observed
 * source/destination entities rather than using the producer's remapping function. */
export function verifyImportedScene(source,destination,receipt,tenantId) {
  assert.equal(destination.revision,1,'IMPORTED_REVISION_INVALID');
  const prepared=structuredClone(destination),pairs=new Map(),inverse=new Map();
  assert.equal(prepared.artworks.length,source.artworks.length);
  for(const [i,artwork]of prepared.artworks.entries()) {
    assert.equal(artwork.revision,1);assert.deepEqual(artwork.extensions?.['org.exhibitos.studio/cms'],{tenantId,artworkId:artwork.id,revision:1});
    delete artwork.extensions['org.exhibitos.studio/cms'];if(Object.keys(artwork.extensions).length===0)delete artwork.extensions;
    artwork.revision=source.artworks[i].revision;
  }
  assert.equal(prepared.mediaAssets.length,source.mediaAssets.length);
  for(const [i,media]of prepared.mediaAssets.entries()){assert.equal(media.path,'media/'+media.id+'/audio.wav');media.path=source.mediaAssets[i].path;}
  prepared.revision=source.revision;
  const bind=(before,after)=> {
    assert(uuid.test(before)&&uuid.test(after),'IMPORTED_ID_INVALID');assert.notEqual(before.toLowerCase(),after.toLowerCase(),'IMPORTED_ID_NOT_FRESH');
    if(pairs.has(before.toLowerCase()))assert.equal(pairs.get(before.toLowerCase()),after.toLowerCase(),'IMPORTED_ID_INCONSISTENT');
    if(inverse.has(after.toLowerCase()))assert.equal(inverse.get(after.toLowerCase()),before.toLowerCase(),'IMPORTED_ID_COLLISION');
    pairs.set(before.toLowerCase(),after.toLowerCase());inverse.set(after.toLowerCase(),before.toLowerCase());
  };
  const observe=(before,after)=> {
    if(Array.isArray(before)){assert(Array.isArray(after));assert.equal(after.length,before.length);before.forEach((v,i)=>observe(v,after[i]));}
    else if(before&&typeof before==='object'){assert(after&&typeof after==='object');for(const[key,value]of Object.entries(before)){if((key==='id'||key==='revisionId')&&typeof value==='string')bind(value,after[key]);else observe(value,after[key]);}}
  };
  observe(source,prepared);
  assert.deepEqual(Object.fromEntries(pairs),receipt.idMap,'IMPORTED_RECEIPT_MAPPING_INVALID');
  const normalize=(value,field='',scope=[])=> {
    if(typeof value==='string')return references.has(field)?inverse.get(value.toLowerCase())??value:value;
    if(Array.isArray(value))return value.map(v=>normalize(v,field,scope));
    if(value&&typeof value==='object')return Object.fromEntries(Object.entries(value).map(([key,v])=>[
      field==='surfaces'&&scope.includes('org.exhibitos.studio/materials')?inverse.get(key.toLowerCase())??key:key,
      normalize(v,key,[...scope,key])
    ]));
    return value;
  };
  assert.deepEqual(normalize(prepared),source,'IMPORTED_COMPLETE_SCENE_CHANGED');
  const original=new Set(pairs.keys());assert([...inverse.keys()].every(id=>!original.has(id)),'IMPORTED_SOURCE_ID_REUSED');
  return {completeSceneCompared:true,independentlyObservedIdentityPairs:pairs.size,opaqueProsePreserved:true};
}
