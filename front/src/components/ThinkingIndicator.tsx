// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useState, type ReactElement } from 'react';
import { formatThinkingSeconds } from '@/lib/time';
import { useNow } from '@/hooks/use-now';
import { useMessages } from '@/hooks/use-messages';
import { OrchestratorCore } from './OrchestratorCore';

/** Shown where the orchestrator's reply will appear while it thinks: its orb, busy, and the time spent. */
export function ThinkingIndicator(): ReactElement {
  const messages = useMessages();
  const [startedAt] = useState(() => Date.now());
  const now = useNow(1000);
  const seconds = formatThinkingSeconds(now - startedAt);

  return (
    <div className="flex items-center gap-2.5 text-sm animate-in fade-in slide-in-from-bottom-1 duration-300 motion-reduce:animate-none">
      <OrchestratorCore isBusy />
      <span aria-hidden="true" className="atlas-shimmer font-medium">
        {messages.orchestrator.thinking}
      </span>
      {seconds && <span className="text-xs text-muted-foreground tabular-nums">{seconds}</span>}
      <span className="sr-only">{messages.orchestrator.thinkingSpoken}</span>
    </div>
  );
}
