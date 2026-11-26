// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { lazy, Suspense, useState, type ReactElement } from 'react';
import type { AgentView } from '@/lib/contract';
import { cn } from 'cn';
import { closeAgent, refusalOf, resumeAgent } from '@/lib/api-client';
import { BOARD_HASH } from '@/lib/route';
import { elapsedLabel, statusLook } from '@/lib/status';
import { currentStep } from '@/lib/steps';
import { useMessages } from '@/hooks/use-messages';
import { Button } from '@/components/ui/button';
import { AgentReportPanel } from './AgentReportPanel';
import { ArchiveButton } from './ArchiveButton';
import { STATUS_TONE } from './status-tone';

/** Secondary actions of the header bar. */
const HEADER_ACTION_CLASS = 'h-11 px-3 text-[13px] font-normal';

const AgentTerminal = lazy(() => import('./AgentTerminal').then((module) => ({ default: module.AgentTerminal })));

type Pane = 'report' | 'terminal';

interface SessionViewProps {
  agent: AgentView;
  agents: readonly AgentView[];
  now: number;
  onBack: () => void;
  onOpenAgent: (agentId: string) => void;
}

export function SessionView({ agent, agents, now, onBack, onOpenAgent }: SessionViewProps): ReactElement {
  const messages = useMessages();
  const [error, setError] = useState<string | null>(null);
  /** The pane the master picked; until then, the report when the session is closed, else the terminal. */
  const [chosenPane, setChosenPane] = useState<Pane | null>(null);
  const stepName = currentStep(agent)?.name ?? null;
  const look = statusLook(agent.status, messages);
  const elapsed = elapsedLabel(agent, now);
  const hasLiveSession = agent.liveSessionId !== null;
  // `?? null`: a daemon older than the front sends no report at all.
  const report = agent.report ?? null;
  const pane: Pane = report === null ? 'terminal' : (chosenPane ?? (hasLiveSession ? 'terminal' : 'report'));

  const run = (action: (agentId: string) => Promise<void>, failure: string): void => {
    setError(null);
    // What the daemon refused says more than "it failed": the master reads why.
    action(agent.id).catch((cause: unknown) => setError(refusalOf(cause) ?? failure));
  };

  return (
    <main className="flex min-h-[560px] min-w-0 flex-1 flex-col bg-terminal lg:min-h-0">
      <div className="flex min-h-[52px] flex-none flex-wrap items-center justify-between gap-x-3 border-b border-line bg-background py-1 pl-4 pr-3 sm:pl-6">
        <nav aria-label={messages.session.breadcrumbLabel} className="flex min-w-0 items-center gap-2 text-sm">
          <a
            href={BOARD_HASH}
            onClick={(event) => {
              event.preventDefault();
              onBack();
            }}
            className="flex min-h-11 items-center text-muted-foreground no-underline hover:text-foreground"
          >
            {messages.session.factory}
          </a>
          <span aria-hidden="true" className="text-border">
            /
          </span>
          <span className="truncate font-medium">{agent.title}</span>
          <span className="font-mono text-xs text-muted-foreground">{agent.ticketRef}</span>
        </nav>
        <div className="flex items-center gap-1 text-[13px]">
          <span className="mr-3 text-muted-foreground" title={agent.statusReason ?? undefined}>
            {stepName !== null && `${stepName}, `}
            <span className={cn('font-medium', STATUS_TONE[agent.status].text)}>
              {look.phrase}
            </span>
            {elapsed !== null && messages.session.since(elapsed)}
          </span>
          {agent.mrUrl !== null && (
            <Button variant="ghost" asChild className={HEADER_ACTION_CLASS}>
              <a href={agent.mrUrl} target="_blank" rel="noreferrer">
                {messages.session.viewMergeRequest}
              </a>
            </Button>
          )}
          <Button
            variant={hasLiveSession ? 'ghost' : 'default'}
            className={cn('h-11 px-3 text-[13px]', hasLiveSession ? 'font-normal' : 'font-medium')}
            disabled={hasLiveSession}
            onClick={() => run(resumeAgent, messages.session.resumeFailed)}
          >
            {messages.session.resume}
          </Button>
          <Button
            variant="ghost"
            className={HEADER_ACTION_CLASS}
            disabled={!hasLiveSession}
            onClick={() => run(closeAgent, messages.session.closeFailed)}
          >
            {messages.session.close}
          </Button>
          {agent.status === 'done' && <ArchiveButton agentId={agent.id} variant="ghost" className={HEADER_ACTION_CLASS} />}
        </div>
      </div>
      {error !== null && (
        <p role="alert" className="m-0 bg-background px-6 pb-2 text-xs text-blocked-text">
          {error}
        </p>
      )}
      {report !== null && (
        <div role="tablist" aria-label={messages.session.panesLabel} className="flex flex-none gap-1 border-b border-line bg-background px-3 sm:px-5">
          {(['report', 'terminal'] as const).map((candidate) => (
            <button
              key={candidate}
              type="button"
              role="tab"
              aria-selected={pane === candidate}
              onClick={() => setChosenPane(candidate)}
              className={cn(
                '-mb-px min-h-11 border-b-2 px-3 text-[13px] transition-colors',
                pane === candidate ? 'border-foreground font-medium text-foreground' : 'border-transparent text-muted-foreground hover:text-foreground',
              )}
            >
              {messages.session.pane[candidate]}
            </button>
          ))}
        </div>
      )}
      {pane === 'report' && report !== null ? (
        <div role="tabpanel" aria-label={messages.session.pane.report} className="min-h-0 flex-1 overflow-y-auto bg-background">
          <AgentReportPanel report={report} agents={agents} now={now} onOpenAgent={onOpenAgent} />
        </div>
      ) : (
      <div className="min-h-0 flex-1 px-4 pb-6 pt-5 sm:px-7">
        {agent.liveSessionId === null ? (
          <p className="m-0 font-mono text-[13px] leading-[21px] text-terminal-foreground/70">
            {messages.session.closedNotice}
          </p>
        ) : (
          <Suspense fallback={null}>
            <AgentTerminal agentId={agent.id} sessionId={agent.liveSessionId} />
          </Suspense>
        )}
      </div>
      )}
    </main>
  );
}
