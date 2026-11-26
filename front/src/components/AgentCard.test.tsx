// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { AgentStatus } from '@/lib/contract';
import { makeAgent, makeStep } from '@/test/fixtures';
import { AgentCard } from './AgentCard';

const now = Date.parse('2026-11-22T20:21:00.000Z');

afterEach(cleanup);

describe('AgentCard', () => {
  it('describes a waiting agent and its wait time', () => {
    const agent = makeAgent({ status: 'waiting', waitingSince: '2026-11-22T20:00:00.000Z' });
    render(<AgentCard agent={agent} now={now} isDimmed={false} onOpen={vi.fn()} />);
    const card = screen.getByRole('button', { name: 'PROJ-1, proj, Export CSV, Qualification, vous attend, 21 min' });
    expect(card.textContent).toContain('proj');
    expect(card.textContent).toContain('Vous attend');
    expect(card.textContent).toContain('21 min');
  });

  it('puts the blocking reason in the accessible name', () => {
    const agent = makeAgent({ status: 'blocked', statusReason: 'Seuil dépassé' });
    render(<AgentCard agent={agent} now={now} isDimmed={false} onOpen={vi.fn()} />);
    expect(screen.getByRole('button').getAttribute('aria-label')).toBe('PROJ-1, proj, Export CSV, Qualification, bloqué, 21 min, Seuil dépassé');
  });

  it('opens the agent on click', () => {
    const onOpen = vi.fn();
    render(<AgentCard agent={makeAgent()} now={now} isDimmed={false} onOpen={onOpen} />);
    fireEvent.click(screen.getByRole('button'));
    expect(onOpen).toHaveBeenCalledWith('agent-1');
  });

  it('takes a filtered-out card out of the keyboard and screen reader path', () => {
    render(<AgentCard agent={makeAgent()} now={now} isDimmed onOpen={vi.fn()} />);
    expect(screen.queryByRole('button')).toBeNull();
    const card = screen.getByRole('button', { hidden: true });
    expect(card.getAttribute('aria-hidden')).toBe('true');
    expect(card.tabIndex).toBe(-1);
  });

  it('shows the pass of a repeated review', () => {
    render(<AgentCard agent={makeAgent({ steps: [makeStep({ pass: 2 })] })} now={now} isDimmed={false} onOpen={vi.fn()} />);
    expect(screen.getByRole('button').textContent).toContain('passe 2');
  });

  it('draws a finished agent distinctly, without a step', () => {
    render(<AgentCard agent={makeAgent({ status: 'done', steps: [makeStep({ state: 'done' })] })} now={now} isDimmed={false} onOpen={vi.fn()} />);
    const card = screen.getByRole('button', { name: 'PROJ-1, proj, Export CSV, terminé' });
    expect(card.getAttribute('data-status')).toBe('done');
    expect(card.textContent).toContain('Terminé');
    expect(card.className).toContain('border-foreground/30');
  });

  it('gives each status its own border', () => {
    const statuses: AgentStatus[] = ['working', 'waiting', 'blocked', 'done'];
    const borders = statuses.map((status) => {
      const { unmount } = render(<AgentCard agent={makeAgent({ status })} now={now} isDimmed={false} onOpen={vi.fn()} />);
      const border = screen.getByRole('button', { name: /PROJ-1/ }).className.split(' ').find((name) => name.startsWith('border-'));
      unmount();
      return border;
    });
    expect(new Set(borders).size).toBe(4);
  });
});
