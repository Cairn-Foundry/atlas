// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useSyncExternalStore } from 'react';
import type { Language } from '@/lib/contract';
import { currentLanguage, messagesFor, subscribeLanguage } from '@/lib/i18n';
import type { Messages } from '@/lib/messages/fr';

export function useLanguage(): Language {
  return useSyncExternalStore(subscribeLanguage, currentLanguage);
}

/** The dictionary of the language shown; a component that renders text calls it so it re-renders on a change. */
export function useMessages(): Messages {
  return messagesFor(useLanguage());
}
