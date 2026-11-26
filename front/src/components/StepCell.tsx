// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { ReactElement } from 'react';
import type { AgentStepView, AgentView } from '@/lib/contract';
import { cn } from 'cn';
import { activeMs, formatTokens, passLabel, weightedTokens } from '@/lib/steps';
import { formatDuration } from '@/lib/time';
import { useMessages } from '@/hooks/use-messages';
import { AgentCard } from './AgentCard';

interface StepCellProps {
  agent: AgentView;
  step: AgentStepView;
  now: number;
  isDimmed: boolean;
  onOpenAgent: (agentId: string) => void;
}

function FinishedStep({ step }: { step: AgentStepView }): ReactElement {
  const messages = useMessages();
  const isExternal = step.state === 'external';
  const duration = isExternal ? null : activeMs(step);
  const pass = passLabel(step.pass, messages);
  return (
    <div className="flex h-[104px] flex-col justify-between rounded-[10px] border bg-background/60 dark:bg-card/60 p-3 text-muted-foreground">
      <span className="flex min-w-0 items-baseline gap-2">
        <span className="truncate text-[13px] font-medium text-foreground">{step.name}</span>
        {pass !== null && <span className="flex-none text-xs">{pass}</span>}
      </span>
      <span className="flex min-w-0 items-baseline gap-1.5 whitespace-nowrap text-xs">
        {isExternal ? (
          <span>{messages.steps.state.external}</span>
        ) : (
          <>
            <span>{messages.steps.finished}</span>
            {duration !== null && <span>{formatDuration(duration)}</span>}
            {step.usage !== null && (
              <span className="tabular-nums" title={messages.steps.tokensTitle}>
                {formatTokens(weightedTokens(step.usage), messages)}
                <span className="sr-only">{messages.steps.tokensSpoken}</span>
              </span>
            )}
          </>
        )}
      </span>
    </div>
  );
}

function EmptySlot({ step }: { step: AgentStepView }): ReactElement {
  const messages = useMessages();
  const isSkipped = step.state === 'skipped';
  const pass = passLabel(step.pass, messages);
  return (
    <div className="flex h-[104px] items-start gap-2 rounded-[10px] border border-dashed border-slot p-3 text-[13px] text-subtle">
      <span className={cn('truncate', isSkipped && 'line-through')}>{step.name}</span>
      {isSkipped && <span className="sr-only">({messages.steps.state.skipped})</span>}
      {pass !== null && <span className="flex-none text-xs">{pass}</span>}
    </div>
  );
}

export function StepCell({ agent, step, now, isDimmed, onOpenAgent }: StepCellProps): ReactElement {
  switch (step.state) {
    case 'current':
      return <AgentCard agent={agent} now={now} isDimmed={isDimmed} onOpen={onOpenAgent} />;
    case 'done':
    case 'external':
      return <FinishedStep step={step} />;
    case 'upcoming':
    case 'skipped':
      return <EmptySlot step={step} />;
    default: {
      const unknown: never = step.state;
      throw new Error(`Unknown step state: ${String(unknown)}`);
    }
  }
}
