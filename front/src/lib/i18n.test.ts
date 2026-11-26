// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { currentLanguage, messages, messagesFor, readLanguage, showLanguage, subscribeLanguage } from './i18n';

describe('readLanguage', () => {
  beforeEach(() => window.localStorage.clear());
  afterEach(() => vi.restoreAllMocks());

  it('gives the language the daemon last spoke', () => {
    window.localStorage.setItem('atlas-language', 'fr');
    vi.spyOn(navigator, 'language', 'get').mockReturnValue('en-US');
    expect(readLanguage()).toBe('fr');
  });

  it('falls back to the system language: French for any fr-*, English for the rest', () => {
    const language = vi.spyOn(navigator, 'language', 'get');
    language.mockReturnValue('fr-CA');
    expect(readLanguage()).toBe('fr');
    language.mockReturnValue('de-DE');
    expect(readLanguage()).toBe('en');
  });

  it('ignores a stored value that is not a language', () => {
    window.localStorage.setItem('atlas-language', 'klingon');
    vi.spyOn(navigator, 'language', 'get').mockReturnValue('fr-FR');
    expect(readLanguage()).toBe('fr');
  });

  it('survives blocked storage', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    vi.spyOn(navigator, 'language', 'get').mockReturnValue('fr-FR');
    expect(readLanguage()).toBe('fr');
  });
});

describe('showLanguage', () => {
  afterEach(() => vi.restoreAllMocks());

  it('sets <html lang>, remembers the language and serves its dictionary', () => {
    showLanguage('en');
    expect(document.documentElement.lang).toBe('en');
    expect(window.localStorage.getItem('atlas-language')).toBe('en');
    expect(currentLanguage()).toBe('en');
    expect(messages()).toBe(messagesFor('en'));
    expect(messages().locale).toBe('en-US');
  });

  it('notifies subscribers only when the language changed', () => {
    showLanguage('fr');
    const listener = vi.fn();
    const unsubscribe = subscribeLanguage(listener);
    showLanguage('fr');
    expect(listener).not.toHaveBeenCalled();
    showLanguage('en');
    expect(listener).toHaveBeenCalledTimes(1);
    unsubscribe();
    showLanguage('fr');
    expect(listener).toHaveBeenCalledTimes(1);
  });

  it('still shows the language when storage is blocked', () => {
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    showLanguage('en');
    expect(document.documentElement.lang).toBe('en');
  });
});
