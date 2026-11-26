// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { act, cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { showLanguage } from '@/lib/i18n';
import { OrchestratorPanel } from './OrchestratorPanel';

afterEach(cleanup);

describe('a language change', () => {
  it('rewrites the texts in place, without remounting the tree', () => {
    showLanguage('en');
    render(<OrchestratorPanel messages={[]} agents={[]} isBusy={false} now={0} onOpenAgent={() => {}} />);
    const panel = screen.getByRole('complementary', { name: 'Orchestrator' });
    const textarea = screen.getByRole('combobox');
    expect(textarea.getAttribute('placeholder')).toBe('Write to the orchestrator, @ for an agent');
    expect(screen.getByText(/New conversation\. Give a ticket/)).toBeTruthy();

    act(() => showLanguage('fr'));

    expect(screen.getByRole('complementary', { name: 'Orchestrateur' })).toBe(panel);
    expect(screen.getByRole('combobox')).toBe(textarea);
    expect(textarea.getAttribute('placeholder')).toBe("Écrire à l'orchestrateur, @ pour un agent");
    expect(screen.getByText(/Nouvelle conversation\. Donnez un ticket/)).toBeTruthy();
  });
});
