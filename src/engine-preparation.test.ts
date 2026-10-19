// SPDX-License-Identifier: Apache-2.0
import {describe,it,expect} from 'vitest';
import {parseEnginePreparation} from './engine-preparation';
const context={jobId:null,stage:'not-started',savedBytes:0,totalBytes:589939097,version:'4.95.0',errorCode:null};
describe('closed official engine preparation wire',()=>{
 it('accepts observed bytes and explicit interrupted/user-action phases',()=>{for(const stage of ['downloading','verifying','installing','awaiting-installation','awaiting-engine','interrupted','cancelled','ready'])expect(parseEnginePreparation({...context,stage,savedBytes:100}).savedBytes).toBe(100);});
 it('refuses arbitrary providers paths URLs unbounded counts and invented progress',()=>{for(const value of [{...context,url:'https://other.invalid'},{...context,path:'/private/tmp/arbitrary'},{...context,version:'latest'},{...context,savedBytes:589939098},{...context,savedBytes:-1},{...context,stage:'success-assumed'},{...context,totalBytes:0},{...context,jobId:'../other'}])expect(()=>parseEnginePreparation(value)).toThrow('MANAGER_PROTOCOL');});
});
