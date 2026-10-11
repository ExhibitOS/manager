// SPDX-License-Identifier: Apache-2.0
import {useEffect,useRef,useState} from 'react';
import {cancelRemote,remoteAvailable,remoteOperation,type RemoteAction,type RemoteConnection,type RemoteMaintenance,type RemoteReceipt} from './remote-deployment';
import {managerError} from './lifecycle';
import type {deploymentDescriptor} from './deployment-wizard';
const labels:Record<RemoteAction,string>={plan:'서버 준비 확인',status:'실제 상태 확인',logs:'저장된 작업 로그',verify:'공개 주소 연결 확인',install:'서버 전시 설치',start:'서버 시작',stop:'서버 정지',restart:'서버 재시작',retry:'실패 작업 재시도',backup:'인증된 전체 백업', 'backup-jobs':'백업 작업 상태',update:'검증된 전체 업데이트','update-status':'업데이트 실제 상태',rollback:'원래 백업으로 전체 복원'};
const authority=['profilePath','updatePath','updateSha256','operationId','expectedGeneration'];
const fields:Partial<Record<RemoteAction,string[]>>={backup:['maintenanceImage','keyPath'],'update-status':authority,update:[...authority,'artifactPath','stagingParent','pythonPath','sourceCommit','maintenanceImage','exportParent','hostArchivePath','trustArchivePath','keyPath','pairBindingPath','hostExtractionPath'],rollback:[...authority,'maintenanceImage','keyPath','archivePath','port']};
const names:Record<string,string>={profilePath:'서버 관리 공간 경로',updatePath:'서버 업데이트 실행파일 경로',updateSha256:'업데이트 실행파일 SHA256',operationId:'현재 업데이트 작업 ID',expectedGeneration:'현재 권한 세대',maintenanceImage:'검증된 유지보수 이미지 digest',keyPath:'서버 외부 키 파일 위치',artifactPath:'서명된 OCI 파일 위치',stagingParent:'서버 임시 작업 부모 폴더',pythonPath:'서버 Python 실행파일 위치',sourceCommit:'검증된 Manager 소스 커밋',exportParent:'서버 내보내기 부모 폴더',hostArchivePath:'전체 관리 공간 백업 위치',trustArchivePath:'권한 백업 위치',pairBindingPath:'결합 증명 위치',hostExtractionPath:'관리 공간 독립 복원 위치',archivePath:'원래 인증된 백업 위치',port:'새 복원 후보 포트'};
export function RemoteDeployment({oed}:{oed:ReturnType<typeof deploymentDescriptor>}){
 const [available,setAvailable]=useState(false),[connection,setConnection]=useState<RemoteConnection>({remoteRoot:'',managerPath:'',managerSha256:'',knownHostsPath:'',knownHostsSha256:'',httpsOrigin:''});
 const [preserve,setPreserve]=useState(false),[existing,setExisting]=useState(false),[writers,setWriters]=useState(false),[closed,setClosed]=useState(false),[pending,setPending]=useState<RemoteAction|null>(null),[receipt,setReceipt]=useState<RemoteReceipt|null>(null),[failure,setFailure]=useState(''),[selected,setSelected]=useState<RemoteAction>('backup'),[values,setValues]=useState<Record<string,string>>({}),[cancelling,setCancelling]=useState(false);
 const owner=useRef<string|null>(null),live=useRef(true);
 useEffect(()=>{live.current=true;remoteAvailable().then(value=>{if(live.current)setAvailable(value);}).catch(()=>{});return()=>{live.current=false;const id=owner.current;if(id)void cancelRemote(id);};},[]);
 const configured=Object.values(connection).every(value=>value.length>0),enabled=available&&configured&&preserve&&existing&&pending===null;
 function invalidate(){setReceipt(null);setFailure('');setPreserve(false);setExisting(false);setWriters(false);setClosed(false);}
 function field(key:keyof RemoteConnection,label:string){return <label>{label}<input value={connection[key]} maxLength={4095} autoComplete="off" disabled={pending!==null} onChange={e=>{setConnection({...connection,[key]:e.target.value});invalidate();}}/></label>;}
 async function run(action:RemoteAction){if(!enabled)return;const write=['backup','update','rollback'].includes(action);if(write&&(!writers||!closed))return;const required=fields[action]??[];if(required.some(k=>!values[k])){setFailure('필수 서버 참조를 확인하세요. 키 내용·비밀번호를 입력하지 마세요.');return;}
  const maintenance=required.length?Object.fromEntries(required.map(k=>[k,['expectedGeneration','port'].includes(k)?Number(values[k]):values[k]]))as RemoteMaintenance:null;
  const id=crypto.randomUUID();owner.current=id;setPending(action);setReceipt(null);setFailure('');setCancelling(false);
  try{const result=await remoteOperation({requestId:id,action,oed,connection,consent:{preserveVolumes:preserve,existingImagesOnly:existing,externalWritersQuiesced:writers,appsClosed:closed},maintenance});if(live.current&&owner.current===id)setReceipt(result);}catch(e){if(live.current&&owner.current===id){const error=managerError(e);setFailure(`${error.guidance} 확인 코드: ${error.code}`);}}finally{if(owner.current===id){owner.current=null;if(live.current){setPending(null);setCancelling(false);}}}
 }
 async function cancel(){const id=owner.current;if(!id||cancelling)return;setCancelling(true);try{await cancelRemote(id);}catch{setFailure('취소 요청을 확인하지 못했습니다. 실제 서버 상태를 확인하세요.');setCancelling(false);}}
 return <section aria-label="보유 서버 연결·실행"><fieldset disabled={pending!==null}><legend>보유 서버 연결·실행</legend>
 <p>서버 운영자가 준비한 Linux Manager·검증된 패키지·이미지를 사용합니다. 키·비밀번호 내용은 입력하지 않으며 OED 보관 위치에서만 해석합니다. Unix SSH 클라이언트 지원이며 Windows에서는 기존 수동 배포를 사용하세요.</p>
 {field('remoteRoot','서버의 전시 관리 폴더')}{field('managerPath','서버 Manager 실행파일 경로')}{field('managerSha256','서버 Manager SHA256')}{field('knownHostsPath','이 컴퓨터의 확인한 known_hosts 파일 위치')}{field('knownHostsSha256','known_hosts SHA256')}{field('httpsOrigin','전시 공개 주소 (https://도메인/)')}
 <label><input type="checkbox" checked={preserve} onChange={e=>setPreserve(e.target.checked)}/>지정한 서버 연결·인증 참조 해석을 승인하며 기존 데이터를 보존합니다.</label>
 <label><input type="checkbox" checked={existing} onChange={e=>setExisting(e.target.checked)}/>서버에 이미 준비된 검증된 이미지·패키지만 사용합니다.</label>
 <div>{(['plan','status','logs','verify','install','start','stop','restart','retry']as RemoteAction[]).map(action=><button key={action} disabled={!enabled} onClick={()=>void run(action)}>{labels[action]}</button>)}</div>
 <label>백업·업데이트·전체 복원 작업<select value={selected} onChange={e=>{setSelected(e.target.value as RemoteAction);setValues({});invalidate();}}>{(['backup','backup-jobs','update-status','update','rollback']as RemoteAction[]).map(action=><option key={action} value={action}>{labels[action]}</option>)}</select></label>
 {(fields[selected]??[]).map(key=><label key={key}>{names[key]}<input autoComplete="off" maxLength={4095} value={values[key]??''} onChange={e=>{setValues({...values,[key]:e.target.value});invalidate();}}/></label>)}
 <p>업데이트는 서버에서 준비된 서명·권한·전체 백업을 다시 검증합니다. 호환되지 않는 변경은 거부하며 복원은 원래 전체 데이터와 이미지·구성·건강·선택을 확인합니다. 이미지 되돌림만으로 복원 성공을 표시하지 않습니다.</p>
 <label><input type="checkbox" checked={writers} onChange={e=>setWriters(e.target.checked)}/>외부 쓰기 작업을 정지했으며 전체 백업·업데이트·복원에 동의합니다.</label>
 <label><input type="checkbox" checked={closed} onChange={e=>setClosed(e.target.checked)}/>서버 관리 공간을 사용하는 앱을 닫았습니다.</label>
 <button disabled={!enabled||(['backup','update','rollback'].includes(selected)&&(!writers||!closed))} onClick={()=>void run(selected)}>{labels[selected]}</button>
 </fieldset>
 {!available&&<p>검증된 실행기가 포함된 데스크톱 앱이 필요합니다. 서버에 연결하지 않았습니다.</p>}
 {pending&&<div role="status">{labels[pending]} 진행 중 · 서버 응답을 기다립니다.<button disabled={cancelling} onClick={()=>void cancel()}>{cancelling?'중단 요청 중':'연결 작업 중단'}</button><p>중단은 원격 작업의 되돌림을 보장하지 않습니다. 실제 상태·로그를 확인하세요.</p></div>}
 {failure&&<p role="alert">{failure} 응답이 불확실하면 변경이 없었다고 가정하지 마세요.</p>}
 {receipt&&<div role="status"><strong>{labels[receipt.action]}: 서버 응답 수신</strong><p>상태: {receipt.state??(receipt.prepared?'준비 확인됨':receipt.ready?'준비됨':'응답 확인됨')}</p>{receipt.authorityGeneration!==undefined&&<p>권한 세대: {receipt.authorityGeneration}</p>}{receipt.code&&<p>확인 코드: {receipt.code}</p>}{receipt.jobId&&<p>작업 ID: {receipt.jobId}</p>}{receipt.backupId&&<p>백업 ID: {receipt.backupId}</p>}{receipt.healthVerified&&<p>실제 건강 확인 완료</p>}{receipt.restorationVerified&&<p>원래 전체 복원 확인 완료</p>}{receipt.selectionVerified&&<p>활성 선택 확인 완료</p>}{receipt.dnsVerified&&<p>공개 주소 DNS 연결 확인 완료</p>}{receipt.tlsVerified&&<p>HTTPS 연결 확인 완료</p>}{receipt.entries&&<ul>{receipt.entries.map((e,i)=><li key={i}>{e.at}: {e.code}</li>)}</ul>}{receipt.jobs&&<ul>{receipt.jobs.map(j=><li key={j.id}>{j.id}: {j.state} / {j.stage}{j.code?` / ${j.code}`:''}</li>)}</ul>}</div>}
 <p>DNS·TLS는 공개 주소 확인에서 별도로 관찰합니다. 도메인 등록·DNS·방화벽 변경은 자동 실행하지 않습니다. 실패 후 실제 상태와 보존 로그를 확인한 뒤 명시적으로 재시도하세요.</p>
 </section>;
}
