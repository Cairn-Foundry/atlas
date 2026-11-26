// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { ReactElement } from 'react';
import { PointCloud } from './PointCloud';

interface OrchestratorCoreProps {
  isBusy: boolean;
}

/** The orchestrator's emblem: a small sphere of points that turns slowly at rest and races and pulses while it thinks. */
export function OrchestratorCore({ isBusy }: OrchestratorCoreProps): ReactElement {
  return (
    <PointCloud
      shape="sphere"
      count={40}
      dotSize={0.8}
      spin={isBusy ? 2.6 : 0.35}
      waves={isBusy ? 'center' : 'none'}
      className="size-[22px] flex-none text-foreground [--cloud-accent:var(--status-working)]"
    />
  );
}
