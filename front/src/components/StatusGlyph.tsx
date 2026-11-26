// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { ReactElement } from 'react';
import type { AgentStatus } from '@/lib/contract';
import { cn } from 'cn';
import type { PointCloudOptions } from '@/lib/point-cloud-renderer';
import { PointCloud } from './PointCloud';
import { STATUS_TONE } from './status-tone';

type GlyphLook = Pick<PointCloudOptions, 'shape' | 'spin' | 'waves' | 'hasGlitch'>;

/** One shape per status, so the status reads without colour or motion: a turning globe, a radar, a cross, a check. */
const GLYPH_LOOK: Record<AgentStatus, GlyphLook> = {
  ['working']: { shape: 'sphere', spin: 1.5, waves: 'random', hasGlitch: false },
  ['waiting']: { shape: 'rings', spin: 0, waves: 'center', hasGlitch: false },
  ['blocked']: { shape: 'cross', spin: 0, waves: 'none', hasGlitch: true },
  ['done']: { shape: 'check', spin: 0, waves: 'none', hasGlitch: false },
};

interface StatusGlyphProps {
  status: AgentStatus;
  /** Sets the size; 24px when omitted. */
  className?: string;
}

/** The status of an agent as a small cloud of points that morphs when the status changes. */
export function StatusGlyph({ status, className = 'size-6' }: StatusGlyphProps): ReactElement {
  return <PointCloud {...GLYPH_LOOK[status]} count={44} dotSize={0.9} className={cn('shrink-0', STATUS_TONE[status].icon, className)} />;
}
