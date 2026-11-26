// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from 'vitest';
import type { ChatMessage, Snapshot } from '@/lib/contract';
import { makeAgent } from '@/test/fixtures';
import { initialStreamState, streamReducer, type StreamState } from './stream-state';

const message: ChatMessage = { id: 'm1', role: 'assistant', kind: 'text', text: 'Bonjour', card: null, actions: [], agentId: null, createdAt: '2026-11-22T20:00:00.000Z' };
const snapshot: Snapshot = { bricks: [{ id: 'qualification', name: 'Qualification' }], agents: [makeAgent()], messages: [], orchestratorBusy: false, language: 'fr' };
const loaded: StreamState = { ...initialStreamState, snapshot };

describe('streamReducer', () => {
  it('replaces everything on snapshot', () => {
    expect(streamReducer(initialStreamState, { type: 'snapshot', snapshot }).snapshot).toBe(snapshot);
  });

  it('takes the busy flag from the snapshot, for a client that connects mid-run', () => {
    expect(streamReducer(initialStreamState, { type: 'snapshot', snapshot: { ...snapshot, orchestratorBusy: true } }).isOrchestratorBusy).toBe(true);
  });

  it('empties the chat thread on reset and keeps the board', () => {
    const withMessage = streamReducer(loaded, { type: 'message', message });
    const reset = streamReducer(withMessage, { type: 'reset' });
    expect(reset.snapshot?.messages).toEqual([]);
    expect(reset.snapshot?.agents).toHaveLength(1);
  });

  it('updates an existing agent in place', () => {
    const updated = makeAgent({ status: 'waiting' });
    const next = streamReducer(loaded, { type: 'agent', agent: updated });
    expect(next.snapshot?.agents).toEqual([updated]);
  });

  it('appends a new agent', () => {
    const next = streamReducer(loaded, { type: 'agent', agent: makeAgent({ id: 'agent-2' }) });
    expect(next.snapshot?.agents.map((agent) => agent.id)).toEqual(['agent-1', 'agent-2']);
  });

  it('removes an archived agent and keeps the others', () => {
    const two = streamReducer(loaded, { type: 'agent', agent: makeAgent({ id: 'agent-2' }) });
    const next = streamReducer(two, { type: 'archived', agentId: 'agent-1' });
    expect(next.snapshot?.agents.map((agent) => agent.id)).toEqual(['agent-2']);
  });

  it('ignores an archived event for an unknown agent or before the snapshot', () => {
    expect(streamReducer(loaded, { type: 'archived', agentId: 'ghost' }).snapshot?.agents).toHaveLength(1);
    expect(streamReducer(initialStreamState, { type: 'archived', agentId: 'agent-1' })).toBe(initialStreamState);
  });

  it('appends messages once, even when the server repeats one', () => {
    const once = streamReducer(loaded, { type: 'message', message });
    const twice = streamReducer(once, { type: 'message', message });
    expect(twice.snapshot?.messages).toEqual([message]);
  });

  it('replaces a message the server sends again, such as a decided proposal', () => {
    const once = streamReducer(loaded, { type: 'message', message });
    const decided = { ...message, actions: [{ type: 'archive' as const }] };
    expect(streamReducer(once, { type: 'message', message: decided }).snapshot?.messages).toEqual([decided]);
  });

  it('ignores agent and message events that arrive before the snapshot', () => {
    expect(streamReducer(initialStreamState, { type: 'agent', agent: makeAgent() })).toBe(initialStreamState);
    expect(streamReducer(initialStreamState, { type: 'message', message })).toBe(initialStreamState);
  });

  it('tracks the orchestrator and the connection', () => {
    const busy = streamReducer(loaded, { type: 'orchestrator', busy: true });
    expect(busy.isOrchestratorBusy).toBe(true);
    expect(streamReducer(busy, { type: 'connection', isConnected: true }).isConnected).toBe(true);
  });
});
