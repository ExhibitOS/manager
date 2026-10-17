// SPDX-License-Identifier: Apache-2.0
export interface InitialAccountInput {subject:string;password:string}
export interface InitialAccountContext {state:'required'|'existing';tenantId:string|null;subject:string|null}
export const validInitialSubject=(value:string)=>/^[a-zA-Z0-9][a-zA-Z0-9_.@-]{2,127}$/.test(value);
export const validInitialPassword=(value:string)=>{const bytes=new TextEncoder().encode(value).byteLength;return bytes>=12&&bytes<=1024&&!/[\p{Cc}]/u.test(value);};
export function parseInitialAccountContext(value:unknown):InitialAccountContext {
 if(!value||typeof value!=='object'||Array.isArray(value))throw Error('MANAGER_PROTOCOL');
 const v=value as Record<string,unknown>;
 if(Object.keys(v).sort().join(',')!=='state,subject,tenantId')throw Error('MANAGER_PROTOCOL');
 if(v.state==='required'&&v.tenantId===null&&v.subject===null)return v as unknown as InitialAccountContext;
 if(v.state==='existing'&&typeof v.tenantId==='string'&&/^[a-f0-9]{8}-[a-f0-9]{4}-[1-8][a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/i.test(v.tenantId)&&typeof v.subject==='string'&&validInitialSubject(v.subject))return v as unknown as InitialAccountContext;
 throw Error('MANAGER_PROTOCOL');
}
