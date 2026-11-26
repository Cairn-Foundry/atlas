// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { AgentStatus } from '@/lib/contract';

/** Token classes per status, kept together so every view colours a status the same way. */
export const STATUS_TONE: Record<AgentStatus, { icon: string; text: string; border: string; dot: string }> = {
  ['working']: { icon: 'text-working', text: 'text-muted-foreground', border: 'border-working/30', dot: 'bg-working' },
  ['waiting']: { icon: 'text-waiting', text: 'text-waiting-text', border: 'border-waiting-border', dot: 'bg-waiting' },
  ['blocked']: { icon: 'text-blocked', text: 'text-blocked-text', border: 'border-blocked-border', dot: 'bg-blocked' },
  ['done']: { icon: 'text-done', text: 'text-foreground', border: 'border-foreground/30', dot: 'bg-done' },
};
