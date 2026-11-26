// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { AgentView, ChatMessage, ServerEvent, Snapshot } from '@/lib/contract';

export interface StreamState {
  snapshot: Snapshot | null;
  isOrchestratorBusy: boolean;
  isConnected: boolean;
}

export type StreamAction = ServerEvent | { type: 'connection'; isConnected: boolean };

export const initialStreamState: StreamState = { snapshot: null, isOrchestratorBusy: false, isConnected: false };

function upsertAgent(agents: readonly AgentView[], agent: AgentView): AgentView[] {
  const index = agents.findIndex((existing) => existing.id === agent.id);
  return index === -1 ? [...agents, agent] : agents.with(index, agent);
}

/** Adds a new message; one the server sends again, such as a proposal the master decided, replaces the one shown. */
function upsertMessage(messages: readonly ChatMessage[], message: ChatMessage): ChatMessage[] {
  return messages.some((existing) => existing.id === message.id)
    ? messages.map((existing) => (existing.id === message.id ? message : existing))
    : [...messages, message];
}

export function streamReducer(state: StreamState, action: StreamAction): StreamState {
  switch (action.type) {
    case 'connection':
      return { ...state, isConnected: action.isConnected };
    case 'snapshot':
      return { ...state, snapshot: action.snapshot, isOrchestratorBusy: action.snapshot.orchestratorBusy };
    case 'agent':
      return state.snapshot === null
        ? state
        : { ...state, snapshot: { ...state.snapshot, agents: upsertAgent(state.snapshot.agents, action.agent) } };
    case 'archived':
      return state.snapshot === null
        ? state
        : { ...state, snapshot: { ...state.snapshot, agents: state.snapshot.agents.filter((agent) => agent.id !== action.agentId) } };
    case 'message':
      return state.snapshot === null
        ? state
        : { ...state, snapshot: { ...state.snapshot, messages: upsertMessage(state.snapshot.messages, action.message) } };
    case 'orchestrator':
      return { ...state, isOrchestratorBusy: action.busy };
    case 'reset':
      return state.snapshot === null ? state : { ...state, snapshot: { ...state.snapshot, messages: [] } };
    default: {
      const unknown: never = action;
      throw new Error(`Unknown event: ${JSON.stringify(unknown)}`);
    }
  }
}
