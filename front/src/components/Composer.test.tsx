// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { makeAgent } from '@/test/fixtures';
import { Composer } from './Composer';

const sendOrchestratorMessage = vi.fn(async () => {});
vi.mock('@/lib/api-client', () => ({ sendOrchestratorMessage: (...args: unknown[]) => sendOrchestratorMessage(...(args as [])) }));

const agents = [
  makeAgent({ id: 'a1', ticketRef: 'TOY-1', title: 'Ajouter une ligne' }),
  makeAgent({ id: 'a2', ticketRef: 'TOY-2', title: 'Ajouter NOTES.md' }),
];

afterEach(() => {
  cleanup();
  sendOrchestratorMessage.mockClear();
});

function type(textarea: HTMLTextAreaElement, value: string): void {
  fireEvent.change(textarea, { target: { value, selectionStart: value.length, selectionEnd: value.length } });
}

describe('Composer mentions', () => {
  it('suggests agents after @ and inserts the picked one with the keyboard', () => {
    render(<Composer agents={agents} />);
    const textarea = screen.getByRole('combobox') as HTMLTextAreaElement;
    type(textarea, 'Relance @TO');
    expect(textarea.getAttribute('aria-expanded')).toBe('true');
    expect(screen.getAllByRole('option').map((option) => option.textContent)).toEqual(['TOY-1Ajouter une ligne', 'TOY-2Ajouter NOTES.md']);

    fireEvent.keyDown(textarea, { key: 'ArrowDown' });
    expect(screen.getAllByRole('option')[1]?.getAttribute('aria-selected')).toBe('true');
    fireEvent.keyDown(textarea, { key: 'Enter' });
    expect(textarea.value).toBe('Relance @TOY-2 ');
    expect(screen.queryByRole('listbox')).toBeNull();
    expect(sendOrchestratorMessage).not.toHaveBeenCalled();
  });

  it('closes the list on Escape and sends the mentioned agent ids', async () => {
    render(<Composer agents={agents} />);
    const textarea = screen.getByRole('combobox') as HTMLTextAreaElement;
    type(textarea, '@TOY-1 et @TO');
    fireEvent.keyDown(textarea, { key: 'Escape' });
    expect(screen.queryByRole('listbox')).toBeNull();
    fireEvent.keyDown(textarea, { key: 'Enter' });
    await vi.waitFor(() =>
      expect(sendOrchestratorMessage).toHaveBeenCalledWith({ text: '@TOY-1 et @TO', broadcast: false, mentions: ['a1'] }),
    );
  });
});
