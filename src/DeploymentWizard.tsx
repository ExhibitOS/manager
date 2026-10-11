// SPDX-License-Identifier: Apache-2.0
import {useState} from 'react';
import {cloudConfiguration,cloudSummary,deploymentDescriptor,descriptorText,type CredentialReference,type CloudInput} from './deployment-wizard';
function download(name:string,text:string){const url=URL.createObjectURL(new Blob([text],{type:'application/json'}));const a=document.createElement('a');a.href=url;a.download=name;a.click();setTimeout(()=>URL.revokeObjectURL(url),1000);}
export function DeploymentWizard(){
 const [mode,setMode]=useState<'local'|'server'|'cloud'>('local'),[level,setLevel]=useState('beginner');
 const [image,setImage]=useState(''),[host,setHost]=useState(''),[port,setPort]=useState('22'),[user,setUser]=useState('');
 const [credential,setCredential]=useState<'environment'|'os-keychain'>('environment');
 const [dbVariable,setDbVariable]=useState('EXHIBITOS_DATABASE_PASSWORD'),[sshVariable,setSshVariable]=useState('EXHIBITOS_SSH_KEY');
 const [service,setService]=useState('exhibitos.deployment'),[dbAccount,setDbAccount]=useState('database-password'),[sshAccount,setSshAccount]=useState('ssh-key');
 const [aws,setAws]=useState<CloudInput>({provider:'aws',account_id:'',region:'',project:'',ami_id:'',instance_type:'t3.small',subnet_id:'',security_group_ids:[],key_name:'',root_gib:20});
 const [review,setReview]=useState<{filename:string;text:string;summary:string}|null>(null),[error,setError]=useState('');
 function changed(){setReview(null);setError('');}
 const db:CredentialReference=credential==='os-keychain'?{source:credential,service,account:dbAccount}:{source:'environment',variable:dbVariable};
 const ssh:CredentialReference=credential==='os-keychain'?{source:credential,service,account:sshAccount}:{source:'environment',variable:sshVariable};
 function prepare(event:React.FormEvent){event.preventDefault();setReview(null);setError('');try{
  if(mode==='cloud'){const config=cloudConfiguration(aws),summary=cloudSummary(config);setReview({filename:'exhibitos-aws-config.json',text:descriptorText(config),summary:`${summary.changes[0]!.instanceType} · ${summary.changes[0]!.rootGiB} GiB · 월 비용 미확인 · 새 지출 허용 0 KRW.`});}
  else{const oed=deploymentDescriptor(mode==='local'?{mode,image,database:{source:'environment',variable:dbVariable}}:{mode,image,database:db,host,port:Number(port),user,ssh});setReview({filename:'exhibitos-deployment.oed.json',text:descriptorText(oed),summary:'실행 포트13200 · PostgreSQL · 작품 전용 파일 영역. 인증 정보의 참조만 포함합니다.'});}
 }catch{setError('입력을 확인하세요. 고정된 이미지 식별자와 인증 정보의 참조가 필요합니다. 비밀번호나 키 내용은 입력하지 마세요.');}}
 function field(label:string,value:string,set:(v:string)=>void){return <label>{label}<input value={value} maxLength={512} onChange={e=>{changed();set(e.target.value);}} autoComplete="off"/></label>;}
 function awsField(key:'account_id'|'region'|'project'|'ami_id'|'subnet_id'|'key_name',label:string){return field(label,aws[key],v=>setAws({...aws,[key]:v}));}
 return <section aria-labelledby="deployment-wizard-title"><h2 id="deployment-wizard-title">배포 준비</h2><p>배포 장소를 선택하고 실행 전 확인할 파일을 만듭니다. 이 화면은 서버·DNS·데이터를 변경하거나 인증 정보를 읽지 않습니다.</p>
 <form onSubmit={prepare} onReset={()=>{changed();}}>
 <label>안내 수준<select value={level} onChange={e=>setLevel(e.target.value)}><option value="beginner">처음 사용</option><option value="intermediate">서버 운영 경험 있음</option><option value="advanced">직접 배포 설정</option></select></label>
 <label>배포 장소<select value={mode} onChange={e=>{changed();setMode(e.target.value as typeof mode);}}><option value="local">내 컴퓨터</option><option value="server">보유한 Linux 서버</option><option value="cloud">클라우드 서버 준비 (AWS)</option></select></label>
 {level==='beginner'&&<p>내 컴퓨터에서 먼저 전시를 확인하세요. 외부 서버를 사용하려면 서버 운영자와 인증 정보 보관 위치를 준비하세요.</p>}
 {level==='intermediate'&&<p>서버의 접근 권한과 공개 주소, 독립적으로 복원한 백업을 확인한 뒤 배포를 검토하세요.</p>}
 {level==='advanced'&&<p>OED 1.0.0-draft.1의 현재 지원 범위: 단일 Compose 전시, PostgreSQL, filesystem. 실행기에서 패키지·권한·readiness를 다시 검증합니다.</p>}
 {mode!=='cloud'&&<>{field('검증한 실행 이미지 (저장소@sha256:식별자)',image,setImage)}
 {mode==='server'&&<>{field('서버 주소',host,setHost)}{field('SSH 포트',port,setPort)}{field('서버 사용자 (root 제외)',user,setUser)}<label>인증 정보 보관 위치<select value={credential} onChange={e=>{changed();setCredential(e.target.value as typeof credential);}}><option value="environment">환경변수 참조</option><option value="os-keychain">OS keychain 참조</option></select></label></>}
 {mode==='local'||credential==='environment'?<>{field('DB 비밀번호 환경변수 이름',dbVariable,setDbVariable)}{mode==='server'&&field('SSH 키 환경변수 이름',sshVariable,setSshVariable)}</>:<>{field('Keychain 서비스 이름',service,setService)}{field('DB 계정 참조 이름',dbAccount,setDbAccount)}{field('SSH 키 계정 참조 이름',sshAccount,setSshAccount)}<p>현재 Generic SSH keychain 해석은 macOS에서 제공됩니다. 다른 환경에서는 환경변수 참조를 사용하세요.</p></>}
 </>}
 {mode==='cloud'&&<>{awsField('account_id','AWS 계정 번호 (12자리)')}{awsField('region','리전')}{awsField('project','격리된 프로젝트 이름')}{awsField('ami_id','확인한 AMI ID')}{awsField('subnet_id','기존 서브넷 ID')}{field('기존 보안 그룹 ID (쉼표로 구분)',aws.security_group_ids.join(','),v=>setAws({...aws,security_group_ids:v.split(',').map(x=>x.trim())}))}{awsField('key_name','기존 SSH 공개 키 등록 이름')}<label>서버 크기<select value={aws.instance_type} onChange={e=>{changed();setAws({...aws,instance_type:e.target.value as CloudInput['instance_type']});}}><option>t3.small</option><option>t3.medium</option></select></label><label>디스크 용량 (20–64 GiB)<input type="number" min={20} max={64} value={aws.root_gib} onChange={e=>{changed();setAws({...aws,root_gib:Number(e.target.value)});}}/></label><p>새 지출 허용은 0 KRW입니다. 비용 추정은 지출 상한을 보장하지 않습니다. 실제 apply는 기존 adapter에서 비용 통제가 검증되지 않으면 거부됩니다. AWS 임시 인증은 실행 환경에서 제공하며 이 파일에 저장하지 않습니다.</p></>}
 <button type="submit">입력 확인·파일 준비</button></form>
 {error&&<p role="alert">{error}</p>}
 {review&&<div role="status"><strong>준비 파일 생성됨 · 배포 미실행</strong><p>{review.summary}</p><button onClick={()=>download(review.filename,review.text)}>준비 파일 저장</button><details><summary>파일 내용 확인 (비밀값 없음)</summary><pre>{review.text}</pre></details></div>}
 <ol><li>{mode==='local'?'만든 OED는 준비 문서입니다. 아래 기존 전시 설치·시작에서 검증된 패키지를 사용하세요. 이 파일을 로컬 설치로 가져오는 연결은 아직 제공하지 않습니다.':mode==='server'?'Generic SSH 실행기에서 OED와 확인한 known_hosts를 사용해 plan을 검토하세요. 고정된 서버 키 확인 없이 연결하지 않습니다.': '기존 provider dry-run에서 저장한 JSON을 확인하세요. 실행 파일·모듈·계정 비용 통제 검증 뒤의 실제 plan/apply는 별도 절차입니다.'}</li><li>원격 설치 후 실제 readiness와 로그를 확인하세요. 공개 도메인·DNS·TLS·방화벽은 운영자가 별도로 확인해야 합니다.</li><li>업데이트 전에 원본 전체 백업의 독립 복원을 확인하세요. 실패하면 저장한 작업·데이터를 보존하고 기존 rollback 절차를 사용하세요. 네트워크 오류를 성공으로 처리하지 않습니다.</li></ol>
 {mode==='cloud'&&<button onClick={()=>{changed();setMode('server');}}>보유 서버로 직접 배포 준비</button>}
 <p>원격 실행·상태·백업·업데이트·rollback 작업의 앱 연결은 아직 제공하지 않습니다. 준비 파일 생성은 배포 성공이나 복구 완료를 의미하지 않습니다.</p>
 </section>;
}
