// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from 'vitest';
import { makeAgent, makeStep } from '@/test/fixtures';
import { en } from './messages/en';
import { fr } from './messages/fr';
import { activeMs, currentStep, formatTokens, passLabel, weightedTokens } from './steps';

describe('currentStep', () => {
  it('finds the step in progress', () => {
    const agent = makeAgent({ steps: [makeStep({ id: 'a', state: 'done' }), makeStep({ id: 'b' }), makeStep({ id: 'c', state: 'upcoming' })] });
    expect(currentStep(agent)?.id).toBe('b');
  });

  it('is undefined when no step is in progress', () => {
    expect(currentStep(makeAgent({ steps: [makeStep({ state: 'done' })] }))).toBeUndefined();
  });
});

describe('passLabel', () => {
  it('only labels repeated passes', () => {
    expect(passLabel(1, fr)).toBeNull();
    expect(passLabel(2, fr)).toBe('passe 2');
    expect(passLabel(2, en)).toBe('pass 2');
  });
});

describe('activeMs', () => {
  const base = { startedAt: '2026-11-22T20:00:00.000Z', endedAt: '2026-11-22T20:30:00.000Z' };

  it('leaves out the time spent waiting for the master', () => {
    expect(activeMs(makeStep({ ...base, waitMs: 10 * 60_000 }))).toBe(20 * 60_000);
  });

  it('is null until the step has started and ended', () => {
    expect(activeMs(makeStep({ ...base, endedAt: null }))).toBeNull();
    expect(activeMs(makeStep({ ...base, startedAt: null }))).toBeNull();
  });

  it('never goes below zero', () => {
    expect(activeMs(makeStep({ ...base, waitMs: 3_600_000 }))).toBe(0);
  });
});

describe('weightedTokens', () => {
  it('counts cache reads a tenth, cache writes a quarter more and output five times an input token', () => {
    expect(weightedTokens({ inputTokens: 30, outputTokens: 5721, cacheReadTokens: 999_568, cacheWriteTokens: 61_821 })).toBe(205_868);
    expect(weightedTokens({ inputTokens: 100, outputTokens: 0, cacheReadTokens: 0, cacheWriteTokens: 0 })).toBe(100);
  });
});

describe('formatTokens', () => {
  it('keeps small counts as they are', () => {
    expect(formatTokens(0, fr)).toBe('0');
    expect(formatTokens(850, fr)).toBe('850');
  });

  it('shortens thousands to k', () => {
    expect(formatTokens(1000, fr)).toBe('1 k');
    expect(formatTokens(850_400, fr)).toBe('850 k');
  });

  it('shortens millions to M with a decimal comma', () => {
    expect(formatTokens(1_200_000, fr)).toBe('1,2 M');
    expect(formatTokens(2_000_000, fr)).toBe('2 M');
  });

  it('rolls 999 500 over to M instead of showing 1000 k', () => {
    expect(formatTokens(999_499, fr)).toBe('999 k');
    expect(formatTokens(999_500, fr)).toBe('1 M');
  });

  it('writes the decimal point of English', () => {
    expect(formatTokens(1_200_000, en)).toBe('1.2 M');
    expect(formatTokens(850_400, en)).toBe('850 k');
  });
});
