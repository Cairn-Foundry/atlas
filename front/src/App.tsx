// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useState, type ReactElement } from 'react';
import { BOARD_HASH, agentHash } from '@/lib/route';
import type { FilterKey } from '@/lib/filters';
import { useAtlasStream } from '@/hooks/use-atlas-stream';
import { useDelayedFlag } from '@/hooks/use-delayed-flag';
import { useHashRoute } from '@/hooks/use-hash-route';
import { useNow } from '@/hooks/use-now';
import { AppHeader } from '@/components/AppHeader';
import { Board } from '@/components/Board';
import { OrchestratorPanel } from '@/components/OrchestratorPanel';
import { SessionView } from '@/components/SessionView';

const OFFLINE_NOTICE_DELAY_MS = 1500;

export function App(): ReactElement {
  const { snapshot, isConnected, isOrchestratorBusy } = useAtlasStream();
  const route = useHashRoute();
  const now = useNow();
  const [filter, setFilter] = useState<FilterKey>('all');
  const isOffline = useDelayedFlag(!isConnected, OFFLINE_NOTICE_DELAY_MS);

  const openAgent = (agentId: string): void => {
    window.location.hash = agentHash(agentId);
  };
  const backToBoard = (): void => {
    window.location.hash = BOARD_HASH;
  };

  const agents = snapshot?.agents ?? [];
  const openedAgent = route.view === 'agent' ? agents.find((agent) => agent.id === route.agentId) : undefined;

  return (
    <div className="flex min-h-dvh flex-col lg:h-dvh lg:flex-row">
      <div className="flex min-w-0 flex-none flex-col lg:min-h-0 lg:flex-1">
        <AppHeader
          agents={agents}
          activeFilter={filter}
          isOffline={isOffline}
          showFilters={openedAgent === undefined}
          onFilterChange={setFilter}
        />
        {snapshot === null ? (
          <main className="board-dots min-w-0 flex-1 px-8 py-7 text-sm text-muted-foreground">Connexion au daemon...</main>
        ) : openedAgent === undefined ? (
          <Board snapshot={snapshot} filter={filter} now={now} onOpenAgent={openAgent} />
        ) : (
          <SessionView key={openedAgent.id} agent={openedAgent} agents={agents} now={now} onBack={backToBoard} onOpenAgent={openAgent} />
        )}
      </div>
      <OrchestratorPanel messages={snapshot?.messages ?? []} agents={agents} isBusy={isOrchestratorBusy} now={now} onOpenAgent={openAgent} />
    </div>
  );
}
