// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useState, type ReactElement } from 'react';
import type { CatalogueChange, CatalogueDecision } from '@/lib/contract';
import { decideCatalogueChange, refusalOf } from '@/lib/api-client';
import { useMessages } from '@/hooks/use-messages';
import type { Messages } from '@/lib/messages/fr';
import { Button } from '@/components/ui/button';

type Status = { state: 'idle' | 'pending' } | { state: 'failed'; reason: string };

const BUTTON_CLASS = 'h-11 rounded-[10px] px-3.5 text-[13px]';

interface CatalogueChangeCardProps {
  messageId: string;
  change: CatalogueChange;
  /** What the master decided, null while the change waits for him. */
  decision: CatalogueDecision | null;
}

/** What the change touches, as a title: the kind of item and its name. */
function changeTitle(change: CatalogueChange, messages: Messages): string {
  switch (change.type) {
    case 'brick':
      return messages.catalogue.brick(change.id);
    case 'skill':
      return messages.catalogue.skill(change.name);
    case 'subagent':
      return messages.catalogue.subagent(change.name);
    case 'disable':
      return messages.catalogue.disable(change.id);
    case 'enable':
      return messages.catalogue.enable(change.id);
    default: {
      const unknown: never = change;
      throw new Error(`Unknown change: ${JSON.stringify(unknown)}`);
    }
  }
}

/** The files the change writes, by path, so the master reads what he validates. */
function changeFiles(change: CatalogueChange): [string, string][] {
  switch (change.type) {
    case 'brick':
      return [[`bricks/${change.id}.yml`, change.content]];
    case 'skill':
      return Object.entries(change.files).map(([path, content]) => [`skills/${change.name}/${path}`, content ?? '']);
    case 'subagent':
      return [[`subagents/${change.name}.md`, change.content]];
    case 'disable':
    case 'enable':
      return [];
  }
}

function FileView({ path, content }: { path: string; content: string }): ReactElement {
  return (
    <details className="min-w-0 rounded-[8px] border bg-background">
      <summary className="cursor-pointer px-2.5 py-1.5 font-mono text-xs text-muted-foreground">{path}</summary>
      <pre className="m-0 max-h-80 overflow-auto border-t px-2.5 py-2 font-mono text-xs leading-[18px] whitespace-pre-wrap [overflow-wrap:anywhere]">
        {content}
      </pre>
    </details>
  );
}

/**
 * A change to the catalogue the orchestrator proposes: what it writes, file by file, and the buttons
 * that apply or refuse it. Nothing is written before the master validates; the daemon keeps his
 * decision on the message, and a refusal of the daemon says why in place.
 */
export function CatalogueChangeCard({ messageId, change, decision }: CatalogueChangeCardProps): ReactElement {
  const messages = useMessages();
  const [status, setStatus] = useState<Status>({ state: 'idle' });
  /** Until the message comes back with it, the decision the daemon just accepted. */
  const [madeHere, setMadeHere] = useState<CatalogueDecision | null>(null);
  const decided = decision ?? madeHere;
  const files = changeFiles(change);

  const decide = (wanted: CatalogueDecision): void => {
    setStatus({ state: 'pending' });
    decideCatalogueChange(messageId, wanted).then(
      () => {
        setMadeHere(wanted);
        setStatus({ state: 'idle' });
      },
      (cause: unknown) => setStatus({ state: 'failed', reason: refusalOf(cause) ?? messages.catalogue.unreachable }),
    );
  };

  return (
    <div className="flex w-full min-w-0 flex-col gap-2 rounded-[10px] border bg-card p-3 text-card-foreground">
      <p className="m-0 flex items-baseline gap-2 text-xs">
        <span className="font-medium text-subtle">{messages.catalogue.label}</span>
        <span className="font-mono text-muted-foreground">{changeTitle(change, messages)}</span>
      </p>
      {files.length > 0 ? (
        <div className="flex min-w-0 flex-col gap-1.5">
          {files.map(([path, content]) => (
            <FileView key={path} path={path} content={content} />
          ))}
        </div>
      ) : null}
      {decided === null ? (
        <div className="flex flex-wrap items-center gap-2">
          <Button variant="outline" className={BUTTON_CLASS} disabled={status.state === 'pending'} onClick={() => decide('applied')}>
            {messages.catalogue.approve}
          </Button>
          <Button variant="ghost" className={BUTTON_CLASS} disabled={status.state === 'pending'} onClick={() => decide('refused')}>
            {messages.catalogue.refuse}
          </Button>
          <span aria-live="polite" className="text-xs text-muted-foreground">
            {status.state === 'failed' ? status.reason : null}
          </span>
        </div>
      ) : (
        <p aria-live="polite" className="m-0 text-xs font-medium text-subtle">
          {messages.catalogue.decision[decided]}
        </p>
      )}
    </div>
  );
}
