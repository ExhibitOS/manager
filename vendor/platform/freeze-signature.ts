// SPDX-License-Identifier: AGPL-3.0-or-later
// Public PR89 function and exact smallest runtime dependency excerpts; see provenance.
import {createHash,createPublicKey,verify,type KeyObject} from "node:crypto";
import type {FreezeManifest} from "@exhibitos/studio-contract";
interface FreezeRow {manifest:FreezeManifest;manifest_sha256:string;signature:string}
export class ApiError extends Error { readonly status:number; readonly code:string; constructor(status:number,code:string) {super(code);this.status=status;this.code=code;} }
export const sha256 = (bytes: Uint8Array) =>
  createHash("sha256").update(bytes).digest("hex");
const hash=(v:unknown)=>sha256(Buffer.from(freezeCanonical(v)));
export function freezeCanonical(value: unknown): string {
 if (Array.isArray(value)) return '[' + value.map(freezeCanonical).join(',') + ']';
 if (value !== null && typeof value === 'object') return '{' + Object.keys(value).sort().map(key => JSON.stringify(key) + ':' + freezeCanonical((value as Record<string, unknown>)[key])).join(',') + '}';
 if (value === undefined || typeof value === 'number' && !Number.isFinite(value)) throw Error('FREEZE_INVALID');
 return JSON.stringify(value);
}
export function verifyFreezeSignature(row:Pick<FreezeRow,'manifest'|'manifest_sha256'|'signature'>,authority:FreezeManifest['authority'],signingKey:KeyObject){
 if(row.manifest.authority.keyId!==authority.keyId||row.manifest.authority.origin!==authority.origin||row.manifest.authority.publicKey!==authority.publicKey)throw new ApiError(403,'FREEZE_AUTHORITY_CHANGED');
 if(hash(row.manifest)!==row.manifest_sha256||!verify(null,Buffer.from(freezeCanonical(row.manifest)),createPublicKey(signingKey),Buffer.from(row.signature,'base64')))throw new ApiError(409,'FREEZE_INTEGRITY');
}
