// SPDX-License-Identifier: Apache-2.0
import {invoke,isTauri} from '@tauri-apps/api/core';
export type ProcessingAppAction='status'|'choose'|'open-detected'|'open-selected';
export interface ProcessingApplication{path:string;name:string;version:string;bundleId:string}
export interface ProcessingAppContext{supported:boolean;detectedCount:number;detected:ProcessingApplication|null;selected:ProcessingApplication|null;selectionUnavailable:boolean;storeAvailable:false}
const object=(v:unknown):v is Record<string,unknown>=>!!v&&typeof v==='object'&&!Array.isArray(v);
function app(v:unknown):v is ProcessingApplication{return object(v)&&Object.keys(v).sort().join(',')==='bundleId,name,path,version'&&typeof v.path==='string'&&v.path.startsWith('/')&&v.path.endsWith('.app')&&v.path.length<=2048&&!/[\x00-\x1f\x7f]/.test(v.path)&&typeof v.name==='string'&&v.name.length<=128&&typeof v.version==='string'&&v.version.length<=128&&typeof v.bundleId==='string'&&/^[A-Za-z0-9._-]{1,256}$/.test(v.bundleId);}
export function parseProcessingAppContext(v:unknown):ProcessingAppContext{
 if(!object(v)||Object.keys(v).sort().join(',')!=='detected,detectedCount,selected,selectionUnavailable,storeAvailable,supported'||typeof v.supported!=='boolean'||!Number.isSafeInteger(v.detectedCount)||(v.detectedCount as number)<0||(v.detectedCount as number)>128||!(v.detected===null||app(v.detected)&&v.detected.bundleId==='org.exhibitos.processing'&&v.detectedCount===1)||!(v.selected===null||app(v.selected))||typeof v.selectionUnavailable!=='boolean'||v.storeAvailable!==false)throw Error('MANAGER_PROTOCOL');return v as unknown as ProcessingAppContext;
}
export const processingAppClient={native:()=>isTauri(),operation:async(action:ProcessingAppAction)=>{if(!['status','choose','open-detected','open-selected'].includes(action))throw Error('MANAGER_PROTOCOL');return parseProcessingAppContext(await invoke('manager_processing_app',{action}));}};
