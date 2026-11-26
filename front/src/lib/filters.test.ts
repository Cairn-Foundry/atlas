// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from 'vitest';
import { makeAgent } from '@/test/fixtures';
import { en } from './messages/en';
import { fr } from './messages/fr';
import { buildFilterOptions, matchesFilter, visibleCountLabel } from './filters';

const agents = [
  makeAgent({ id: 'a', status: 'working' }),
  makeAgent({ id: 'b', status: 'working' }),
  makeAgent({ id: 'c', status: 'waiting' }),
  makeAgent({ id: 'd', status: 'blocked' }),
];

describe('buildFilterOptions', () => {
  it('lists every filter in display order with its count', () => {
    expect(buildFilterOptions(agents, fr)).toEqual([
      { key: 'all', label: 'Tous', count: 4 },
      { key: 'working', label: 'Au travail', count: 2 },
      { key: 'waiting', label: 'Vous attendent', count: 1 },
      { key: 'blocked', label: 'Bloqués', count: 1 },
      { key: 'done', label: 'Terminés', count: 0 },
    ]);
  });

  it('labels the filters in English', () => {
    expect(buildFilterOptions(agents, en).map((option) => option.label)).toEqual(['All', 'Working', 'Waiting for you', 'Blocked', 'Done']);
  });

  it('shows zero everywhere when there is no agent', () => {
    expect(buildFilterOptions([], fr).every((option) => option.count === 0)).toBe(true);
  });
});

describe('matchesFilter', () => {
  it('matches everything on "all"', () => {
    expect(agents.every((agent) => matchesFilter(agent, 'all'))).toBe(true);
  });

  it('matches only the selected status', () => {
    expect(agents.filter((agent) => matchesFilter(agent, 'waiting')).map((agent) => agent.id)).toEqual(['c']);
  });
});

describe('visibleCountLabel', () => {
  it('announces how many agents the filter leaves visible', () => {
    expect(visibleCountLabel(agents, 'all', fr)).toBe('4 agents affichés');
    expect(visibleCountLabel(agents, 'waiting', fr)).toBe('1 agent affiché');
    expect(visibleCountLabel(agents, 'done', fr)).toBe('0 agents affichés');
  });

  it('announces the count in English, with its plural', () => {
    expect(visibleCountLabel(agents, 'all', en)).toBe('4 agents shown');
    expect(visibleCountLabel(agents, 'waiting', en)).toBe('1 agent shown');
  });
});
