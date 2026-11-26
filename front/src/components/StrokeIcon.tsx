// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { ReactElement, ReactNode } from 'react';

interface StrokeIconProps {
  /** Width and height in pixels, and the side of the square the paths are drawn in. */
  size: number;
  strokeWidth: number;
  strokeLinecap?: 'round' | undefined;
  children: ReactNode;
}

/** A line icon in the text colour; decorative, so the button around it carries the label. */
export function StrokeIcon({ size, strokeWidth, strokeLinecap, children }: StrokeIconProps): ReactElement {
  return (
    <svg
      width={size}
      height={size}
      viewBox={`0 0 ${size} ${size}`}
      aria-hidden="true"
      fill="none"
      stroke="currentColor"
      strokeWidth={strokeWidth}
      strokeLinecap={strokeLinecap}
    >
      {children}
    </svg>
  );
}
