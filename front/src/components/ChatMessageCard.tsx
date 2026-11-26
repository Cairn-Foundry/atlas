// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { ReactElement } from 'react';
import type { StepState, MessageKind, AgentView, MessageCard } from '@/lib/contract';
import { cn } from 'cn';
import { useMessages } from '@/hooks/use-messages';
import { MarkdownMessage } from './MarkdownMessage';

export type CardKind = Exclude<MessageKind, 'text'>;

const CHIP_LOOK: Record<StepState, string> = {
  ['upcoming']: 'border-dashed border-slot text-subtle',
  ['current']: 'border-foreground/30 font-medium text-foreground',
  ['done']: 'bg-secondary text-muted-foreground',
  ['skipped']: 'border-dashed border-slot text-muted-foreground line-through',
  ['external']: 'bg-secondary text-muted-foreground',
};

interface ChatMessageCardProps {
  kind: CardKind;
  card: MessageCard;
  /** The reason the orchestrator gave. */
  text: string;
  agents: readonly AgentView[];
  onOpenAgent: (agentId: string) => void;
}

/** A launch, decision or workflow change of the orchestrator: what it did, for which ticket, why. */
export function ChatMessageCard({ kind, card, text, agents, onOpenAgent }: ChatMessageCardProps): ReactElement {
  const messages = useMessages();
  return (
    <div className="flex min-w-0 flex-col gap-2 rounded-[10px] border bg-card p-3 text-card-foreground">
      <p className="m-0 flex items-baseline gap-2 text-xs">
        <span className="font-medium text-subtle">{messages.message.cardKind[kind]}</span>
        <span className="font-mono text-muted-foreground">{card.ticketRef}</span>
      </p>
      <MarkdownMessage text={text} agents={agents} onOpenAgent={onOpenAgent} />
      <ul className="m-0 flex list-none flex-wrap gap-1.5 p-0">
        {card.steps.map((step, index) => (
          <li key={index} className={cn('rounded-[5px] border border-transparent px-1.5 py-px text-xs', CHIP_LOOK[step.state])}>
            {step.name}
            <span className="sr-only"> ({messages.steps.state[step.state]})</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
