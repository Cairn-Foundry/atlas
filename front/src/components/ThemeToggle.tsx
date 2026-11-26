// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { ReactElement } from 'react';
import { cn } from 'cn';
import { applyTheme } from '@/lib/theme';
import { useMessages } from '@/hooks/use-messages';
import { useTheme } from '@/hooks/use-theme';
import { StrokeIcon } from './StrokeIcon';

interface ThemeToggleProps {
  className?: string;
}

export function ThemeToggle({ className }: ThemeToggleProps): ReactElement {
  const theme = useTheme();
  const messages = useMessages();
  const isDark = theme === 'dark';
  return (
    <button
      type="button"
      aria-label={isDark ? messages.theme.switchToLight : messages.theme.switchToDark}
      title={isDark ? messages.theme.light : messages.theme.dark}
      className={cn(
        'flex size-10 flex-none items-center justify-center rounded-full text-subtle transition-colors hover:bg-secondary hover:text-foreground',
        className,
      )}
      onClick={() => applyTheme(isDark ? 'light' : 'dark')}
    >
      <StrokeIcon size={14} strokeWidth={1.4} strokeLinecap="round">
        {isDark ? (
          <>
            <circle cx="7" cy="7" r="2.6" />
            <path d="M7 1v1.4M7 11.6V13M1 7h1.4M11.6 7H13M2.8 2.8l1 1M10.2 10.2l1 1M2.8 11.2l1-1M10.2 3.8l1-1" />
          </>
        ) : (
          <path d="M11.8 8.6A5 5 0 0 1 5.4 2.2a5 5 0 1 0 6.4 6.4Z" />
        )}
      </StrokeIcon>
    </button>
  );
}
