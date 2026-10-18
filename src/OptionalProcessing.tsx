// SPDX-License-Identifier: Apache-2.0
import {useEffect,useRef,useState} from 'react';
import {processingAppClient,type ProcessingAppAction,type ProcessingAppContext} from './processing-app';
import {managerError} from './lifecycle';
export function OptionalProcessing(){
 const [context,setContext]=useState<ProcessingAppContext|null>(null),[pending,setPending]=useState(false),[error,setError]=useState(''),[notice,setNotice]=useState('');
 const live=useRef(true),working=useRef(false),generation=useRef(0),opened=useRef(false);
 async function act(action:ProcessingAppAction){if(!processingAppClient.native()||working.current)return;working.current=true;const request=++generation.current;setPending(true);setError('');setNotice('');try{const result=await processingAppClient.operation(action);if(live.current&&request===generation.current){setContext(result);if(action.startsWith('open-'))setNotice('앱 열기를 요청했습니다. 모델 처리는 앱에서 직접 시작하세요.');}}catch(e){if(live.current&&request===generation.current)setError(managerError(e).guidance);}finally{working.current=false;if(live.current)setPending(false);}}
 useEffect(()=>{live.current=true;return()=>{live.current=false;generation.current++;};},[]);
 const available=processingAppClient.native()&&context?.supported!==false;
 return <section className="panel optional-processing" aria-labelledby="optional-processing-heading"><p className="eyebrow">OPTIONAL TOOLS</p><h2 id="optional-processing-heading">촬영 파일을 모델로 만들기</h2><p>Studio는 처리 앱 없이 기존 지원 작품 파일을 가져와 사용할 수 있어요.</p><details onToggle={event=>{if(event.currentTarget.open&&!opened.current){opened.current=true;void act('status');}}}><summary>선택적 처리 앱 연결</summary><p>Mac Processing이나 직접 선택한 처리 앱을 열 수 있어요.</p>
 {!processingAppClient.native()?<p className="note">앱 연결은 Mac Manager에서 사용할 수 있어요.</p>:context?.supported===false?<p className="note">현재 환경에서 앱 연결을 지원하지 않습니다. 파일로 직접 전달할 수 있어요.</p>:null}
 {context?.detected&&<div className="job-summary"><strong>{context.detected.name}</strong><p>{context.detected.version?`버전 ${context.detected.version} · `:''}등록된 앱을 찾았어요. 정식 스토어 설치·서명 여부는 확인하지 않았어요.</p><button type="button" disabled={pending} onClick={()=>void act('open-detected')}>ExhibitOS Processing 열기</button></div>}
 {context&&context.detectedCount===0&&<p className="note">등록된 ExhibitOS Processing 앱을 찾지 못했어요. 설치한 앱이나 직접 빌드한 앱을 선택할 수 있어요.</p>}
 {context&&context.detectedCount>1&&<p className="note">여러 Processing 앱을 찾았어요. 사용할 앱을 직접 선택해 주세요.</p>}
 {context?.selected&&<div className="job-summary"><strong>선택한 도구: {context.selected.name}</strong><p>{context.selected.version?`버전 ${context.selected.version}`:'버전 정보 없음'}</p><button type="button" disabled={pending} onClick={()=>void act('open-selected')}>선택한 처리 앱 열기</button><details><summary>선택한 앱 위치</summary><p className="code">{context.selected.path}</p></details></div>}
 {context?.selectionUnavailable&&<p className="note" role="status">이전에 선택한 앱을 확인하지 못했어요. 파일은 바꾸지 않았으며 다른 앱을 다시 선택할 수 있어요.</p>}
 <div className="panel-actions"><button type="button" disabled={!available||pending} onClick={()=>void act('choose')}>사용할 처리 앱 선택</button><button type="button" disabled={!available||pending} onClick={()=>void act('status')}>처리 앱 다시 확인</button></div>
 {pending&&<p role="status">앱을 확인하고 있어요. 선택 중이라면 파일 선택 창을 확인하세요.</p>}{error&&<p className="alert" role="alert">{error}</p>}{notice&&<p role="status">{notice}</p>}
 <p className="note">공식 Mac App Store 제품 페이지는 준비 중이에요. 이 화면은 앱을 설치하거나 업데이트하지 않아요. 앱의 결과물은 기존 파일 계약과 품질 기준으로 별도 확인해요.</p></details></section>;
}
