// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { makeAgent, makeStep } from '@/test/fixtures';
import { BoardRow } from './BoardRow';

afterEach(cleanup);

const now = Date.parse('2026-11-22T21:00:00.000Z');
const renderRow = (agent: ReturnType<typeof makeAgent>) => render(<BoardRow agent={agent} filter="all" now={now} onOpenAgent={vi.fn()} />);

describe('BoardRow', () => {
  it('ends the row of a finished agent with its card, after the last step', () => {
    const steps = [makeStep({ id: 's1', name: 'Plan', state: 'done' }), makeStep({ id: 's2', name: 'Revue', state: 'done' })];
    renderRow(makeAgent({ status: 'done', steps, mrUrl: 'https://git.example/mr/1' }));
    const card = screen.getByRole('button', { name: /PROJ-1, proj, Export CSV, terminé/ });
    const lastStep = screen.getByText('Revue');
    expect(lastStep.compareDocumentPosition(card) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('draws the card once, on the current step, when there is one', () => {
    renderRow(makeAgent());
    expect(screen.getAllByRole('button', { name: /PROJ-1/ })).toHaveLength(1);
  });

  it('shows the other projects as tags next to the project name', () => {
    renderRow(makeAgent({ extraRepos: ['api', 'docs'] }));
    const heading = screen.getByRole('heading');
    expect(heading.textContent).toBe('PROJ-1projapidocs');
  });
});
