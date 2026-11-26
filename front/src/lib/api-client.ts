// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { CatalogueDecision, Language, LanguageInput, PostMessageInput } from '@/lib/contract';
import { daemonFetch } from '@/lib/daemon-connection';

/** A call the daemon answered with an error; `refusal` is the reason it gave, when it gave one in words. */
export class ApiError extends Error {
  readonly status: number;
  readonly refusal: string | null;

  constructor(path: string, status: number, refusal: string | null) {
    super(`${path} answered ${status}`);
    this.name = 'ApiError';
    this.status = status;
    this.refusal = refusal;
  }
}

/** The reason the daemon gave for refusing a call, null for any other failure (the daemon is unreachable). */
export function refusalOf(cause: unknown): string | null {
  return cause instanceof ApiError ? cause.refusal : null;
}

async function readRefusal(response: Response): Promise<string | null> {
  const body: unknown = await response.json().catch(() => null);
  if (typeof body === 'object' && body !== null && 'error' in body && typeof body.error === 'string') return body.error;
  return null;
}

async function postJson(path: string, body?: unknown): Promise<void> {
  const response = await daemonFetch(path, {
    method: 'POST',
    headers: body === undefined ? {} : { 'content-type': 'application/json' },
    body: body === undefined ? null : JSON.stringify(body),
  });
  if (!response.ok) {
    throw new ApiError(path, response.status, await readRefusal(response));
  }
}

export function sendOrchestratorMessage(input: PostMessageInput): Promise<void> {
  return postJson('/api/messages', input);
}

/** Starts a new orchestrator conversation; resolves to the refusal message when it is busy (409). */
export async function resetOrchestrator(): Promise<string | null> {
  const response = await daemonFetch('/api/orchestrator/reset', { method: 'POST' });
  if (response.ok) return null;
  if (response.status === 409) {
    const refusal = await readRefusal(response);
    if (refusal !== null) return refusal;
  }
  throw new Error(`/api/orchestrator/reset answered ${response.status}`);
}

/** Changes the language of the whole factory; the daemon then sends a fresh snapshot to every client. */
export function setLanguage(language: Language): Promise<void> {
  const input: LanguageInput = { language };
  return postJson('/api/language', input);
}

export function resumeAgent(agentId: string): Promise<void> {
  return postJson(`/api/agents/${encodeURIComponent(agentId)}/resume`);
}

export function closeAgent(agentId: string): Promise<void> {
  return postJson(`/api/agents/${encodeURIComponent(agentId)}/close`);
}

export function archiveAgent(agentId: string): Promise<void> {
  return postJson(`/api/agents/${encodeURIComponent(agentId)}/archive`);
}

/** Applies or refuses the change to the catalogue a message proposes; the daemon checks an applied one again first. */
export function decideCatalogueChange(messageId: string, decision: CatalogueDecision): Promise<void> {
  return postJson(`/api/messages/${encodeURIComponent(messageId)}/catalogue-change`, { decision });
}

/** Keeps a note in the memory of a project, or of every project when `repo` is null, for future agents. */
export function saveNote(repo: string | null, note: string): Promise<void> {
  return postJson(repo === null ? '/api/notes' : `/api/repos/${encodeURIComponent(repo)}/notes`, { note });
}
