// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { AgentStatus, AgentView } from '@/lib/contract';
import type { Messages } from './messages/fr';

export type FilterKey = 'all' | AgentStatus;

export interface FilterOption {
  key: FilterKey;
  label: string;
  count: number;
}

const FILTER_KEYS: readonly FilterKey[] = ['all', 'working', 'waiting', 'blocked', 'done'];

export function matchesFilter(agent: AgentView, filter: FilterKey): boolean {
  return filter === 'all' || agent.status === filter;
}

/** Text announced to screen readers when the filter changes. */
export function visibleCountLabel(agents: readonly AgentView[], filter: FilterKey, messages: Messages): string {
  return messages.filters.agentsShown(agents.filter((agent) => matchesFilter(agent, filter)).length);
}

export function buildFilterOptions(agents: readonly AgentView[], messages: Messages): FilterOption[] {
  return FILTER_KEYS.map((key) => ({
    key,
    label: messages.filters[key],
    count: agents.filter((agent) => matchesFilter(agent, key)).length,
  }));
}
