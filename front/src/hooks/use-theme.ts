// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useSyncExternalStore } from 'react';
import { currentTheme, subscribeTheme, type Theme } from '@/lib/theme';

export function useTheme(): Theme {
  return useSyncExternalStore(subscribeTheme, currentTheme, () => 'light');
}
