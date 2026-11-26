// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { DaemonGate } from '@/components/DaemonGate';

describe('DaemonGate', () => {
  afterEach(cleanup);

  it('shows the factory only once the daemon answers', async () => {
    let answer: () => void = () => {};
    const connect = vi.fn(() => new Promise<void>((resolve) => (answer = resolve)));
    render(
      <DaemonGate connect={connect}>
        <p>usine</p>
      </DaemonGate>,
    );
    expect(screen.getByText("Démarrage de l'usine...")).toBeTruthy();
    expect(screen.queryByText('usine')).toBeNull();
    answer();
    expect(await screen.findByText('usine')).toBeTruthy();
  });

  it('tells why the daemon could not start and connects again on retry', async () => {
    const connect = vi.fn().mockRejectedValueOnce('Le daemon ne répond pas sur le port 4317.').mockResolvedValueOnce(undefined);
    render(
      <DaemonGate connect={connect}>
        <p>usine</p>
      </DaemonGate>,
    );
    expect((await screen.findByRole('alert')).textContent).toBe('Le daemon ne répond pas sur le port 4317.');
    fireEvent.click(screen.getByRole('button', { name: 'Réessayer' }));
    expect(await screen.findByText('usine')).toBeTruthy();
    expect(connect).toHaveBeenCalledTimes(2);
  });
});
