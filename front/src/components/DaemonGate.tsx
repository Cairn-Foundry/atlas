// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useCallback, useEffect, useState, type ReactElement, type ReactNode } from 'react';
import { connectToDaemon } from '@/lib/daemon-connection';
import { AtlasLogo } from '@/components/AtlasLogo';
import { useMessages } from '@/hooks/use-messages';
import { Button } from '@/components/ui/button';

type GateState = { status: 'connecting' } | { status: 'connected' } | { status: 'failed'; reason: string };

interface DaemonGateProps {
  /** Resolves once the daemon can be called; injected so the gate can be shown without a desktop shell. */
  connect?: () => Promise<void>;
  children: ReactNode;
}

/** Shows the factory only once the daemon answers; until then, the startup screen or why it failed. */
export function DaemonGate({ connect = connectToDaemon, children }: DaemonGateProps): ReactElement {
  const messages = useMessages();
  const [state, setState] = useState<GateState>({ status: 'connecting' });

  const start = useCallback((): void => {
    setState({ status: 'connecting' });
    connect().then(
      () => setState({ status: 'connected' }),
      (error: unknown) => setState({ status: 'failed', reason: error instanceof Error ? error.message : String(error) }),
    );
  }, [connect]);

  useEffect(start, [start]);

  if (state.status === 'connected') {
    return <>{children}</>;
  }
  return (
    <main className="grid min-h-dvh place-items-center p-6">
      <div className="grid max-w-md justify-items-center gap-3 text-center">
        <AtlasLogo className="size-14" />
        <h1 className="text-lg font-semibold">Atlas</h1>
        {state.status === 'connecting' ? (
          <p className="text-sm text-muted-foreground">{messages.gate.starting}</p>
        ) : (
          <>
            <p role="alert" className="text-sm whitespace-pre-wrap text-destructive">
              {state.reason}
            </p>
            <Button onClick={start}>{messages.gate.retry}</Button>
          </>
        )}
      </div>
    </main>
  );
}
