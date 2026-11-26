// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { ReactElement } from 'react';
import type { MessageAction } from '@/lib/contract';
import { resumeAgent, saveNote } from '@/lib/api-client';
import { useMessages } from '@/hooks/use-messages';
import { Button } from '@/components/ui/button';
import { ApiActionButton } from './ApiActionButton';
import { ArchiveButton } from './ArchiveButton';
import { CatalogueChangeCard } from './CatalogueChangeCard';
import { StrokeIcon } from './StrokeIcon';

const BUTTON_CLASS = 'h-11 rounded-[10px] px-3.5 text-[13px]';

interface MessageActionsProps {
  actions: readonly MessageAction[];
  /** The message the actions sit under: a decided proposal is recorded on it. */
  messageId: string;
  /** The agent the session, resume and archive actions act on. */
  agentId: string | null;
  onOpenAgent: (agentId: string) => void;
}

interface ActionButtonProps {
  action: MessageAction;
  messageId: string;
  agentId: string | null;
  onOpenAgent: (agentId: string) => void;
}

function ActionButton({ action, messageId, agentId, onOpenAgent }: ActionButtonProps): ReactElement | null {
  const messages = useMessages();
  if (action.type === 'link') {
    return (
      <Button asChild variant="outline" className={BUTTON_CLASS}>
        <a href={action.url} target="_blank" rel="noreferrer">
          {action.label}
        </a>
      </Button>
    );
  }
  if (action.type === 'save-note') {
    const label = action.repo === null ? messages.actions.rememberForAll : messages.actions.rememberForProject;
    return <ApiActionButton label={label} variant="outline" className={BUTTON_CLASS} isSingleUse run={() => saveNote(action.repo, action.note)} />;
  }
  if (action.type === 'apply-catalogue-change') return <CatalogueChangeCard messageId={messageId} change={action.change} decision={action.decision} />;
  if (agentId === null) return null;
  switch (action.type) {
    case 'session':
      return (
        <Button variant="outline" className={BUTTON_CLASS} onClick={() => onOpenAgent(agentId)}>
          <StrokeIcon size={14} strokeWidth={1.3}>
            <path d="M2 4l3 3-3 3M7 10h5" />
          </StrokeIcon>
          {messages.actions.openSession}
        </Button>
      );
    case 'resume':
      return <ApiActionButton label={messages.actions.reopenSession} variant="outline" className={BUTTON_CLASS} run={() => resumeAgent(agentId)} />;
    case 'archive':
      return <ArchiveButton agentId={agentId} variant="outline" className={BUTTON_CLASS} />;
    default: {
      const unknown: never = action;
      throw new Error(`Unknown action: ${JSON.stringify(unknown)}`);
    }
  }
}

/** The buttons the orchestrator offers under a message, in order; the row wraps on a narrow panel. */
export function MessageActions({ actions, messageId, agentId, onOpenAgent }: MessageActionsProps): ReactElement | null {
  if (actions.length === 0) return null;
  return (
    <div className="flex flex-wrap gap-2">
      {actions.map((action, index) => (
        <ActionButton key={index} action={action} messageId={messageId} agentId={agentId} onOpenAgent={onOpenAgent} />
      ))}
    </div>
  );
}
