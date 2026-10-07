// SPDX-License-Identifier: Apache-2.0
import test from 'node:test';
import assert from 'node:assert/strict';
import { verifyImportedScene } from './qualify-imported-scene.mjs';
const old='10000000-0000-4000-8000-000000000001',fresh='20000000-0000-4000-8000-000000000001';
function fixture(){
  // Material dictionaries can precede entity definitions after canonical OEX/JSONB sorting.
  const source={revision:7,artworks:[],mediaAssets:[],extensions:{'org.exhibitos.studio/materials':{version:1,surfaces:{[old]:{color:'#aabbcc',roughness:.6}}}},surfaces:[{id:old}]};
  const destination={...structuredClone(source),revision:1,surfaces:[{id:fresh}]};
  destination.extensions['org.exhibitos.studio/materials'].surfaces={[fresh]:{color:'#aabbcc',roughness:.6}};
  return {source,destination,receipt:{idMap:{[old]:fresh}}};
}
test('whole comparison accepts independently observed remapped material keys before entity definitions',()=>{const {source,destination,receipt}=fixture();assert.equal(verifyImportedScene(source,destination,receipt,'tenant').completeSceneCompared,true);});
test('whole comparison still rejects changed material properties and orphaned surface keys',()=>{
  for(const kind of ['property','orphan']){const {source,destination,receipt}=fixture();const materials=destination.extensions['org.exhibitos.studio/materials'].surfaces;if(kind==='property')materials[fresh].roughness=.9;else{materials[old]=materials[fresh];delete materials[fresh];}assert.throws(()=>verifyImportedScene(source,destination,receipt,'tenant'),/IMPORTED_SCENE_CHANGED/);}
});
