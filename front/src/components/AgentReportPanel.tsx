// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { ReactElement } from 'react';
import type { AgentReport, AgentView } from '@/lib/contract';
import { formatMessageTime } from '@/lib/time';
import { useMessages } from '@/hooks/use-messages';
import { MarkdownMessage } from './MarkdownMessage';

interface AgentReportPanelProps {
  report: AgentReport;
  agents: readonly AgentView[];
  now: number;
  onOpenAgent: (agentId: string) => void;
}

/** The rendering the agent wrote for the master, readable once its session is closed. */
export function AgentReportPanel({ report, agents, now, onOpenAgent }: AgentReportPanelProps): ReactElement {
  const messages = useMessages();
  return (
    <article className="atlas-report mx-auto w-full max-w-[820px] px-5 pb-12 pt-6 sm:px-8">
      <p className="m-0 mb-4 text-xs text-muted-foreground">
        {report.stepName}, {formatMessageTime(report.writtenAt, now, messages)}
      </p>
      <MarkdownMessage text={report.markdown} agents={agents} onOpenAgent={onOpenAgent} />
    </article>
  );
}
