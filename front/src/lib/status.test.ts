// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from 'vitest';
import { makeAgent } from '@/test/fixtures';
import { en } from './messages/en';
import { fr } from './messages/fr';
import { elapsedLabel, elapsedSince, statusLook } from './status';

describe('statusLook', () => {
  it('needs no note while working', () => {
    expect(statusLook('working', fr)).toEqual({ note: null, phrase: 'au travail' });
  });

  it('says who or what the agent is held by', () => {
    expect(statusLook('waiting', fr).note).toBe('Vous attend');
    expect(statusLook('blocked', fr).note).toBe('Bloqué');
    expect(statusLook('done', fr).note).toBe('Terminé');
  });

  it('speaks English with the English dictionary', () => {
    expect(statusLook('working', en)).toEqual({ note: null, phrase: 'working' });
    expect(statusLook('waiting', en)).toEqual({ note: 'Waiting for you', phrase: 'waiting for you' });
    expect(statusLook('blocked', en).note).toBe('Blocked');
    expect(statusLook('done', en).note).toBe('Done');
  });
});

describe('elapsedSince', () => {
  it('counts from the step start while working', () => {
    expect(elapsedSince(makeAgent())).toBe('2026-11-22T20:00:00.000Z');
  });

  it('counts from the start of the wait while waiting', () => {
    const agent = makeAgent({ status: 'waiting', waitingSince: '2026-11-22T20:30:00.000Z' });
    expect(elapsedSince(agent)).toBe('2026-11-22T20:30:00.000Z');
  });

  it('falls back to the step start when the wait start is missing', () => {
    expect(elapsedSince(makeAgent({ status: 'waiting' }))).toBe('2026-11-22T20:00:00.000Z');
  });

  it('shows no time once done', () => {
    expect(elapsedSince(makeAgent({ status: 'done' }))).toBeNull();
  });
});

describe('elapsedLabel', () => {
  it('formats the time since the step start', () => {
    expect(elapsedLabel(makeAgent(), Date.parse('2026-11-22T20:06:00.000Z'))).toBe('6 min');
  });

  it('is null once done', () => {
    expect(elapsedLabel(makeAgent({ status: 'done' }), Date.parse('2026-11-22T20:06:00.000Z'))).toBeNull();
  });
});
