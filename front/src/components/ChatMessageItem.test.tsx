// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { ChatMessage } from '@/lib/contract';
import { makeAgent } from '@/test/fixtures';
import { ChatMessageItem } from './ChatMessageItem';

afterEach(cleanup);

const agent = makeAgent({ id: 'a1', ticketRef: 'TOY-1' });

describe('ChatMessageItem', () => {
  it('renders a known @mention as a button that opens the agent session', () => {
    const onOpenAgent = vi.fn();
    render(
      <ChatMessageItem
        message={{ id: 'm1', role: 'user', kind: 'text', text: 'Relance @TOY-1 et @TOY-9', card: null, actions: [], agentId: null, createdAt: '2026-11-22T20:00:00.000Z' }}
        now={Date.parse('2026-11-22T20:05:00.000Z')}
        agents={[agent]}
        onOpenAgent={onOpenAgent}
      />,
    );
    const mention = screen.getByRole('button', { name: '@TOY-1' });
    fireEvent.click(mention);
    expect(onOpenAgent).toHaveBeenCalledWith('a1');
    expect(screen.queryByRole('button', { name: '@TOY-9' })).toBeNull();
    expect(mention.parentElement?.textContent).toBe('Relance @TOY-1 et @TOY-9');
  });

  it('copies the raw text of a message, Markdown included', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('navigator', { clipboard: { writeText } });
    render(
      <ChatMessageItem
        message={{ id: 'm3', role: 'assistant', kind: 'text', text: 'Agent **lancé**.', card: null, actions: [], agentId: null, createdAt: '2026-11-22T20:00:00.000Z' }}
        now={Date.parse('2026-11-22T20:05:00.000Z')}
        agents={[]}
        onOpenAgent={() => {}}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Copier' }));
    expect(writeText).toHaveBeenCalledWith('Agent **lancé**.');
    expect(await screen.findByRole('button', { name: 'Copié' })).toBeTruthy();
    vi.unstubAllGlobals();
  });

  it('shows when the message was sent', () => {
    const sent = new Date(2026, 10, 22, 21, 42);
    render(
      <ChatMessageItem
        message={{ id: 'm2', role: 'assistant', kind: 'text', text: 'Agent lancé.', card: null, actions: [], agentId: null, createdAt: sent.toISOString() }}
        now={new Date(2026, 10, 22, 23, 0).getTime()}
        agents={[]}
        onOpenAgent={() => {}}
      />,
    );
    const time = screen.getByText('21:42');
    expect(time.tagName).toBe('TIME');
    expect(time.getAttribute('datetime')).toBe(sent.toISOString());
  });

  describe('card messages', () => {
    const decision: ChatMessage = {
      id: 'm4',
      role: 'assistant',
      kind: 'decision',
      text: 'Qualification validée, le plan est inutile.',
      card: {
        ticketRef: 'TOY-1',
        steps: [
          { name: 'Qualification', state: 'done' },
          { name: 'Plan', state: 'skipped' },
          { name: 'Développement', state: 'current' },
        ],
      },
      actions: [{ type: 'session' }],
      agentId: 'a1',
      createdAt: '2026-11-22T20:00:00.000Z',
    };
    const renderCard = (message: ChatMessage, onOpenAgent = vi.fn()) =>
      render(<ChatMessageItem message={message} now={Date.parse('2026-11-22T20:05:00.000Z')} agents={[agent]} onOpenAgent={onOpenAgent} />);

    it('shows the label, the ticket, the reason and one chip per step', () => {
      renderCard(decision);
      expect(screen.getByText('Décision')).toBeTruthy();
      expect(screen.getByText('TOY-1')).toBeTruthy();
      expect(screen.getByText('Qualification validée, le plan est inutile.')).toBeTruthy();
      expect(screen.getAllByRole('listitem').map((chip) => chip.textContent)).toEqual([
        'Qualification (terminée)',
        'Plan (retirée)',
        'Développement (en cours)',
      ]);
    });

    it('strikes a skipped step through', () => {
      renderCard(decision);
      const [done, skipped] = screen.getAllByRole('listitem');
      expect(skipped?.className).toContain('line-through');
      expect(done?.className).not.toContain('line-through');
    });

    it('labels each kind of card', () => {
      renderCard({ ...decision, kind: 'launch' });
      expect(screen.getByText('Lancement')).toBeTruthy();
      cleanup();
      renderCard({ ...decision, kind: 'workflow' });
      expect(screen.getByText('Parcours modifié')).toBeTruthy();
    });

    it('keeps the copy button and the session shortcut', () => {
      const onOpenAgent = vi.fn();
      renderCard(decision, onOpenAgent);
      expect(screen.getByRole('button', { name: 'Copier' })).toBeTruthy();
      fireEvent.click(screen.getByRole('button', { name: 'Ouvrir la session' }));
      expect(onOpenAgent).toHaveBeenCalledWith('a1');
    });

    it('renders a text message as plain text even when it carries a card', () => {
      renderCard({ ...decision, kind: 'text' });
      expect(screen.queryByText('Décision')).toBeNull();
      expect(screen.queryAllByRole('listitem')).toEqual([]);
    });
  });

  describe('actions', () => {
    const base: ChatMessage = {
      id: 'm5',
      role: 'assistant',
      kind: 'text',
      text: 'Fini.',
      card: null,
      actions: [],
      agentId: 'a1',
      createdAt: '2026-11-22T20:00:00.000Z',
    };
    const renderActions = (actions: ChatMessage['actions'], onOpenAgent = vi.fn()) =>
      render(<ChatMessageItem message={{ ...base, actions }} now={Date.parse('2026-11-22T20:05:00.000Z')} agents={[agent]} onOpenAgent={onOpenAgent} />);

    it('offers no button without actions, even for a message about an agent', () => {
      renderActions([]);
      expect(screen.queryByRole('button', { name: 'Ouvrir la session' })).toBeNull();
      expect(screen.queryByRole('link')).toBeNull();
    });

    it('renders the buttons in the order given, with their labels', () => {
      renderActions([{ type: 'link', label: 'Voir la MR', url: 'https://git.example/mr/1' }, { type: 'archive' }, { type: 'resume' }, { type: 'session' }]);
      const names = [...screen.getAllByRole('link'), ...screen.getAllByRole('button')]
        .filter((element) => element.textContent !== 'Copier')
        .sort((a, b) => (a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING ? -1 : 1))
        .map((element) => element.textContent);
      expect(names).toEqual(['Voir la MR', 'Archiver', 'Rouvrir la session', 'Ouvrir la session']);
    });

    it('opens a link in a new tab', () => {
      renderActions([{ type: 'link', label: 'Voir la MR', url: 'https://git.example/mr/1' }]);
      const link = screen.getByRole('link', { name: 'Voir la MR' });
      expect(link.getAttribute('href')).toBe('https://git.example/mr/1');
      expect(link.getAttribute('target')).toBe('_blank');
      expect(link.getAttribute('rel')).toBe('noreferrer');
    });

    it('opens the session of the agent', () => {
      const onOpenAgent = vi.fn();
      renderActions([{ type: 'session' }], onOpenAgent);
      fireEvent.click(screen.getByRole('button', { name: 'Ouvrir la session' }));
      expect(onOpenAgent).toHaveBeenCalledWith('a1');
    });

    it('resumes the agent through the API', () => {
      const fetchMock = vi.fn().mockResolvedValue({ ok: true, status: 204 });
      vi.stubGlobal('fetch', fetchMock);
      renderActions([{ type: 'resume' }]);
      fireEvent.click(screen.getByRole('button', { name: 'Rouvrir la session' }));
      expect(fetchMock).toHaveBeenCalledWith('http://daemon.test/api/agents/a1/resume', expect.objectContaining({ method: 'POST' }));
      vi.unstubAllGlobals();
    });

    it('keeps a proposed note in the memory of its project, then disables the button', async () => {
      const fetchMock = vi.fn().mockResolvedValue({ ok: true, status: 204 });
      vi.stubGlobal('fetch', fetchMock);
      renderActions([{ type: 'save-note', repo: 'shop', note: 'API GitLab : gitlab.example.com' }]);
      fireEvent.click(screen.getByRole('button', { name: 'Retenir pour le projet' }));
      expect(fetchMock).toHaveBeenCalledWith(
        'http://daemon.test/api/repos/shop/notes',
        expect.objectContaining({ method: 'POST', body: JSON.stringify({ note: 'API GitLab : gitlab.example.com' }) }),
      );
      await waitFor(() => expect((screen.getByRole('button', { name: 'Retenir pour le projet' }) as HTMLButtonElement).disabled).toBe(true));
      vi.unstubAllGlobals();
    });

    it('keeps a note meant for every project in the common memory', () => {
      const fetchMock = vi.fn().mockResolvedValue({ ok: true, status: 204 });
      vi.stubGlobal('fetch', fetchMock);
      renderActions([{ type: 'save-note', repo: null, note: 'Assigner le maître aux MR' }]);
      fireEvent.click(screen.getByRole('button', { name: 'Retenir pour tous les projets' }));
      expect(fetchMock).toHaveBeenCalledWith('http://daemon.test/api/notes', expect.objectContaining({ method: 'POST' }));
      vi.unstubAllGlobals();
    });

    it('shows the files a catalogue change writes and applies it once validated', async () => {
      const fetchMock = vi.fn().mockResolvedValue({ ok: true, status: 204 });
      vi.stubGlobal('fetch', fetchMock);
      const change = { type: 'skill' as const, name: 'house-style', files: { 'SKILL.md': 'Style maison', 'references/rules.md': 'Règles' } };
      renderActions([{ type: 'apply-catalogue-change', change, decision: null }]);
      expect(screen.getByText('Skill house-style')).toBeTruthy();
      expect(screen.getByText('skills/house-style/references/rules.md')).toBeTruthy();
      expect(fetchMock).not.toHaveBeenCalled();
      fireEvent.click(screen.getByRole('button', { name: 'Valider' }));
      expect(fetchMock).toHaveBeenCalledWith(
        `http://daemon.test/api/messages/${base.id}/catalogue-change`,
        expect.objectContaining({ method: 'POST', body: JSON.stringify({ decision: 'applied' }) }),
      );
      expect(await screen.findByText('Appliqué')).toBeTruthy();
      expect(screen.queryByRole('button', { name: 'Valider' })).toBeNull();
      vi.unstubAllGlobals();
    });

    it('refuses a catalogue change without writing it', async () => {
      const fetchMock = vi.fn().mockResolvedValue({ ok: true, status: 204 });
      vi.stubGlobal('fetch', fetchMock);
      renderActions([{ type: 'apply-catalogue-change', change: { type: 'disable', id: 'rapport' }, decision: null }]);
      fireEvent.click(screen.getByRole('button', { name: 'Refuser' }));
      expect(fetchMock).toHaveBeenCalledWith(
        `http://daemon.test/api/messages/${base.id}/catalogue-change`,
        expect.objectContaining({ body: JSON.stringify({ decision: 'refused' }) }),
      );
      expect(await screen.findByText('Refusé')).toBeTruthy();
      vi.unstubAllGlobals();
    });

    it('shows a decision the daemon kept, with no button left', () => {
      renderActions([{ type: 'apply-catalogue-change', change: { type: 'enable', id: 'rapport' }, decision: 'refused' }]);
      expect(screen.getByText('Refusé')).toBeTruthy();
      expect(screen.queryByRole('button', { name: 'Valider' })).toBeNull();
      expect(screen.queryByRole('button', { name: 'Refuser' })).toBeNull();
    });

    it('says in place why the daemon refused a catalogue change', async () => {
      const refusal = 'audit : skills cite un skill absent de skills/ (house-style)';
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: false, status: 422, json: () => Promise.resolve({ error: refusal }) }));
      renderActions([{ type: 'apply-catalogue-change', change: { type: 'disable', id: 'rapport' }, decision: null }]);
      fireEvent.click(screen.getByRole('button', { name: 'Valider' }));
      expect(await screen.findByText(refusal)).toBeTruthy();
      expect((screen.getByRole('button', { name: 'Valider' }) as HTMLButtonElement).disabled).toBe(false);
      vi.unstubAllGlobals();
    });

    it('archives the agent, then disables the button', async () => {
      const fetchMock = vi.fn().mockResolvedValue({ ok: true, status: 204 });
      vi.stubGlobal('fetch', fetchMock);
      renderActions([{ type: 'archive' }]);
      fireEvent.click(screen.getByRole('button', { name: 'Archiver' }));
      expect(fetchMock).toHaveBeenCalledWith('http://daemon.test/api/agents/a1/archive', expect.objectContaining({ method: 'POST' }));
      await waitFor(() => expect((screen.getByRole('button', { name: 'Archiver' }) as HTMLButtonElement).disabled).toBe(true));
      vi.unstubAllGlobals();
    });

    it('shows a failure in place and lets the user retry', async () => {
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: false, status: 500 }));
      renderActions([{ type: 'resume' }]);
      fireEvent.click(screen.getByRole('button', { name: 'Rouvrir la session' }));
      const retry = await screen.findByRole('button', { name: 'Échec, réessayer' });
      expect((retry as HTMLButtonElement).disabled).toBe(false);
      vi.unstubAllGlobals();
    });

    it('applies to plain messages and cards alike', () => {
      renderActions([{ type: 'resume' }]);
      expect(screen.getByRole('button', { name: 'Rouvrir la session' })).toBeTruthy();
      cleanup();
      render(
        <ChatMessageItem
          message={{ ...base, kind: 'launch', card: { ticketRef: 'TOY-1', steps: [] }, actions: [{ type: 'resume' }] }}
          now={0}
          agents={[agent]}
          onOpenAgent={vi.fn()}
        />,
      );
      expect(screen.getByRole('button', { name: 'Rouvrir la session' })).toBeTruthy();
    });
  });
});
