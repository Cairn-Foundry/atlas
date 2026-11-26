// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useEffect, useRef, useState, type ReactElement } from 'react';
import type { AgentView, ChatMessage } from '@/lib/contract';
import { resetOrchestrator } from '@/lib/api-client';
import { useMessages } from '@/hooks/use-messages';
import { ChatMessageItem } from './ChatMessageItem';
import { Composer } from './Composer';
import { OrchestratorCore } from './OrchestratorCore';
import { StrokeIcon } from './StrokeIcon';
import { ThinkingIndicator } from './ThinkingIndicator';

interface OrchestratorPanelProps {
  messages: readonly ChatMessage[];
  agents: readonly AgentView[];
  isBusy: boolean;
  now: number;
  onOpenAgent: (agentId: string) => void;
}

export function OrchestratorPanel({ messages, agents, isBusy, now, onOpenAgent }: OrchestratorPanelProps): ReactElement {
  const text = useMessages();
  const threadRef = useRef<HTMLDivElement>(null);
  const [resetError, setResetError] = useState<string | null>(null);

  // The thread is `relative` so that visually hidden text (sr-only is absolutely positioned) scrolls
  // with it: anchored to the page instead, it stretched the document below the viewport.
  // Scroll the thread only. scrollIntoView would also scroll every ancestor, the window included:
  // in the stacked layout each new message dragged the page down and pushed the board off screen.
  useEffect(() => {
    const thread = threadRef.current;
    if (thread) thread.scrollTop = thread.scrollHeight;
  }, [messages.length, isBusy]);

  const startNewConversation = async (): Promise<void> => {
    setResetError(null);
    try {
      setResetError(await resetOrchestrator());
    } catch {
      setResetError(text.orchestrator.newConversationFailed);
    }
  };

  return (
    <aside aria-label={text.orchestrator.title} className="flex h-[560px] w-full flex-none flex-col border-t border-border bg-background lg:h-auto lg:w-[320px] lg:border-l lg:border-t-0 xl:w-[360px] 2xl:w-[400px]">
      <div className="flex h-14 flex-none items-center gap-2.5 border-b border-border bg-background pl-5 pr-3">
        <OrchestratorCore isBusy={isBusy} />
        <span className="text-[15px] font-semibold">{text.orchestrator.title}</span>
        <button
          type="button"
          aria-label={text.orchestrator.newConversation}
          title={text.orchestrator.newConversation}
          disabled={isBusy}
          onClick={() => void startNewConversation()}
          className="ml-auto flex size-9 items-center justify-center rounded-lg text-subtle transition-colors hover:bg-secondary hover:text-foreground disabled:cursor-default disabled:opacity-40 disabled:hover:bg-transparent"
        >
          <StrokeIcon size={16} strokeWidth={1.4} strokeLinecap="round">
            <path d="M8.5 2.5H4A1.5 1.5 0 0 0 2.5 4v8A1.5 1.5 0 0 0 4 13.5h8a1.5 1.5 0 0 0 1.5-1.5V7.5" />
            <path d="M11.5 2.5l2 2-5 5H6.5v-2z" />
          </StrokeIcon>
        </button>
      </div>
      {resetError && (
        <p role="status" className="m-0 border-b border-line px-5 py-2 text-xs text-muted-foreground">
          {resetError}
        </p>
      )}
      <div ref={threadRef} className="relative flex min-h-0 flex-1 flex-col gap-6 overflow-y-auto px-5 py-6" aria-live="polite">
        {messages.length === 0 && !isBusy && (
          <p className="m-0 text-sm text-muted-foreground">{text.orchestrator.emptyThread}</p>
        )}
        {messages.map((message) => (
          <ChatMessageItem key={message.id} message={message} now={now} agents={agents} onOpenAgent={onOpenAgent} />
        ))}
        {isBusy && <ThinkingIndicator />}
      </div>
      <Composer agents={agents} />
    </aside>
  );
}
