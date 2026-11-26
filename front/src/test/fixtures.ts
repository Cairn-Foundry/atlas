// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { AgentStepView, AgentView } from '@/lib/contract';

export function makeStep(overrides: Partial<AgentStepView> = {}): AgentStepView {
  return {
    id: 'step-1',
    brickId: 'qualification',
    name: 'Qualification',
    state: 'current',
    pass: 1,
    model: 'sonnet',
    startedAt: '2026-11-22T20:00:00.000Z',
    endedAt: null,
    waitMs: 0,
    usage: null,
    ...overrides,
  };
}

export function makeAgent(overrides: Partial<AgentView> = {}): AgentView {
  return {
    id: 'agent-1',
    ticketRef: 'PROJ-1',
    title: 'Export CSV',
    repoName: 'proj',
    repoPath: '/repo',
    worktreePath: '/worktrees/proj-1',
    branch: 'feat/proj-1',
    extraRepos: [],
    stepId: 'qualification',
    steps: [makeStep()],
    status: 'working',
    statusReason: null,
    stepStartedAt: '2026-11-22T20:00:00.000Z',
    waitingSince: null,
    mrUrl: null,
    liveSessionId: 'session-1',
    report: null,
    createdAt: '2026-11-22T20:00:00.000Z',
    updatedAt: '2026-11-22T20:00:00.000Z',
    ...overrides,
  };
}
