// SPDX-License-Identifier: Apache-2.0
import {it,expect} from 'vitest';
import {selectedEngineReady} from './selected-engine';
import {parseStatus,type EngineProbe,type Status} from './lifecycle';
const engines:EngineProbe[]=['docker','podman'].map(kind=>({kind,installed:true,available:kind==='podman',engineVersion:null,composeVersion:null,errorCode:null,guidance:null}));
const status:Status={installed:false,requiredEngine:null,bundleId:null,version:null,state:'not_installed',services:[],readiness:{ready:false,version:null,protocolVersion:null,errorCode:null},storage:{usedBytes:null,freeBytes:1,minimumFreeBytes:0},activeJob:null};
it('keeps a fresh unknown bundle installable, but honors known affinity',()=>{
 expect(selectedEngineReady(status,engines)).toBe(true);
 expect(selectedEngineReady({...status,requiredEngine:'docker'},engines)).toBe(false);
 expect(selectedEngineReady({...status,requiredEngine:'podman'},engines)).toBe(true);
 expect(selectedEngineReady(status,[])).toBe(false);
});
it('accepts older status replies and rejects unrecognized engine metadata',()=>{
 const legacy={...status};delete legacy.requiredEngine;expect(parseStatus(legacy)).toEqual(legacy);
 expect(parseStatus({...status,requiredEngine:'docker'}).requiredEngine).toBe('docker');
 expect(()=>parseStatus({...status,requiredEngine:'other'})).toThrow('MANAGER_PROTOCOL');
});
