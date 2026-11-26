// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useId, useLayoutEffect, useRef, useState, type ChangeEvent, type KeyboardEvent, type RefObject, type SyntheticEvent } from 'react';
import type { AgentView } from '@/lib/contract';
import { activeMentionQuery, insertMention, mentionCandidates } from '@/lib/mentions';

export interface MentionCombobox {
  text: string;
  suggestions: readonly AgentView[];
  isListOpen: boolean;
  highlightedIndex: number;
  listboxId: string;
  optionId: (index: number) => string;
  highlight: (index: number) => void;
  pick: (agent: AgentView) => void;
  /** Empties the field, once its text has been sent. */
  clear: () => void;
  /** Moves through, picks from or closes the list; true when the key was used for that. */
  handleListKeyDown: (event: KeyboardEvent<HTMLTextAreaElement>) => boolean;
  handleChange: (event: ChangeEvent<HTMLTextAreaElement>) => void;
  handleSelect: (event: SyntheticEvent<HTMLTextAreaElement>) => void;
  handleBlur: () => void;
  handleFocus: () => void;
}

/**
 * The text of a field where typing `@` suggests agents to mention: the suggestion list, the
 * highlighted option and the keys that drive them, following the ARIA combobox pattern.
 */
export function useMentionCombobox(agents: readonly AgentView[], textareaRef: RefObject<HTMLTextAreaElement | null>): MentionCombobox {
  const listboxId = useId();
  const pendingCaret = useRef<number | null>(null);
  const [text, setText] = useState('');
  const [caret, setCaret] = useState(0);
  const [activeIndex, setActiveIndex] = useState(0);
  const [dismissedAt, setDismissedAt] = useState<number | null>(null);

  const mention = activeMentionQuery(text, caret);
  const suggestions = mention === null || mention.start === dismissedAt ? [] : mentionCandidates(agents, mention.query);
  const isListOpen = suggestions.length > 0;
  const highlightedIndex = Math.min(activeIndex, Math.max(suggestions.length - 1, 0));

  // A picked mention moves the caret after it, once the new text is in the field.
  useLayoutEffect(() => {
    if (pendingCaret.current === null || textareaRef.current === null) return;
    textareaRef.current.setSelectionRange(pendingCaret.current, pendingCaret.current);
    pendingCaret.current = null;
  });

  const handleSelect = (event: SyntheticEvent<HTMLTextAreaElement>): void => {
    setCaret(event.currentTarget.selectionStart);
  };

  const pick = (agent: AgentView): void => {
    if (mention === null) return;
    const next = insertMention(text, mention, caret, agent.ticketRef);
    setText(next.text);
    setCaret(next.caret);
    setActiveIndex(0);
    pendingCaret.current = next.caret;
    textareaRef.current?.focus();
  };

  const handleListKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>): boolean => {
    if (!isListOpen) return false;
    const picked = suggestions[highlightedIndex];
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      const step = event.key === 'ArrowDown' ? 1 : -1;
      setActiveIndex((highlightedIndex + step + suggestions.length) % suggestions.length);
      return true;
    }
    if ((event.key === 'Enter' || event.key === 'Tab') && picked !== undefined) {
      event.preventDefault();
      pick(picked);
      return true;
    }
    if (event.key === 'Escape' && mention !== null) {
      event.preventDefault();
      setDismissedAt(mention.start);
      return true;
    }
    return false;
  };

  return {
    text,
    suggestions,
    isListOpen,
    highlightedIndex,
    listboxId,
    optionId: (index) => `${listboxId}-option-${index}`,
    highlight: setActiveIndex,
    pick,
    clear: () => {
      setText('');
      setCaret(0);
    },
    handleListKeyDown,
    handleChange: (event) => {
      setText(event.target.value);
      handleSelect(event);
      setActiveIndex(0);
    },
    handleSelect,
    handleBlur: () => setDismissedAt(mention?.start ?? null),
    handleFocus: () => setDismissedAt(null),
  };
}
