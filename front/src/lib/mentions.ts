// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { AgentView } from '@/lib/contract';

const MAX_SUGGESTIONS = 6;

export interface MentionQuery {
  /** Index of the `@` in the text. */
  start: number;
  /** What the user typed after the `@`, up to the caret. */
  query: string;
}

export type MentionSegment = { type: 'text'; value: string } | { type: 'mention'; agent: AgentView; value: string };

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

/** A token `@REF` ends at the end of the text, a space or a punctuation sign, never inside a longer ref. */
function tokenPattern(ticketRef: string): RegExp {
  return new RegExp(`(^|\\s)@${escapeRegExp(ticketRef)}(?=$|[\\s.,;:!?)])`, 'i');
}

/** The mention being typed at the caret: an `@` at the start of the text or after a space, then no space. */
export function activeMentionQuery(text: string, caret: number): MentionQuery | null {
  const before = text.slice(0, caret);
  const match = /(^|\s)@([^\s@]*)$/.exec(before);
  if (!match) return null;
  const query = match[2] ?? '';
  return { start: caret - query.length - 1, query };
}

/** Agents whose ticket ref or title contains the query, ticket refs that start with it first. */
export function mentionCandidates(agents: readonly AgentView[], query: string, limit = MAX_SUGGESTIONS): AgentView[] {
  const needle = query.toLowerCase();
  const matches = agents.filter(
    (agent) => agent.ticketRef.toLowerCase().includes(needle) || agent.title.toLowerCase().includes(needle),
  );
  const rank = (agent: AgentView): number => (agent.ticketRef.toLowerCase().startsWith(needle) ? 0 : 1);
  return matches.toSorted((a, b) => rank(a) - rank(b) || a.ticketRef.localeCompare(b.ticketRef)).slice(0, limit);
}

/** Replaces the mention being typed with `@REF ` and returns the new text and caret. */
export function insertMention(text: string, mention: MentionQuery, caret: number, ticketRef: string): { text: string; caret: number } {
  const inserted = `@${ticketRef} `;
  // Spaces only: a line break after the mention is the master's, and stays.
  const after = text.slice(caret).replace(/^[ \t]+/, '');
  return { text: `${text.slice(0, mention.start)}${inserted}${after}`, caret: mention.start + inserted.length };
}

/** Ids of the agents whose `@REF` token is present in the text, each once. */
export function mentionedAgentIds(text: string, agents: readonly AgentView[]): string[] {
  return agents.filter((agent) => tokenPattern(agent.ticketRef).test(text)).map((agent) => agent.id);
}

/** Splits a message into plain text and `@REF` mentions of known agents, for rendering. */
export function splitMentions(text: string, agents: readonly AgentView[]): MentionSegment[] {
  const byRef = new Map(agents.map((agent) => [agent.ticketRef.toLowerCase(), agent]));
  const segments: MentionSegment[] = [];
  const pattern = /(^|\s)@([A-Za-z0-9][\w-]*)/g;
  let cursor = 0;
  for (const match of text.matchAll(pattern)) {
    const ref = (match[2] ?? '').replace(/[-_]+$/, '');
    const agent = byRef.get(ref.toLowerCase());
    if (!agent) continue;
    const start = match.index + (match[1] ?? '').length;
    if (start > cursor) segments.push({ type: 'text', value: text.slice(cursor, start) });
    segments.push({ type: 'mention', agent, value: `@${ref}` });
    cursor = start + ref.length + 1;
  }
  if (cursor < text.length) segments.push({ type: 'text', value: text.slice(cursor) });
  return segments;
}
