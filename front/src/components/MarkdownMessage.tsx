// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { Children, createContext, useContext, useMemo, type ReactElement, type ReactNode } from 'react';
import Markdown, { type Components } from 'react-markdown';
import remarkGfm from 'remark-gfm';
import type { AgentView } from '@/lib/contract';
import { MessageText } from './MessageText';

interface MarkdownMessageProps {
  text: string;
  agents: readonly AgentView[];
  onOpenAgent: (agentId: string) => void;
}

/**
 * The orchestrator answers in Markdown. Rendered without raw HTML; `@TICKET` mentions in the text
 * stay clickable chips (code spans and blocks are left untouched).
 */
export function MarkdownMessage({ text, agents, onOpenAgent }: MarkdownMessageProps): ReactElement {
  const mentionable = useMemo(() => ({ agents, onOpenAgent }), [agents, onOpenAgent]);
  return (
    <div className="atlas-markdown">
      <MentionableAgents value={mentionable}>
        <Markdown remarkPlugins={REMARK_PLUGINS} skipHtml components={COMPONENTS}>
          {text}
        </Markdown>
      </MentionableAgents>
    </div>
  );
}

const REMARK_PLUGINS = [remarkGfm];

const MentionableAgents = createContext<Pick<MarkdownMessageProps, 'agents' | 'onOpenAgent'>>({
  agents: [],
  onOpenAgent: () => undefined,
});

function Mentions({ children }: { children: ReactNode }): ReactNode {
  const { agents, onOpenAgent } = useContext(MentionableAgents);
  return Children.map(children, (child) =>
    typeof child === 'string' ? <MessageText text={child} agents={agents} onOpenAgent={onOpenAgent} /> : child,
  );
}

/**
 * Defined once, outside the render: components rebuilt at each render are new element types, and
 * React would remount the whole message at every event of the factory, dropping the selection and
 * the focus. The agents reach the mentions through a context instead.
 */
const COMPONENTS: Components = {
  p: ({ children }) => (
    <p>
      <Mentions>{children}</Mentions>
    </p>
  ),
  li: ({ children }) => (
    <li>
      <Mentions>{children}</Mentions>
    </li>
  ),
  strong: ({ children }) => (
    <strong>
      <Mentions>{children}</Mentions>
    </strong>
  ),
  em: ({ children }) => (
    <em>
      <Mentions>{children}</Mentions>
    </em>
  ),
  td: ({ children }) => (
    <td>
      <Mentions>{children}</Mentions>
    </td>
  ),
  th: ({ children }) => (
    <th>
      <Mentions>{children}</Mentions>
    </th>
  ),
  h1: ({ children }) => (
    <h3>
      <Mentions>{children}</Mentions>
    </h3>
  ),
  h2: ({ children }) => (
    <h3>
      <Mentions>{children}</Mentions>
    </h3>
  ),
  h3: ({ children }) => (
    <h3>
      <Mentions>{children}</Mentions>
    </h3>
  ),
  a: ({ href, children }) => (
    <a href={href} target="_blank" rel="noopener noreferrer">
      {children}
    </a>
  ),
};
