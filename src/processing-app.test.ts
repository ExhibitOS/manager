// SPDX-License-Identifier: Apache-2.0
import {expect,it} from 'vitest';
import {parseProcessingAppContext} from './processing-app';
const app={path:'/Applications/Synthetic.app',name:'Synthetic',version:'1',bundleId:'org.exhibitos.processing'},base={supported:true,detectedCount:0,detected:null,selected:null,selectionUnavailable:false,storeAvailable:false};
it('accepts absent, multiple and explicitly selected tools without a Store claim',()=>{expect(parseProcessingAppContext(base)).toEqual(base);expect(parseProcessingAppContext({...base,detectedCount:2}).detected).toBeNull();expect(parseProcessingAppContext({...base,detectedCount:1,detected:app,selected:app}).selected).toEqual(app);});
it('refuses executable paths, unknown fields and invented Store verification',()=>{for(const invalid of [{...base,storeAvailable:true},{...base,token:'secret'},{...base,selected:{...app,path:'/bin/sh'}},{...base,detected:app},{...base,detectedCount:129},{...base,selected:{...app,bundleId:'bad/value'}}])expect(()=>parseProcessingAppContext(invalid)).toThrow('MANAGER_PROTOCOL');});
