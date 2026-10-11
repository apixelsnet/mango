// Which Mango this is and whether a newer one is out. Asked once after the first
// paint; Mango answers from what it already knows, without going to the release list.

import { get, send, type VersionInfo } from './api';

export const version = $state<{ info: VersionInfo | null }>({ info: null });

export async function loadVersion() {
  version.info = await get<VersionInfo>('/version');
}

/** Has Mango ask the release list now. */
export async function checkVersion() {
  version.info = await send<VersionInfo>('POST', '/version/check');
}
