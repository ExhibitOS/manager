// SPDX-License-Identifier: Apache-2.0
import type {EngineProbe,Status} from './lifecycle';
/** Guidance only: native lifecycle still validates and preserves the engine pin. */
export function selectedEngineReady(status:Status|null,engines:EngineProbe[]):boolean {
 return status?.requiredEngine ? engines.some(engine=>engine.kind===status.requiredEngine&&engine.available) : engines.some(engine=>engine.available);
}
