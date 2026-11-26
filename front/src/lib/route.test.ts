// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from 'vitest';
import { BOARD_HASH, agentHash, parseRoute } from './route';

describe('parseRoute', () => {
  it.each(['', '#', '#/', '#/agents', '#/agents/', '#/other/x', '#/agents/a/b', '#/animations'])('shows the board for %j', (hash) => {
    expect(parseRoute(hash)).toEqual({ view: 'board' });
  });

  it('links back to the board', () => {
    expect(parseRoute(BOARD_HASH)).toEqual({ view: 'board' });
  });

  it('opens the agent named in the hash', () => {
    expect(parseRoute('#/agents/abc-123')).toEqual({ view: 'agent', agentId: 'abc-123' });
  });

  it('round-trips ids that need encoding', () => {
    expect(parseRoute(agentHash('a b/c'))).toEqual({ view: 'agent', agentId: 'a b/c' });
  });
});
