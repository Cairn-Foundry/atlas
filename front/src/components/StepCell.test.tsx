// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { makeAgent, makeStep } from '@/test/fixtures';
import { StepCell } from './StepCell';

afterEach(cleanup);

const now = Date.parse('2026-11-22T21:00:00.000Z');
const renderStep = (step: ReturnType<typeof makeStep>) =>
  render(<StepCell agent={makeAgent({ steps: [step] })} step={step} now={now} isDimmed={false} onOpenAgent={vi.fn()} />);

describe('StepCell', () => {
  it('draws the agent card for the current step', () => {
    renderStep(makeStep());
    expect(screen.getByRole('button', { name: /PROJ-1, proj, Export CSV/ })).toBeTruthy();
  });

  it('shows a done step with its active duration and its tokens', () => {
    renderStep(
      makeStep({
        name: 'Développement',
        state: 'done',
        startedAt: '2026-11-22T20:00:00.000Z',
        endedAt: '2026-11-22T20:30:00.000Z',
        waitMs: 10 * 60_000,
        usage: { inputTokens: 100_000, outputTokens: 50_000, cacheReadTokens: 700_000, cacheWriteTokens: 0 },
      }),
    );
    expect(screen.getByText('Développement')).toBeTruthy();
    expect(screen.getByText('20 min')).toBeTruthy();
    expect(screen.getByText('420 k').textContent).toContain('tokens');
  });

  it('leaves the tokens out when the usage is unknown', () => {
    renderStep(makeStep({ state: 'done', startedAt: '2026-11-22T20:00:00.000Z', endedAt: '2026-11-22T20:30:00.000Z' }));
    expect(screen.queryByText(/tokens/)).toBeNull();
  });

  it('labels an external step "hors Atlas" without a duration', () => {
    renderStep(makeStep({ state: 'external', startedAt: '2026-11-22T20:00:00.000Z', endedAt: '2026-11-22T20:30:00.000Z' }));
    expect(screen.getByText('hors Atlas')).toBeTruthy();
    expect(screen.queryByText(/min/)).toBeNull();
  });

  it('puts the brick name in an upcoming slot', () => {
    renderStep(makeStep({ name: 'Relecture', state: 'upcoming' }));
    expect(screen.getByText('Relecture').className).not.toContain('line-through');
  });

  it('strikes a skipped step through but keeps it visible', () => {
    renderStep(makeStep({ name: 'Plan', state: 'skipped' }));
    expect(screen.getByText('Plan').className).toContain('line-through');
  });

  it('shows the pass of a repeated step', () => {
    renderStep(makeStep({ state: 'done', pass: 2 }));
    expect(screen.getByText('passe 2')).toBeTruthy();
  });
});
