// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

export type Route = { view: 'board' } | { view: 'agent'; agentId: string };

/** The board is the default view: any hash that names nothing else shows it. */
export const BOARD_HASH = '#/';

const AGENT_HASH = /^#\/agents\/([^/]+)$/;

export function parseRoute(hash: string): Route {
  const match = AGENT_HASH.exec(hash);
  const agentId = match?.[1];
  return agentId === undefined ? { view: 'board' } : { view: 'agent', agentId: decodeURIComponent(agentId) };
}

export function agentHash(agentId: string): string {
  return `#/agents/${encodeURIComponent(agentId)}`;
}
