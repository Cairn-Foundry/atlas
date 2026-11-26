// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { afterEach, describe, expect, it, vi } from 'vitest';
import { daemonFetch, websocketUrl } from '@/lib/daemon-connection';
import { TEST_DAEMON_TOKEN, TEST_DAEMON_URL } from '@/test/setup';

describe('daemon connection', () => {
  afterEach(() => vi.unstubAllGlobals());

  it('calls the daemon at its address with the token and the headers of the caller', async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(null, { status: 204 }));
    vi.stubGlobal('fetch', fetchMock);
    await daemonFetch('/api/notes', { method: 'POST', headers: { 'content-type': 'application/json' }, body: '{}' });
    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    const headers = new Headers(init.headers);
    expect(url).toBe(`${TEST_DAEMON_URL}/api/notes`);
    expect(init.method).toBe('POST');
    expect(headers.get('authorization')).toBe(`Bearer ${TEST_DAEMON_TOKEN}`);
    expect(headers.get('content-type')).toBe('application/json');
  });

  it('gives the token in the address of a socket', () => {
    expect(websocketUrl('/ws/terminal/a1')).toBe(`ws://daemon.test/ws/terminal/a1?token=${TEST_DAEMON_TOKEN}`);
  });
});
