// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { ChatMessage } from '@/lib/contract';
import { OrchestratorPanel } from './OrchestratorPanel';

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

function message(id: string, text: string): ChatMessage {
  return { id, role: 'assistant', kind: 'text', text, card: null, actions: [], agentId: null, createdAt: '2026-11-23T00:15:00.000Z' };
}

function renderPanel(messages: ChatMessage[], isBusy = false) {
  return render(
    <OrchestratorPanel messages={messages} agents={[]} isBusy={isBusy} now={Date.parse('2026-11-23T00:16:00.000Z')} onOpenAgent={() => {}} />,
  );
}

describe('OrchestratorPanel', () => {
  // Regression: scrollIntoView also scrolled the window, so in the stacked layout every new
  // message moved the page down to the chat and the board went out of view.
  it('scrolls the thread to the new message without scrolling the page', () => {
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView = scrollIntoView;
    const scrollTo = vi.spyOn(window, 'scrollTo').mockImplementation(() => {});
    vi.spyOn(HTMLElement.prototype, 'scrollHeight', 'get').mockReturnValue(640);

    const { container, rerender } = renderPanel([message('m1', 'Bonjour')]);
    rerender(
      <OrchestratorPanel
        messages={[message('m1', 'Bonjour'), message('m2', 'Aucun projet pour l\'instant.')]}
        agents={[]}
        isBusy={false}
        now={Date.parse('2026-11-23T00:16:00.000Z')}
        onOpenAgent={() => {}}
      />,
    );

    const thread = container.querySelector<HTMLElement>('[aria-live="polite"]');
    expect(thread?.scrollTop).toBe(640);
    expect(scrollIntoView).not.toHaveBeenCalled();
    expect(scrollTo).not.toHaveBeenCalled();
  });

  it('shows the thinking indicator while busy, and the empty state otherwise', () => {
    const { rerender } = renderPanel([], true);
    expect(screen.getByText("L'orchestrateur réfléchit")).toBeTruthy();
    expect(screen.queryByText(/Nouvelle conversation\./)).toBeNull();
    rerender(<OrchestratorPanel messages={[]} agents={[]} isBusy={false} now={0} onOpenAgent={() => {}} />);
    expect(screen.queryByText("L'orchestrateur réfléchit")).toBeNull();
    expect(screen.getByText(/Nouvelle conversation\. Donnez un ticket/)).toBeTruthy();
  });

  it('starts a new conversation, disabled while busy', async () => {
    const fetchMock = vi.spyOn(globalThis, 'fetch').mockResolvedValue(new Response(null, { status: 204 }));
    const { rerender } = renderPanel([message('m1', 'Bonjour')], true);
    const button = screen.getByRole('button', { name: 'Nouvelle conversation' });
    expect((button as HTMLButtonElement).disabled).toBe(true);
    rerender(<OrchestratorPanel messages={[message('m1', 'Bonjour')]} agents={[]} isBusy={false} now={0} onOpenAgent={() => {}} />);
    fireEvent.click(screen.getByRole('button', { name: 'Nouvelle conversation' }));
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('http://daemon.test/api/orchestrator/reset', expect.objectContaining({ method: 'POST' })));
  });

  it('shows why the reset was refused', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValue(
      new Response(JSON.stringify({ error: "l'orchestrateur réfléchit encore" }), { status: 409 }),
    );
    renderPanel([message('m1', 'Bonjour')]);
    fireEvent.click(screen.getByRole('button', { name: 'Nouvelle conversation' }));
    expect(await screen.findByText("l'orchestrateur réfléchit encore")).toBeTruthy();
  });
});
