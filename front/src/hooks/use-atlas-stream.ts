// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useEffect, useReducer } from 'react';
import { connectToDaemon, websocketUrl } from '@/lib/daemon-connection';
import { showLanguage } from '@/lib/i18n';
import { parseServerEvent } from '@/lib/socket-events';
import { initialStreamState, streamReducer, type StreamState } from '@/lib/stream-state';

const MAX_BACKOFF_MS = 10_000;
/** Failed attempts after which the shell is asked for the daemon again: it starts one when none runs. */
const ATTEMPTS_BEFORE_RESTART = 3;

/** Keeps the factory state in sync with the daemon over `/ws`, reconnecting with backoff. */
export function useAtlasStream(): StreamState {
  const [state, dispatch] = useReducer(streamReducer, initialStreamState);

  useEffect(() => {
    let socket: WebSocket | null = null;
    let retryTimer: ReturnType<typeof setTimeout> | undefined;
    let attempt = 0;
    let isDisposed = false;

    const connect = (): void => {
      socket = new WebSocket(websocketUrl('/ws'));
      socket.addEventListener('open', () => {
        dispatch({ type: 'connection', isConnected: true });
      });
      socket.addEventListener('message', (event: MessageEvent<string>) => {
        const serverEvent = parseServerEvent(event.data);
        if (serverEvent === null) return;
        // Only a daemon that sent its state is back: one that accepts then hangs up keeps the backoff.
        if (serverEvent.type === 'snapshot') {
          attempt = 0;
          // The daemon owns the language of the factory: the page follows it.
          showLanguage(serverEvent.snapshot.language);
        }
        dispatch(serverEvent);
      });
      socket.addEventListener('close', () => {
        dispatch({ type: 'connection', isConnected: false });
        if (isDisposed) {
          return;
        }
        const delay = Math.min(MAX_BACKOFF_MS, 500 * 2 ** attempt);
        attempt += 1;
        retryTimer = setTimeout(attempt >= ATTEMPTS_BEFORE_RESTART ? reconnectThroughShell : connect, delay);
      });
    };

    /** A daemon that died while the window was open is never started again by waiting for it. */
    const reconnectThroughShell = (): void => {
      void connectToDaemon()
        .catch(() => undefined)
        .then(() => {
          if (!isDisposed) connect();
        });
    };

    connect();
    return () => {
      isDisposed = true;
      clearTimeout(retryTimer);
      socket?.close();
    };
  }, []);

  return state;
}
