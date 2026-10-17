// SPDX-License-Identifier: Apache-2.0
import {test,expect} from '@playwright/test';
test('local published catalog prepares the selected exhibition without exposing credentials or deploying',async({page})=>{
 await page.addInitScript(()=>{
  const w=window as unknown as {isTauri:boolean;publicationCalls:{command:string,args:Record<string,unknown>}[];__TAURI_INTERNALS__:{invoke:(command:string,args:Record<string,unknown>)=>Promise<unknown>}};w.isTauri=true;w.publicationCalls=[];
  w.__TAURI_INTERNALS__={invoke:async(command,args={})=>{
   w.publicationCalls.push({command,args});
   if(command==='manager_installations')return {selectionToken:'00000000-0000-4000-8000-000000000001',activeId:'00000000-0000-4000-8000-000000000002',mode:'managed',errorCode:null,installations:[{id:'00000000-0000-4000-8000-000000000002',kind:'default',createdAt:1,path:'/private/tmp/synthetic-profile/local-runtime',available:true}]};
   if(command==='manager_hosted_resume')return null;if(command==='manager_hosted_connected')return false;
   if(command==='manager_publication_catalog')return {items:[{publicationId:'00000000-0000-4000-8000-000000000003',exhibitionId:'00000000-0000-4000-8000-000000000004',title:'나의 공개 전시',revisionSha256:'a'.repeat(64),publishedAt:'2026-10-17T00:00:00.000Z',status:'published'}]};
   if(command==='manager_publication_export')return {title:'나의 공개 전시',publicationId:'00000000-0000-4000-8000-000000000003',projectName:'exhibition-publication'};
   throw {code:'MANAGER_CONNECTION'};
  }};
 });
 await page.goto('/');const section=page.getByRole('region',{name:'전시를 웹에 공개하기'});
 await section.getByRole('button',{name:'로컬 공개 전시 불러오기'}).click();await expect(section.getByLabel('공개할 전시')).toHaveValue('00000000-0000-4000-8000-000000000003');await section.getByRole('button',{name:'선택한 전시 준비'}).click();await expect(section.getByText('선택한 전시: 나의 공개 전시')).toBeVisible();
 const calls=await page.evaluate(()=>(window as unknown as {publicationCalls:{command:string;args:Record<string,unknown>}[]}).publicationCalls);
 expect(calls.filter(c=>c.command==='manager_publication_export').map(c=>c.args)).toEqual([{selectionToken:'00000000-0000-4000-8000-000000000001',publicationId:'00000000-0000-4000-8000-000000000003'}]);expect(calls.filter(c=>c.command==='manager_hosted')).toHaveLength(0);expect(JSON.stringify(calls)).not.toContain('password');await expect(section.getByRole('button',{name:'선택한 전시 배포'})).toBeDisabled();
});
