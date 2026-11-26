// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { Language } from '@/lib/contract';
import { en } from './messages/en';
import { fr, type Messages } from './messages/fr';

const LANGUAGE_STORAGE_KEY = 'atlas-language';

/** The last language the daemon spoke, else the system's; storage may be blocked. Used before the daemon answers. */
export function readLanguage(): Language {
  try {
    const stored = window.localStorage.getItem(LANGUAGE_STORAGE_KEY);
    if (stored === 'fr' || stored === 'en') return stored;
  } catch {
    // Storage blocked: fall back to the system language.
  }
  return navigator.language.toLowerCase().startsWith('fr') ? 'fr' : 'en';
}

let current: Language | null = null;
const listeners = new Set<() => void>();

export function currentLanguage(): Language {
  current ??= readLanguage();
  return current;
}

export function subscribeLanguage(onChange: () => void): () => void {
  listeners.add(onChange);
  return () => listeners.delete(onChange);
}

/** Makes `language` the one the page speaks and remembers it for the next startup screen. */
export function showLanguage(language: Language): void {
  const hasChanged = language !== currentLanguage();
  current = language;
  document.documentElement.lang = language;
  try {
    window.localStorage.setItem(LANGUAGE_STORAGE_KEY, language);
  } catch {
    // Storage blocked: the startup screen falls back to the system language.
  }
  if (hasChanged) for (const listener of listeners) listener();
}

export function messagesFor(language: Language): Messages {
  return language === 'fr' ? fr : en;
}

/** The dictionary of the current language, for code that is not a component. */
export function messages(): Messages {
  return messagesFor(currentLanguage());
}
