// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from 'vitest';
import { makeAgent } from '@/test/fixtures';
import { orderRows } from './board';

describe('orderRows', () => {
  it('gives one row per agent, oldest first', () => {
    const rows = orderRows([
      makeAgent({ id: 'late', createdAt: '2026-11-22T21:00:00.000Z' }),
      makeAgent({ id: 'early', createdAt: '2026-11-22T19:00:00.000Z' }),
    ]);
    expect(rows.map((agent) => agent.id)).toEqual(['early', 'late']);
  });

  it('does not reorder the list it was given', () => {
    const agents = [makeAgent({ id: 'b', createdAt: '2026-11-22T21:00:00.000Z' }), makeAgent({ id: 'a', createdAt: '2026-11-22T19:00:00.000Z' })];
    orderRows(agents);
    expect(agents.map((agent) => agent.id)).toEqual(['b', 'a']);
  });
});
