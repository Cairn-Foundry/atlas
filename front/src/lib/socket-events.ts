// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { ServerEvent, TerminalServerEvent } from '@/lib/contract';

/** Every `type` of a union, as a record so the compiler flags a type added to the union and missing here. */
type EventTypes<Event extends { type: string }> = Record<Event['type'], true>;

const SERVER_EVENT_TYPES: EventTypes<ServerEvent> = {
  snapshot: true,
  agent: true,
  archived: true,
  message: true,
  orchestrator: true,
  reset: true,
};

const TERMINAL_EVENT_TYPES: EventTypes<TerminalServerEvent> = { output: true, exit: true };

/**
 * Reads a websocket frame as one of the events of `knownTypes`; null for a frame that is not
 * JSON or whose `type` is unknown. Only the discriminant is checked: the daemon builds the rest
 * from the same shared types.
 */
function parseEvent<Event extends { type: string }>(data: string, knownTypes: EventTypes<Event>): Event | null {
  let payload: unknown;
  try {
    payload = JSON.parse(data);
  } catch {
    return null;
  }
  if (typeof payload !== 'object' || payload === null || !('type' in payload) || typeof payload.type !== 'string') {
    return null;
  }
  return Object.hasOwn(knownTypes, payload.type) ? (payload as Event) : null;
}

/** An event of `GET /ws`, or null when the frame is not one. */
export function parseServerEvent(data: string): ServerEvent | null {
  return parseEvent(data, SERVER_EVENT_TYPES);
}

/** An event of `GET /ws/terminal/:agentId`, or null when the frame is not one. */
export function parseTerminalServerEvent(data: string): TerminalServerEvent | null {
  return parseEvent(data, TERMINAL_EVENT_TYPES);
}
