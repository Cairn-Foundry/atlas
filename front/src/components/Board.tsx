// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { ReactElement } from 'react';
import type { Snapshot } from '@/lib/contract';
import { orderRows } from '@/lib/board';
import type { FilterKey } from '@/lib/filters';
import { useMessages } from '@/hooks/use-messages';
import { useMediaQuery } from '@/hooks/use-media-query';
import { BoardCanvas } from './BoardCanvas';
import { BoardRow } from './BoardRow';

interface BoardProps {
  snapshot: Snapshot;
  filter: FilterKey;
  now: number;
  onOpenAgent: (agentId: string) => void;
}

export function Board({ snapshot, filter, now, onOpenAgent }: BoardProps): ReactElement {
  const messages = useMessages();
  const isWide = useMediaQuery('(min-width: 768px)');
  const agents = orderRows(snapshot.agents);

  if (isWide) {
    return (
      <main className="min-w-0 flex-1 md:h-[calc(100dvh-3.5rem)] md:flex-none lg:h-auto lg:min-h-0 lg:flex-1">
        <BoardCanvas agents={agents} filter={filter} now={now} onOpenAgent={onOpenAgent} />
      </main>
    );
  }

  return (
    <main className="board-dots min-w-0 flex-1 px-4 pb-8 pt-6">
      <div className="flex flex-col gap-6">
        {agents.length === 0 ? (
          <p className="m-0 text-sm text-muted-foreground">{messages.board.empty}</p>
        ) : (
          agents.map((agent) => <BoardRow key={agent.id} agent={agent} filter={filter} now={now} onOpenAgent={onOpenAgent} />)
        )}
      </div>
    </main>
  );
}
