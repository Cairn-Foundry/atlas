// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useId, type ReactElement } from 'react';
import { cn } from 'cn';

/** Letter outlines on a 16-unit cap height, each in its own box starting at x = 0. Legs run past the baseline and are clipped flat. */
const LETTERS: ReadonlyArray<{ x: number; path: string; dotX?: number }> = [
  { x: 0, path: 'M-0.55 18 L6.5 2.7 L13.55 18', dotX: 6.5 },
  { x: 14.6, path: 'M0 1.2 H11 M5.5 1.2 V16' },
  { x: 28.4, path: 'M1.2 0 V14.8 H9' },
  { x: 38.2, path: 'M-0.55 18 L6.5 2.7 L13.55 18', dotX: 6.5 },
  {
    x: 54,
    path: 'M9.4 3.4 C8.6 1.8 7 1.2 5.1 1.2 C2.8 1.2 1.2 2.5 1.2 4.4 C1.2 6.5 3 7.3 5.2 8 C7.6 8.7 9.3 9.5 9.3 11.7 C9.3 13.7 7.5 14.8 5.2 14.8 C3.1 14.8 1.5 14.1 0.6 12.4',
  },
];
const WIDTH = 65.2;
const HEIGHT = 16;

interface AtlasLogoProps {
  /** Sets the height; the width follows. */
  className?: string;
}

/**
 * The Atlas wordmark: letters drawn stroke by stroke on arrival, the bar of each A replaced by a
 * dot, and a glint of light that crosses the word now and then.
 */
export function AtlasLogo({ className = 'h-4' }: AtlasLogoProps): ReactElement {
  const sheenId = useId();
  const fadeId = useId();
  return (
    <svg
      role="img"
      aria-label="Atlas"
      viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
      className={cn('atlas-logo w-auto flex-none text-foreground', className)}
      fill="none"
      stroke="currentColor"
      strokeWidth="2.4"
      strokeMiterlimit="8"
    >
      <defs>
        <linearGradient id={fadeId}>
          <stop offset="0" stopColor="#fff" stopOpacity="0" />
          <stop offset="0.5" stopColor="#fff" />
          <stop offset="1" stopColor="#fff" stopOpacity="0" />
        </linearGradient>
        <mask id={sheenId} maskUnits="userSpaceOnUse" x="0" y="0" width={WIDTH} height={HEIGHT}>
          <rect className="atlas-logo-sheen" x="-18" y="-4" width="18" height="24" fill={`url(#${fadeId})`} stroke="none" />
        </mask>
      </defs>
      {LETTERS.map((letter, index) => (
        <g key={letter.x} transform={`translate(${letter.x} 0)`}>
          <path className="atlas-logo-stroke" pathLength={1} d={letter.path} style={{ animationDelay: `${index * 110}ms` }} />
          {letter.dotX !== undefined && (
            <circle
              className="atlas-logo-dot"
              cx={letter.dotX}
              cy="12.2"
              r="1.55"
              stroke="none"
              style={{ animationDelay: `${700 + index * 110}ms` }}
            />
          )}
        </g>
      ))}
      <g mask={`url(#${sheenId})`} stroke="var(--status-working)">
        {LETTERS.map((letter) => (
          <path key={letter.x} transform={`translate(${letter.x} 0)`} d={letter.path} />
        ))}
      </g>
    </svg>
  );
}
