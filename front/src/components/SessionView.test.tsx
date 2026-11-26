// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { afterEach, describe, expect, it } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import type { AgentView } from '@/lib/contract';
import { makeAgent } from '@/test/fixtures';
import { SessionView } from './SessionView';

afterEach(cleanup);

const report = { stepName: 'Rapport', markdown: '### Conclusion\n\nAucun finding bloquant.', writtenAt: '2026-11-22T21:00:00.000Z' };

function renderView(agent: AgentView) {
  return render(<SessionView agent={agent} agents={[agent]} now={Date.parse('2026-11-22T21:05:00.000Z')} onBack={() => {}} onOpenAgent={() => {}} />);
}

describe('SessionView', () => {
  it('opens on the report once the session is closed', () => {
    renderView(makeAgent({ liveSessionId: null, report }));
    expect(screen.getByRole('tab', { name: 'Rapport' }).getAttribute('aria-selected')).toBe('true');
    expect(screen.getByRole('heading', { name: 'Conclusion' })).toBeDefined();
  });

  it('switches to the terminal and back', () => {
    renderView(makeAgent({ liveSessionId: null, report }));
    fireEvent.click(screen.getByRole('tab', { name: 'Terminal' }));
    expect(screen.queryByRole('heading', { name: 'Conclusion' })).toBeNull();
    expect(screen.getByText(/Session fermée/)).toBeDefined();
    fireEvent.click(screen.getByRole('tab', { name: 'Rapport' }));
    expect(screen.getByRole('heading', { name: 'Conclusion' })).toBeDefined();
  });

  it('shows no tabs when the agent wrote no report', () => {
    renderView(makeAgent({ liveSessionId: null, report: null }));
    expect(screen.queryByRole('tablist')).toBeNull();
  });
});
