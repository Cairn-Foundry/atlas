// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { currentLanguage, showLanguage } from '@/lib/i18n';
import { LanguageToggle } from './LanguageToggle';

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe('LanguageToggle', () => {
  it('shows the language in use, and names the other one in its own words', () => {
    render(<LanguageToggle />);
    const button = screen.getByRole('button', { name: 'Switch to English' });
    expect(button.textContent).toBe('FR');
    expect(button.getAttribute('title')).toBe('Switch to English');
    act(() => showLanguage('en'));
    const frenchButton = screen.getByRole('button', { name: 'Passer en français' });
    expect(frenchButton.textContent).toBe('EN');
  });

  it('asks the daemon, and shows the new language at once', async () => {
    const fetchMock = vi.spyOn(globalThis, 'fetch').mockResolvedValue(new Response(null, { status: 204 }));
    render(<LanguageToggle />);
    fireEvent.click(screen.getByRole('button', { name: 'Switch to English' }));
    expect(currentLanguage()).toBe('en');
    expect(document.documentElement.lang).toBe('en');
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1));
    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(url).toBe('http://daemon.test/api/language');
    expect(init.method).toBe('POST');
    expect(init.body).toBe('{"language":"en"}');
  });

  it('puts the previous language back when the daemon refuses', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValue(new Response(JSON.stringify({ error: 'nope' }), { status: 500 }));
    render(<LanguageToggle />);
    fireEvent.click(screen.getByRole('button', { name: 'Switch to English' }));
    await waitFor(() => expect(currentLanguage()).toBe('fr'));
    expect(screen.getByRole('button', { name: 'Switch to English' })).toBeTruthy();
  });
});
