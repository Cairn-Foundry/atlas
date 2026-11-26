// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { ReactElement } from 'react';
import type { AgentView } from '@/lib/contract';
import { matchesFilter, type FilterKey } from '@/lib/filters';
import { currentStep } from '@/lib/steps';
import { AgentCard } from './AgentCard';
import { StepCell } from './StepCell';

/** A column of the board: as wide as `COLUMN_WIDTH` in lib/canvas on wide screens. */
const CELL_CLASS = 'min-w-0 md:w-[208px] md:flex-none';

interface BoardRowProps {
  agent: AgentView;
  filter: FilterKey;
  now: number;
  onOpenAgent: (agentId: string) => void;
}

/** One agent's workflow: the steps left to right on wide screens, stacked on small ones, then the agent card when it is not on a step. */
export function BoardRow({ agent, filter, now, onOpenAgent }: BoardRowProps): ReactElement {
  const isDimmed = !matchesFilter(agent, filter);
  return (
    <section aria-label={`${agent.ticketRef}, ${agent.repoName}`} className="flex flex-col gap-2">
      <h2 className="m-0 flex h-6 items-center gap-2 px-0.5 text-sm font-semibold">
        <span className="font-mono text-xs">{agent.ticketRef}</span>
        {[agent.repoName, ...agent.extraRepos].map((repoName) => (
          <span key={repoName} className="truncate rounded-[5px] bg-secondary px-1.5 py-px text-[11px] font-normal leading-4 text-subtle">
            {repoName}
          </span>
        ))}
      </h2>
      <div className="flex flex-col gap-2 md:flex-row md:gap-6">
        {agent.steps.map((step) => (
          <div key={step.id} className={CELL_CLASS}>
            <StepCell agent={agent} step={step} now={now} isDimmed={isDimmed} onOpenAgent={onOpenAgent} />
          </div>
        ))}
        {/* No current step, so no card in the steps above: the agent closes its row. */}
        {currentStep(agent) === undefined && (
          <div className={CELL_CLASS}>
            <AgentCard agent={agent} now={now} isDimmed={isDimmed} onOpen={onOpenAgent} />
          </div>
        )}
      </div>
    </section>
  );
}
