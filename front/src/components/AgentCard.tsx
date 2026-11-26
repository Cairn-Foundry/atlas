// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { ReactElement } from 'react';
import type { AgentView } from '@/lib/contract';
import { cn } from 'cn';
import { elapsedLabel, statusLook } from '@/lib/status';
import { currentStep, passLabel } from '@/lib/steps';
import { useMessages } from '@/hooks/use-messages';
import { StatusGlyph } from './StatusGlyph';
import { STATUS_TONE } from './status-tone';

interface AgentCardProps {
  agent: AgentView;
  now: number;
  isDimmed: boolean;
  onOpen: (agentId: string) => void;
}

export function AgentCard({ agent, now, isDimmed, onOpen }: AgentCardProps): ReactElement {
  const messages = useMessages();
  const look = statusLook(agent.status, messages);
  const tone = STATUS_TONE[agent.status];
  const elapsed = elapsedLabel(agent, now) ?? '';
  const reason = agent.status === 'blocked' && agent.statusReason !== null ? agent.statusReason : null;
  const step = currentStep(agent);
  const pass = passLabel(step?.pass ?? 1, messages);
  const label = [agent.ticketRef, agent.repoName, agent.title, step?.name, pass, look.phrase, elapsed, reason].filter(Boolean).join(', ');

  return (
    <button
      type="button"
      aria-label={label}
      aria-hidden={isDimmed || undefined}
      tabIndex={isDimmed ? -1 : undefined}
      title={reason ?? agent.title}
      onClick={() => onOpen(agent.id)}
      data-status={agent.status}
      data-dimmed={isDimmed || undefined}
      className={cn(
        'atlas-card nodrag nopan animate-in fade-in zoom-in-95 duration-300 motion-reduce:animate-none flex h-[104px] w-full flex-col justify-between rounded-[10px] border bg-card p-3 text-left text-card-foreground shadow-[0_1px_2px_rgba(16,18,23,0.04)] transition-[opacity,border-color,box-shadow] duration-150 motion-reduce:transition-none',
        tone.border,
        isDimmed
          ? 'opacity-40'
          : 'hover:border-foreground/25 hover:shadow-[0_1px_2px_rgba(16,18,23,0.06),0_6px_16px_-8px_rgba(16,18,23,0.22)] focus-visible:border-foreground/25 focus-visible:shadow-[0_1px_2px_rgba(16,18,23,0.06),0_6px_16px_-8px_rgba(16,18,23,0.22)]',
      )}
    >
      <span aria-hidden="true" className="atlas-card-field" />
      <span aria-hidden="true" className="atlas-card-beam" />
      {/* Keyed by status: the sweep plays again each time the status changes. */}
      <span key={agent.status} aria-hidden="true" className="atlas-card-flash" />
      <span className="relative flex min-w-0 items-center gap-2">
        <StatusGlyph status={agent.status} />
        <span className="min-w-0 truncate font-mono text-xs text-muted-foreground">{agent.ticketRef}</span>
        <span className="ml-auto max-w-[45%] flex-none truncate rounded-[5px] bg-secondary px-1.5 py-px text-[11px] leading-4 text-subtle">{agent.repoName}</span>
      </span>
      <span className="relative line-clamp-2 text-[13px] leading-[18px]">{agent.title}</span>
      <span className="relative flex min-w-0 items-baseline gap-1.5 whitespace-nowrap text-xs">
        {step !== undefined && <span className="flex-none font-medium">{step.name}</span>}
        {pass !== null && <span className="flex-none text-muted-foreground">{pass}</span>}
        {look.note !== null && <span className={cn('truncate font-medium', tone.text)}>{look.note}</span>}
        {elapsed !== '' && <span className="flex-none text-muted-foreground">{elapsed}</span>}
      </span>
    </button>
  );
}
