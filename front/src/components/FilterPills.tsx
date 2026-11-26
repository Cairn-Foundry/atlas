// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { ReactElement } from 'react';
import type { AgentView } from '@/lib/contract';
import { cn } from 'cn';
import { buildFilterOptions, visibleCountLabel, type FilterKey } from '@/lib/filters';
import { useMessages } from '@/hooks/use-messages';
import { STATUS_TONE } from './status-tone';

interface FilterPillsProps {
  agents: readonly AgentView[];
  activeFilter: FilterKey;
  onFilterChange: (filter: FilterKey) => void;
}

export function FilterPills({ agents, activeFilter, onFilterChange }: FilterPillsProps): ReactElement {
  const messages = useMessages();
  const options = buildFilterOptions(agents, messages);
  return (
    <div className="-mx-4 min-w-0 max-w-[calc(100%+2rem)] overflow-x-auto px-4 sm:mx-0 sm:max-w-full sm:px-0">
      <div role="group" aria-label={messages.filters.groupLabel} className="flex items-center gap-1 xl:gap-1.5">
        {options.map((option) => {
          const isActive = option.key === activeFilter;
          return (
            <button
              key={option.key}
              type="button"
              aria-pressed={isActive}
              onClick={() => onFilterChange(option.key)}
              className={cn(
                'flex h-10 flex-none items-center gap-2 whitespace-nowrap rounded-full border px-3 text-[13px] font-medium transition-colors xl:px-3.5',
                isActive ? 'border-primary bg-primary text-primary-foreground' : 'border-border bg-background hover:bg-secondary',
              )}
            >
              {option.key !== 'all' && (
                <span
                  aria-hidden="true"
                  // The done dot is the colour of the active pill: it turns to the pill's text colour to stay visible.
                  className={cn('size-[7px] rounded-full', isActive && option.key === 'done' ? 'bg-primary-foreground' : STATUS_TONE[option.key].dot)}
                />
              )}
              {option.label}
              <span className={cn('font-normal', isActive ? 'text-primary-foreground/75' : 'text-muted-foreground')}>{option.count}</span>
            </button>
          );
        })}
      </div>
      <p role="status" className="sr-only">
        {visibleCountLabel(agents, activeFilter, messages)}
      </p>
    </div>
  );
}
