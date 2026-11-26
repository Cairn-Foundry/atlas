// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { invoke } from '@tauri-apps/api/core';
import type { Language } from '@/lib/contract';

/** Where the daemon of this machine listens, and the secret every call to it must carry. */
export interface DaemonConnection {
  url: string;
  token: string;
}

let connection: DaemonConnection | null = null;

export function setDaemonConnection(next: DaemonConnection): void {
  connection = next;
}

/** Asks the desktop shell for the daemon, which it starts when none runs; rejects with the reason it could not. */
export async function connectToDaemon(): Promise<void> {
  setDaemonConnection(await invoke<DaemonConnection>('connect_to_daemon'));
}

function currentConnection(): DaemonConnection {
  if (connection === null) {
    throw new Error('The daemon was called before the connection was made');
  }
  return connection;
}

/** Tells the desktop shell the language the page shows, for its menu and its errors; no shell (tests, browser) is no loss. */
export function tellShellLanguage(language: Language): void {
  invoke('set_language', { language }).catch(() => undefined);
}

/** `fetch` on a path of the daemon, with the token it asks for. */
export function daemonFetch(path: string, init: RequestInit = {}): Promise<Response> {
  const { url, token } = currentConnection();
  const headers = new Headers(init.headers);
  headers.set('authorization', `Bearer ${token}`);
  return fetch(`${url}${path}`, { ...init, headers });
}

/** Address of a WebSocket of the daemon; a page cannot set a header on one, so the token rides in the address. */
export function websocketUrl(path: string): string {
  const { url, token } = currentConnection();
  return `${url.replace(/^http/, 'ws')}${path}?token=${encodeURIComponent(token)}`;
}
