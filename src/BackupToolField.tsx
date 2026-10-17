// SPDX-License-Identifier: Apache-2.0
import {useEffect,useRef} from 'react';
export interface BackupToolContext{image:string|null;message:string|null;retry?():void}
export function BackupToolField({id,label,value,onChange,context}:{id:string;label:string;value:string;onChange(value:string):void;context:BackupToolContext}){
 const applied=useRef<string|null>(null);
 useEffect(()=>{if(context.image&&applied.current!==context.image){applied.current=context.image;if(!value)onChange(context.image);}},[context.image,value,onChange]);
 return <div><p className="note">{value?(value===context.image?'이 Manager에 포함된 복구 도구를 선택했습니다. 실행할 때 이미지와 백업의 호환성을 다시 확인합니다.':'직접 지정한 복구 도구를 사용합니다. 실행할 때 백업과의 호환성을 확인합니다.'):context.image?'복구 도구를 선택하세요. 포함된 도구를 사용하거나 고급 설정에서 직접 지정할 수 있습니다.':context.message??'포함된 복구 도구 정보를 확인하고 있어요.'}</p>
 {context.image&&value!==context.image&&<button type="button" onClick={()=>{if(context.image)onChange(context.image);}}>포함된 복구 도구 사용</button>}
 {context.message&&context.retry&&<button type="button" onClick={context.retry}>복구 도구 다시 확인</button>}
 <details><summary>복구 도구 고급 설정</summary><label htmlFor={id}>{label}</label><input id={id} value={value} onChange={event=>onChange(event.target.value)} maxLength={71} autoComplete="off" spellCheck={false}/><p className="note">기존에 검증한 sha256: 로컬 이미지 ID가 있을 때만 직접 지정하세요. Runtime 이미지가 복구용 이미지와 같다고 추측하지 않습니다.</p></details></div>;
}
