// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { AgentStatus, AgentView } from '@/lib/contract';
import type { Messages } from './messages/fr';
import { formatElapsed } from './time';

export interface StatusLook {
  /** Short note shown on the card, null when the status needs no words. */
  note: string | null;
  /** Lower-case wording for sentences and aria-labels. */
  phrase: string;
}

export function statusLook(status: AgentStatus, messages: Messages): StatusLook {
  return messages.status[status];
}

/** The instant the elapsed time counts from: the wait when waiting, the step otherwise. */
export function elapsedSince(agent: AgentView): string | null {
  if (agent.status === 'done') {
    return null;
  }
  if (agent.status === 'waiting' && agent.waitingSince !== null) {
    return agent.waitingSince;
  }
  return agent.stepStartedAt;
}

/** Time elapsed on the current wait or step, null once the agent is done. */
export function elapsedLabel(agent: AgentView, now: number): string | null {
  const since = elapsedSince(agent);
  return since === null ? null : formatElapsed(since, now);
}
