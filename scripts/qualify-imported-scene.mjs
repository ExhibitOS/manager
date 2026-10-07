// SPDX-License-Identifier: Apache-2.0
import assert from 'node:assert/strict';
import { isDeepStrictEqual } from 'node:util';
const uuid=/^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
// Public entity/reference field contract. Authored text is never rewritten.
const references=new Set(['id','revisionId','primaryAssetId','sourceAssetIds','appliedToAssetIds','roomId','surfaceId','connectsToOpeningId','artworkRevisionId','assetId','targetPlacementId','viaOpeningId','routeIds','placementId','targetId','annotationId','zoneId','routeId']);
/** Compare the complete rich fixture, inferring identity pairs from observed
 * source/destination entities rather than using the producer's remapping function. */
export function verifyImportedScene(source,destination,receipt,tenantId) {
  assert.equal(destination.revision,1,'IMPORTED_REVISION_INVALID');
  const prepared=structuredClone(destination),pairs=new Map(),inverse=new Map();
  assert.equal(prepared.artworks.length,source.artworks.length,'IMPORTED_ARTWORK_COUNT_CHANGED');
  for(const [i,artwork]of prepared.artworks.entries()) {
    assert.equal(artwork.revision,1,'IMPORTED_ARTWORK_REVISION_INVALID');assert.deepEqual(artwork.extensions?.['org.exhibitos.studio/cms'],{tenantId,artworkId:artwork.id,revision:1},'IMPORTED_CMS_BINDING_INVALID');
    delete artwork.extensions['org.exhibitos.studio/cms'];if(Object.keys(artwork.extensions).length===0)delete artwork.extensions;
    artwork.revision=source.artworks[i].revision;
  }
  assert.equal(prepared.mediaAssets.length,source.mediaAssets.length,'IMPORTED_MEDIA_COUNT_CHANGED');
  for(const [i,media]of prepared.mediaAssets.entries()){assert.equal(media.path,'media/'+media.id+'/audio.wav','IMPORTED_MEDIA_PATH_INVALID');media.path=source.mediaAssets[i].path;}
  prepared.revision=source.revision;
  const bind=(before,after)=> {
    assert(uuid.test(before)&&uuid.test(after),'IMPORTED_ID_INVALID');assert.notEqual(before.toLowerCase(),after.toLowerCase(),'IMPORTED_ID_NOT_FRESH');
    if(pairs.has(before.toLowerCase()))assert.equal(pairs.get(before.toLowerCase()),after.toLowerCase(),'IMPORTED_ID_INCONSISTENT');
    if(inverse.has(after.toLowerCase()))assert.equal(inverse.get(after.toLowerCase()),before.toLowerCase(),'IMPORTED_ID_COLLISION');
    pairs.set(before.toLowerCase(),after.toLowerCase());inverse.set(after.toLowerCase(),before.toLowerCase());
  };
  const observe=(before,after,scope=[])=> {
    if(Array.isArray(before)){assert(Array.isArray(after),'IMPORTED_ARRAY_TYPE_CHANGED');assert.equal(after.length,before.length,'IMPORTED_ARRAY_COUNT_CHANGED');before.forEach((v,i)=>observe(v,after[i],scope));}
    else if(before&&typeof before==='object'){assert(after&&typeof after==='object','IMPORTED_OBJECT_TYPE_CHANGED');for(const[key,value]of Object.entries(before)){if((key==='id'||key==='revisionId')&&typeof value==='string')bind(value,after[key]);else if(key==='surfaces'&&!Array.isArray(value)&&scope.includes('org.exhibitos.studio/materials')){ /* UUID dictionary keys are compared after all entity pairs are observed. */ }else observe(value,after[key],[...scope,key]);}}
  };
  observe(source,prepared);
  assert.deepEqual(Object.fromEntries(pairs),receipt.idMap,'IMPORTED_RECEIPT_MAPPING_INVALID');
  const normalize=(value,field='',scope=[])=> {
    if(typeof value==='string'){if(references.has(field)){assert(!pairs.has(value.toLowerCase()),'IMPORTED_SOURCE_REFERENCE_REUSED');return inverse.get(value.toLowerCase())??value;}return value;}
    if(Array.isArray(value))return value.map(v=>normalize(v,field,scope));
    if(value&&typeof value==='object')return Object.fromEntries(Object.entries(value).map(([key,v])=>[
      field==='surfaces'&&scope.includes('org.exhibitos.studio/materials')?normalize(key,'surfaceId'):key,
      normalize(v,key,[...scope,key])
    ]));
    return value;
  };
  const normalized=normalize(prepared);
  // Emit only contract field names/array indices, never values, UUIDs or raw diffs.
  const fieldMismatch=(expected,actual,path=[])=> {
    if(isDeepStrictEqual(expected,actual))return null;
    if(expected&&actual&&typeof expected==='object'&&typeof actual==='object'){
      const keys=new Set([...Object.keys(expected),...Object.keys(actual)]);
      for(const key of keys){const found=fieldMismatch(expected[key],actual[key],[...path,/^[a-zA-Z0-9_]+$/.test(key)?key:'KEY']);if(found)return found;}
    }
    return path.join('_').toUpperCase();
  };
  const mismatch=fieldMismatch(source,normalized);
  assert.deepEqual(normalized,source,('IMPORTED_SCENE_CHANGED_'+(mismatch??'ROOT')).slice(0,79));
  const original=new Set(pairs.keys());assert([...inverse.keys()].every(id=>!original.has(id)),'IMPORTED_SOURCE_ID_REUSED');
  return {completeSceneCompared:true,independentlyObservedIdentityPairs:pairs.size,opaqueProsePreserved:true};
}
