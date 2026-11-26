// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useId, useRef, useState, type FormEvent, type KeyboardEvent, type ReactElement } from 'react';
import type { AgentView } from '@/lib/contract';
import { cn } from 'cn';
import { refusalOf, sendOrchestratorMessage } from '@/lib/api-client';
import { mentionedAgentIds } from '@/lib/mentions';
import { useMentionCombobox } from '@/hooks/use-mention-combobox';
import { useMessages } from '@/hooks/use-messages';
import { Textarea } from '@/components/ui/textarea';
import { StrokeIcon } from './StrokeIcon';

const IME_KEY_CODE = 229;

interface ComposerProps {
  agents: readonly AgentView[];
}

export function Composer({ agents }: ComposerProps): ReactElement {
  const messages = useMessages();
  const textareaId = useId();
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const combobox = useMentionCombobox(agents, textareaRef);
  const { text, suggestions, isListOpen, highlightedIndex, listboxId, optionId } = combobox;
  const [isBroadcast, setIsBroadcast] = useState(false);
  const [isSending, setIsSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const canSend = text.trim() !== '' && !isSending;

  const send = async (): Promise<void> => {
    if (!canSend) return;
    setIsSending(true);
    setError(null);
    try {
      const message = text.trim();
      await sendOrchestratorMessage({ text: message, broadcast: isBroadcast, mentions: mentionedAgentIds(message, agents) });
      // What was typed while the message was leaving stays in the field.
      if (textareaRef.current?.value.trim() === message) combobox.clear();
      setIsBroadcast(false);
    } catch (cause) {
      const refusal = refusalOf(cause);
      setError(refusal === null ? messages.composer.sendFailed : messages.composer.sendRefused(refusal));
    } finally {
      setIsSending(false);
    }
  };

  const handleSubmit = (event: FormEvent<HTMLFormElement>): void => {
    event.preventDefault();
    void send();
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>): void => {
    // A key that serves an input method (it commits a composition) is not ours: WebKit reports the
    // committing Enter as not composing, and only its key code 229 tells.
    if (event.nativeEvent.isComposing || event.keyCode === IME_KEY_CODE) return;
    if (combobox.handleListKeyDown(event)) return;
    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault();
      void send();
    }
  };

  return (
    <form onSubmit={handleSubmit} className="relative flex-none px-4 pb-5 pt-3">
      {isListOpen && (
        <ul
          id={listboxId}
          role="listbox"
          aria-label={messages.composer.mentionListLabel}
          className="absolute inset-x-4 bottom-full z-10 m-0 flex list-none flex-col gap-0.5 rounded-[14px] border bg-popover p-1.5 shadow-[0_8px_24px_-12px_rgba(16,18,23,0.25)]"
        >
          {suggestions.map((agent, index) => (
            <li
              key={agent.id}
              id={optionId(index)}
              role="option"
              aria-selected={index === highlightedIndex}
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => combobox.pick(agent)}
              onMouseEnter={() => combobox.highlight(index)}
              className={cn(
                'flex min-h-10 items-center gap-2.5 rounded-[10px] px-2.5 text-[13px]',
                index === highlightedIndex && 'bg-secondary',
              )}
            >
              <span className="font-mono text-xs">{agent.ticketRef}</span>
              <span className="truncate text-muted-foreground">{agent.title}</span>
            </li>
          ))}
        </ul>
      )}
      <div className="flex flex-col gap-1 rounded-[18px] border bg-background py-3 pl-3.5 pr-3 focus-within:outline-2 focus-within:outline-offset-2 focus-within:outline-ring">
        <label htmlFor={textareaId} className="sr-only">
          {messages.composer.messageLabel}
        </label>
        <Textarea
          ref={textareaRef}
          id={textareaId}
          rows={2}
          value={text}
          role="combobox"
          aria-autocomplete="list"
          aria-expanded={isListOpen}
          aria-controls={isListOpen ? listboxId : undefined}
          aria-activedescendant={isListOpen ? optionId(highlightedIndex) : undefined}
          onChange={combobox.handleChange}
          onSelect={combobox.handleSelect}
          onKeyDown={handleKeyDown}
          onBlur={combobox.handleBlur}
          onFocus={combobox.handleFocus}
          placeholder={isBroadcast ? messages.composer.broadcastPlaceholder : messages.composer.placeholder}
          className="max-h-[40vh] min-h-0 resize-none overflow-y-auto [overflow-wrap:anywhere] border-0 p-0 text-sm dark:bg-transparent leading-[21px] shadow-none focus-visible:outline-none focus-visible:ring-0"
        />
        <div className="flex items-center justify-between gap-2">
          <button
            type="button"
            aria-pressed={isBroadcast}
            onClick={() => setIsBroadcast((current) => !current)}
            className={cn(
              'flex h-11 items-center gap-1.5 rounded-[10px] px-2.5 text-[13px] transition-colors',
              isBroadcast ? 'bg-secondary text-foreground' : 'text-muted-foreground hover:bg-secondary',
            )}
          >
            <StrokeIcon size={16} strokeWidth={1.3}>
              <circle cx="8" cy="8" r="1.5" />
              <path d="M4.5 4.5a5 5 0 000 7M11.5 4.5a5 5 0 010 7M2.5 2.5a8 8 0 000 11M13.5 2.5a8 8 0 010 11" />
            </StrokeIcon>
            {messages.composer.broadcast}
          </button>
          <button
            type="submit"
            aria-label={messages.composer.send}
            disabled={!canSend}
            className="flex size-11 items-center justify-center rounded-full bg-primary text-primary-foreground transition-opacity disabled:opacity-30"
          >
            <StrokeIcon size={16} strokeWidth={1.6}>
              <path d="M8 13V3M3.5 7.5L8 3l4.5 4.5" />
            </StrokeIcon>
          </button>
        </div>
      </div>
      {error !== null && (
        <p role="alert" className="m-0 mt-2 text-xs text-blocked-text">
          {error}
        </p>
      )}
    </form>
  );
}
