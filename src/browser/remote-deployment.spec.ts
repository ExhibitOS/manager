// SPDX-License-Identifier: Apache-2.0
import {test,expect} from '@playwright/test';
test('synthetic remote IPC shows owned progress/cancel/uncertain status and requires backup consent',async({page})=>{
 await page.addInitScript(()=>{
  type Input={requestId:string;action:string;maintenance:unknown;consent:Record<string,boolean>};
  const w=window as unknown as {isTauri:boolean;__TAURI_INTERNALS__:{invoke:(command:string,args?:{input?:Input})=>Promise<unknown>};remoteFixture:{calls:Input[];cancelId:string|null}};
  w.isTauri=true;w.remoteFixture={calls:[],cancelId:null};let owned:string|null=null;let rejectOwned:((v:unknown)=>void)|null=null;
  w.__TAURI_INTERNALS__={invoke:async(command,args)=>{
   if(command==='manager_remote_available')return true;
   if(command==='manager_remote_cancel'){const id=args?.input?.requestId;if(id!==owned)return false;w.remoteFixture.cancelId=id;rejectOwned?.({code:'SSH_OPERATION_UNCERTAIN',guidance:'실제 서버 상태를 확인하세요.'});owned=null;return true;}
   if(command!=='manager_remote')throw {code:'FIXTURE_UNAVAILABLE',guidance:'로컬 관리 공간은 합성 원격 검사에 포함되지 않습니다.'};
   const input=args!.input!;w.remoteFixture.calls.push(input);
   if(input.action==='start'){owned=input.requestId;return await new Promise((_,reject)=>{rejectOwned=reject;});}
   if(input.action==='status')return {version:1,action:'status',state:'stopped',ready:false,healthVerified:false};
   if(input.action==='backup')return {version:1,action:'backup',state:'completed',jobId:'12345678-1234-4234-8234-123456789abc',backupId:'22345678-1234-4234-8234-123456789abc',authenticatedManifestSha256:'a'.repeat(64),writersPaused:true,preservedVolumes:true};
   throw {code:'SSH_OPERATION_REFUSED',guidance:'합성 검사 범위 밖입니다.'};
  }};
 });
 await page.goto('/');await page.getByRole('button',{name:'고급 설정',exact:true}).click();await page.getByText('기존 서버 배포 도구',{exact:true}).click();const wizard=page.getByRole('region',{name:'배포 준비'});
 await wizard.getByLabel('배포 장소').selectOption('server');await wizard.getByLabel('검증한 실행 이미지 (저장소 digest 또는 로컬 이미지 ID)').fill('ghcr.io/exhibitos/platform@sha256:'+'a'.repeat(64));await wizard.getByLabel('서버 주소').fill('host.example.invalid');await wizard.getByLabel('서버 사용자 (root 제외)').fill('exhibitos');await wizard.getByRole('button',{name:'입력 확인·파일 준비'}).click();
 const remote=page.getByRole('region',{name:'보유 서버 연결·실행'});
 for(const[label,value]of [['서버의 전시 관리 폴더','/home/exhibitos/runtime'],['서버 Manager 실행파일 경로','/home/exhibitos/bin/manager'],['서버 Manager SHA256','a'.repeat(64)],['이 컴퓨터의 확인한 known_hosts 파일 위치','/tmp/fixture_known_hosts'],['known_hosts SHA256','b'.repeat(64)],['전시 공개 주소 (https://도메인/)','https://host.example.invalid/']])await remote.getByLabel(label!).fill(value!);
 const consent=async()=>{await remote.getByLabel(/지정한 서버 연결/).check();await remote.getByLabel(/서버에 이미 준비된/).check();};await consent();
 await remote.getByRole('button',{name:'서버 시작',exact:true}).click();await expect(remote.getByText(/서버 시작 진행 중/)).toBeVisible();await expect(remote.getByRole('button',{name:'실제 상태 확인',exact:true})).toBeDisabled();
 await remote.getByRole('button',{name:'연결 작업 중단',exact:true}).click();await expect(remote.getByRole('alert')).toContainText('SSH_OPERATION_UNCERTAIN');
 await remote.getByRole('button',{name:'실제 상태 확인',exact:true}).click();await expect(remote.getByText('상태: stopped',{exact:true})).toBeVisible();
 await remote.getByLabel('검증된 유지보수 이미지 digest').fill('sha256:'+'c'.repeat(64));await remote.getByLabel('서버 외부 키 파일 위치').fill('/home/exhibitos/keys/external.bin');await consent();
 const backup=remote.getByRole('button',{name:'인증된 전체 백업',exact:true});await expect(backup).toBeDisabled();await remote.getByLabel(/외부 쓰기 작업을 정지/).check();await remote.getByLabel(/서버 관리 공간을 사용하는 앱/).check();await backup.click();await expect(remote.getByText('상태: completed',{exact:true})).toBeVisible();await expect(remote.getByText('백업 ID: 22345678-1234-4234-8234-123456789abc',{exact:true})).toBeVisible();
 const actual=await page.evaluate(()=>{const w=window as unknown as {remoteFixture:{calls:{requestId:string;action:string;maintenance:unknown;consent:Record<string,boolean>}[];cancelId:string}};return w.remoteFixture;});expect(actual.calls.map(c=>c.action)).toEqual(['start','status','backup']);expect(actual.cancelId).toBe(actual.calls[0]!.requestId);expect(actual.calls[2]!.consent.externalWritersQuiesced).toBe(true);expect(actual.calls[2]!.consent.appsClosed).toBe(true);expect(actual.calls[2]!.maintenance).toEqual({maintenanceImage:'sha256:'+'c'.repeat(64),keyPath:'/home/exhibitos/keys/external.bin'});await expect(remote.locator('pre')).toHaveCount(0);
});
