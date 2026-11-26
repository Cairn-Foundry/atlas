// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { ReactElement } from 'react';
import type { AgentView } from '@/lib/contract';
import type { FilterKey } from '@/lib/filters';
import { useMessages } from '@/hooks/use-messages';
import { AtlasLogo } from './AtlasLogo';
import { FilterPills } from './FilterPills';
import { LanguageToggle } from './LanguageToggle';
import { ThemeToggle } from './ThemeToggle';

interface AppHeaderProps {
  agents: readonly AgentView[];
  activeFilter: FilterKey;
  isOffline: boolean;
  showFilters: boolean;
  onFilterChange: (filter: FilterKey) => void;
}

export function AppHeader({ agents, activeFilter, isOffline, showFilters, onFilterChange }: AppHeaderProps): ReactElement {
  const messages = useMessages();
  return (
    <header className="flex min-h-14 flex-none flex-wrap items-center justify-between gap-x-3 gap-y-2 border-b border-border px-4 py-2 sm:px-8 lg:h-14 lg:pr-4 lg:flex-nowrap lg:py-0">
      {/* Below lg the filters wrap to their own row: the theme toggle stays on the logo's. */}
      <div className="flex min-w-fit flex-1 items-center gap-2.5">
        <AtlasLogo />
        <LanguageToggle />
        {isOffline && (
          <span role="status" className="text-[13px] text-blocked-text">
            {messages.board.offline}
          </span>
        )}
        <ThemeToggle className="ml-auto lg:hidden" />
      </div>
      {showFilters && <FilterPills agents={agents} activeFilter={activeFilter} onFilterChange={onFilterChange} />}
      <ThemeToggle className="hidden lg:flex" />
    </header>
  );
}
