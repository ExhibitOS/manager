// SPDX-License-Identifier: Apache-2.0
import {test,expect} from '@playwright/test';
// Real browser interactions, synthetic IPC only. No provider, keychain or OS picker is exercised.
test('hosted backup invalidates prior-source confirmation and keeps archive copy cancellation safe',async({page})=>{
 await page.addInitScript(()=>{
  const id='00000000-0000-4000-8000-000000000001';let source={workspaceId:'tea-a',serviceId:'srv-a',postgresId:'dpg-a'},jobs:unknown[]=[],restoreChecks:unknown[]=[];let copies=0,publicationCopied=true;
  let selectionToken='45678901-1234-1234-1234-123456789012',activeId='56789012-1234-1234-1234-123456789012';
  const installation=()=>({selectionToken,activeId,mode:'managed',installations:[{id:activeId,kind:'default',createdAt:1,path:'/private/tmp/synthetic-profile/local-runtime',available:true}],errorCode:null});
  const calls:unknown[]=[];Object.assign(window,{isTauri:true,__hostedRecoveryCalls:calls,__clearHostedPublication:()=>{publicationCopied=false;},__changeHostedSource:()=>{source={workspaceId:'tea-b',serviceId:'srv-b',postgresId:'dpg-b'};},__TAURI_INTERNALS__:{invoke:async(cmd:string,args?:{input?:{action:string;expectedSource?:unknown}})=>{
   if(cmd==='manager_installations')return installation();
   if(cmd==='manager_create_installation'){selectionToken='78901234-1234-1234-1234-123456789012';activeId='67890123-1234-1234-1234-123456789012';jobs=[];restoreChecks=[];return installation();}
   if(cmd==='manager_status')return {installed:true,bundleId:'synthetic',version:'0.1.0',state:'stopped',services:[],readiness:{ready:false,version:null,protocolVersion:null,errorCode:null},storage:{usedBytes:null,freeBytes:10000000000,minimumFreeBytes:0},activeJob:null};
   if(cmd==='manager_detect'||cmd==='manager_jobs'||cmd==='manager_logs'||cmd==='manager_backup_jobs'||cmd==='manager_helper_reconciliations'||cmd==='manager_maintenance_retries')return [];
   if(cmd==='manager_restoration_context')return {fresh:false,job:null,receipt:null};
   if(cmd==='manager_maintenance_context'||cmd==='manager_hosted_resume')return null;
   if(cmd==='manager_hosted_connected')return false;
   if(cmd==='manager_hosted_recovery'){
    const input=args!.input!;calls.push(input);let archiveSaved=null,publicationSaved=null;
    if(input.action==='create')jobs=[{id,source,image:`sha256:${'a'.repeat(64)}`,jobBudgetBytes:64*1024**3,state:'completed',stage:'completed',errorCode:null,createdAt:1,updatedAt:2,receipt:{provider:'render',action:'local-encrypted-backup',operation:'created-and-verified',...source,backupId:id,files:1,authenticatedManifestSha256:'b'.repeat(64)}}];
    if(input.action==='check-restore')restoreChecks=[{id:'00000000-0000-4000-8000-000000000002',sourceJobId:id,source,image:`sha256:${'a'.repeat(64)}`,expectedBackupId:id,expectedManifestSha256:'b'.repeat(64),expectedFiles:1,jobBudgetBytes:1024**3,state:'completed',stage:'completed',errorCode:null,receipt:{operation:'restored-and-verified',backupId:id,authenticatedManifestSha256:'b'.repeat(64),files:1},createdAt:2,updatedAt:3}];
    if(input.action==='export-archive'&&++copies>1&&copies!==4){if(copies===5)await new Promise<void>(resolve=>Object.assign(window,{__finishHostedArchive:resolve}));archiveSaved={jobId:id,backupId:id,path:'/synthetic/ExhibitOS-backup-copy',files:4,bytes:400};publicationSaved={jobId:id,state:copies===2?'unavailable':'copied',path:'/synthetic/ExhibitOS-publication',files:copies===2?0:3,bytes:copies===2?0:100};}
    return {publicationSaved,publicationCopies:jobs.map(()=>({jobId:id,state:publicationCopied?'copied':'unavailable',files:publicationCopied?3:0,bytes:publicationCopied?100:0})),source:input.action==='context'||input.action==='create'?source:null,jobs,restoreChecks,keySaved:false,archiveSaved};
   }
   throw {code:'MANAGER_CONNECTION',guidance:'Synthetic native bridge only'};
  }}});
 });
 const errors:string[]=[];page.on('pageerror',e=>errors.push(e.message));await page.goto('/');await page.getByRole('button',{name:'고급 설정',exact:true}).click();await page.getByLabel('API·데이터베이스 준비 · 추가 연결 필요').check();
 const panel=page.getByRole('region',{name:'원격 서버 백업'});await panel.getByText('서버 백업 관리',{exact:true}).click();
 const prepare=panel.getByRole('button',{name:'배포한 서버의 백업 준비'});await prepare.click();await panel.getByText('네트워크 접근 설정',{exact:true}).click();
 const cidr=panel.getByLabel('허용한 현재 Mac의 IP 범위'),consent=panel.getByLabel('Render에서 이 API를 일시 중지했고, 다른 앱의 DB 쓰기도 멈췄습니다.'),create=panel.getByRole('button',{name:'이 Mac에 암호화 백업 만들기'});
 await cidr.fill('192.0.2.3/32');await consent.check();await expect(create).toBeEnabled();
 await panel.getByRole('button',{name:'백업 기록 다시 확인',exact:true}).click();await expect(cidr).toBeVisible();await expect(cidr).toHaveValue('192.0.2.3/32');await expect(consent).toBeChecked();await expect(create).toBeEnabled();
 await page.evaluate(()=>(window as unknown as {__changeHostedSource:()=>void}).__changeHostedSource());await prepare.click();await expect(consent).not.toBeChecked();await expect(cidr).toHaveValue('');await expect(create).toBeDisabled();
 await cidr.fill('192.0.2.4/32');await consent.check();await create.click();await expect(panel.getByRole('heading',{name:'암호화 사본 생성·검증 완료'})).toBeVisible();
 await expect(panel).toContainText('이 Mac의 전시 파일 별도 사본 보관 기록이 있어요.');await expect(panel).toContainText('암호화 백업 사본에는 포함되지 않으며 전체 서비스 복원을 뜻하지 않습니다.');
 await page.evaluate(()=>(window as unknown as {__clearHostedPublication:()=>void}).__clearHostedPublication());await panel.getByRole('button',{name:'백업 기록 다시 확인',exact:true}).click();
 await expect(panel).toContainText('이 Mac의 전시 파일 별도 사본 보관은 아직 확인되지 않았어요.');await expect(panel.getByRole('heading',{name:'암호화 사본 생성·검증 완료'})).toBeVisible();await expect(panel.getByRole('button',{name:'암호화 백업 사본 보관',exact:true})).toBeEnabled();await expect(panel.getByRole('button',{name:'사본 복원 확인',exact:true})).toBeEnabled();
 await panel.getByRole('button',{name:'사본 복원 확인',exact:true}).click();await expect(panel.getByRole('heading',{name:'사본 복원 확인 완료'})).toBeVisible();await expect(panel).toContainText('전체 설치 복원이나 서비스 시작이 완료된 것은 아닙니다.');
 const copy=panel.getByRole('button',{name:'암호화 백업 사본 보관'});await copy.click();await expect(panel).not.toContainText('백업 사본을 보관했어요. 위치:');await expect(copy).toBeEnabled();await copy.click();await expect(panel).toContainText('위치: /synthetic/ExhibitOS-backup-copy');
 const calls=await page.evaluate(()=>(window as unknown as {__hostedRecoveryCalls:{action:string;expectedSource?:unknown}[]}).__hostedRecoveryCalls);expect(calls.filter(c=>c.action==='create')).toEqual([{action:'create',sourceCidr:'192.0.2.4/32',confirmExternalWritersStopped:true,expectedSource:{workspaceId:'tea-b',serviceId:'srv-b',postgresId:'dpg-b'}}]);
 expect(calls.filter(c=>c.action==='check-restore')).toEqual([{action:'check-restore',jobId:'00000000-0000-4000-8000-000000000001'}]);
 expect(calls.filter(c=>c.action==='export-archive')).toHaveLength(2);
 await expect(panel).toContainText('암호화 백업 사본은 보관했지만 전시 파일 별도 전달은 완료하지 못했어요.');await expect(copy).toBeEnabled();
 await copy.click();await expect(panel).toContainText('전시 파일도 별도 폴더에 보관했어요. 위치: /synthetic/ExhibitOS-publication');await expect(panel).toContainText('이 폴더 안의 ‘publication’');await expect(panel).toContainText('DB 복원이나 전체 서비스 복원은 아닙니다.');
 await panel.getByRole('button',{name:'백업 기록 다시 확인',exact:true}).click();await expect(panel).toContainText('전시 파일도 별도 폴더에 보관했어요. 위치: /synthetic/ExhibitOS-publication');
 await copy.click();await expect(panel.getByRole('status',{name:'별도 전시 파일 전달 결과',exact:true})).toHaveCount(0);await expect(panel).not.toContainText('백업 사본을 보관했어요. 위치:');await expect(copy).toBeEnabled();
 // A delayed chooser result belongs to its original space. Changing spaces
 // must discard that result but still release the completed request's busy UI.
 await copy.click();await expect(prepare).toBeDisabled();
 const spaces=page.getByRole('region',{name:'관리 공간 선택'});await spaces.getByRole('checkbox').check();await spaces.getByRole('button',{name:'새 복원 공간 만들고 선택'}).click();
 await expect(panel).not.toContainText('위치: /synthetic/ExhibitOS-backup-copy');
 await page.evaluate(()=>(window as unknown as {__finishHostedArchive:()=>void}).__finishHostedArchive());
 await expect(prepare).toBeEnabled();await expect(panel).not.toContainText('위치: /synthetic/ExhibitOS-backup-copy');await expect(panel.getByRole('heading',{name:'암호화 사본 생성·검증 완료'})).toHaveCount(0);
 expect(errors).toEqual([]);expect(await page.evaluate(()=>Object.keys(localStorage).concat(Object.keys(sessionStorage)))).toEqual([]);
});
