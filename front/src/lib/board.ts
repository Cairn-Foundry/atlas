// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { AgentView } from '@/lib/contract';

/** The board has one row per agent, oldest first. */
export function orderRows(agents: readonly AgentView[]): AgentView[] {
  return agents.toSorted((a, b) => a.createdAt.localeCompare(b.createdAt));
}
