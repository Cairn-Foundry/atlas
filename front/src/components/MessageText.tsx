// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { ReactNode } from 'react';
import type { AgentView } from '@/lib/contract';
import { splitMentions } from '@/lib/mentions';
import { useMessages } from '@/hooks/use-messages';

interface MessageTextProps {
  text: string;
  agents: readonly AgentView[];
  onOpenAgent: (agentId: string) => void;
}

/** Plain text where each known `@TICKET` becomes a chip that opens the agent's session. */
export function MessageText({ text, agents, onOpenAgent }: MessageTextProps): ReactNode {
  const messages = useMessages();
  return splitMentions(text, agents).map((segment, index) =>
    segment.type === 'text' ? (
      segment.value
    ) : (
      <button
        key={index}
        type="button"
        title={messages.message.openSessionOf(segment.agent.ticketRef)}
        onClick={() => onOpenAgent(segment.agent.id)}
        className="rounded-[5px] bg-background/70 px-1 font-mono text-[13px] text-foreground ring-1 ring-border hover:bg-background"
      >
        {segment.value}
      </button>
    ),
  );
}
