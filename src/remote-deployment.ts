// SPDX-License-Identifier: Apache-2.0
import {invoke,isTauri} from '@tauri-apps/api/core';
import type {deploymentDescriptor} from './deployment-wizard';
export type RemoteAction='plan'|'status'|'logs'|'verify'|'install'|'start'|'stop'|'restart'|'retry'|'backup'|'backup-jobs'|'update'|'update-status'|'rollback';
export type RemoteConnection={remoteRoot:string;managerPath:string;managerSha256:string;knownHostsPath:string;knownHostsSha256:string;httpsOrigin:string};
export type RemoteBackupInput={maintenanceImage:string;keyPath:string};
export type RemoteAuthorityInput={profilePath:string;updatePath:string;updateSha256:string;operationId:string;expectedGeneration:number};
export type RemoteUpdateInput=RemoteAuthorityInput&{artifactPath:string;stagingParent:string;pythonPath:string;sourceCommit:string;maintenanceImage:string;exportParent:string;hostArchivePath:string;trustArchivePath:string;keyPath:string;pairBindingPath:string;hostExtractionPath:string};
export type RemoteRollbackInput=RemoteAuthorityInput&{maintenanceImage:string;keyPath:string;archivePath:string;port:number};
export type RemoteMaintenance=RemoteBackupInput|RemoteAuthorityInput|RemoteUpdateInput|RemoteRollbackInput|null;
export type RemoteRequest={requestId:string;action:RemoteAction;oed:ReturnType<typeof deploymentDescriptor>;connection:RemoteConnection;consent:{preserveVolumes:boolean;existingImagesOnly:boolean;externalWritersQuiesced:boolean;appsClosed:boolean};maintenance:RemoteMaintenance};
export type RemoteReceipt={version:1;action:RemoteAction;state?:string;ready?:boolean;prepared?:boolean;healthVerified?:boolean;selectionVerified?:boolean;restorationVerified?:boolean;activationVerified?:boolean;dnsVerified?:boolean;tlsVerified?:boolean;authorityGeneration?:number;jobId?:string;backupId?:string;operationId?:string;candidateId?:string|null;code?:string|null;entries?:{at:number;code:string}[];jobs?:{id:string;state:string;stage:string;code:string|null;createdAt:number;updatedAt:number}[]};
export async function remoteAvailable(){if(!isTauri())return false;return await invoke('manager_remote_available')===true;}
export async function cancelRemote(requestId:string){if(!isTauri())return false;return await invoke('manager_remote_cancel',{input:{requestId}})===true;}
export async function remoteOperation(input:RemoteRequest):Promise<RemoteReceipt>{
 if(!isTauri())throw {code:'REMOTE_DESKTOP_REQUIRED',guidance:'데스크톱 앱에서 서버 작업을 실행하세요.'};
 const value=await invoke('manager_remote',{input});
 if(!value||typeof value!=='object'||Array.isArray(value)||(value as Record<string,unknown>).version!==1||(value as Record<string,unknown>).action!==input.action)throw Error('MANAGER_PROTOCOL');return value as RemoteReceipt;
}
