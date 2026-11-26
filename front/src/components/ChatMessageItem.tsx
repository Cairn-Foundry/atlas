// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useEffect, useState, type ReactElement } from 'react';
import type { AgentView, ChatMessage } from '@/lib/contract';
import { formatMessageTime } from '@/lib/time';
import { useMessages } from '@/hooks/use-messages';
import { ChatMessageCard } from './ChatMessageCard';
import { MarkdownMessage } from './MarkdownMessage';
import { MessageActions } from './MessageActions';
import { MessageText } from './MessageText';

interface ChatMessageItemProps {
  message: ChatMessage;
  now: number;
  agents: readonly AgentView[];
  onOpenAgent: (agentId: string) => void;
}

function MessageTime({ iso, now }: { iso: string; now: number }): ReactElement {
  const messages = useMessages();
  return (
    <time dateTime={iso} className="text-xs text-muted-foreground">
      {formatMessageTime(iso, now, messages)}
    </time>
  );
}

const COPIED_NOTICE_MS = 1500;

/** Copies `text` to the clipboard; shown when the message is hovered or the button focused. */
function CopyButton({ text }: { text: string }): ReactElement {
  const messages = useMessages();
  const [isCopied, setIsCopied] = useState(false);
  useEffect(() => {
    if (!isCopied) return;
    const timer = setTimeout(() => setIsCopied(false), COPIED_NOTICE_MS);
    return () => clearTimeout(timer);
  }, [isCopied]);

  const copy = (): void => {
    navigator.clipboard.writeText(text).then(
      () => setIsCopied(true),
      () => setIsCopied(false),
    );
  };

  return (
    <button
      type="button"
      onClick={copy}
      className="rounded-[5px] px-1 text-xs text-muted-foreground opacity-0 transition-opacity hover:text-foreground focus-visible:opacity-100 group-hover:opacity-100 group-focus-within:opacity-100 [@media(hover:none)]:opacity-100"
    >
      <span aria-live="polite">{isCopied ? messages.message.copied : messages.message.copy}</span>
    </button>
  );
}

export function ChatMessageItem({ message, now, agents, onOpenAgent }: ChatMessageItemProps): ReactElement {
  if (message.role === 'user') {
    return (
      <div className="group flex min-w-0 max-w-[85%] flex-col items-end gap-1 self-end animate-in fade-in slide-in-from-bottom-1 duration-300 motion-reduce:animate-none">
        <div className="max-w-full whitespace-pre-wrap [overflow-wrap:anywhere] rounded-[18px] bg-secondary px-3.5 py-2.5 text-sm leading-[21px]">
          <MessageText text={message.text} agents={agents} onOpenAgent={onOpenAgent} />
        </div>
        <span className="flex items-center gap-1.5 pr-1">
          <CopyButton text={message.text} />
          <MessageTime iso={message.createdAt} now={now} />
        </span>
      </div>
    );
  }
  const { actions, agentId, card, kind } = message;
  return (
    <div className="flex flex-col items-start gap-2.5 text-sm leading-[22px] animate-in fade-in slide-in-from-bottom-1 duration-300 motion-reduce:animate-none">
      <div className="group flex min-w-0 max-w-full flex-col gap-1">
        {card !== null && kind !== 'text' ? (
          <ChatMessageCard kind={kind} card={card} text={message.text} agents={agents} onOpenAgent={onOpenAgent} />
        ) : (
          <MarkdownMessage text={message.text} agents={agents} onOpenAgent={onOpenAgent} />
        )}
        <span className="flex items-center gap-1.5">
          <MessageTime iso={message.createdAt} now={now} />
          <CopyButton text={message.text} />
        </span>
      </div>
      <MessageActions actions={actions} messageId={message.id} agentId={agentId} onOpenAgent={onOpenAgent} />
    </div>
  );
}
