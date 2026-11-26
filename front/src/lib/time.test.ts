// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from 'vitest';
import { en } from './messages/en';
import { fr } from './messages/fr';
import { formatElapsed, formatMessageTime, formatThinkingSeconds } from './time';

const start = '2026-11-22T20:00:00.000Z';
const at = (minutes: number): number => Date.parse(start) + minutes * 60_000;

describe('formatElapsed', () => {
  it.each([
    [0, '0 min'],
    [6, '6 min'],
    [59, '59 min'],
    [60, '1 h 00'],
    [64, '1 h 04'],
    [135, '2 h 15'],
  ])('formats %i minutes as %s', (minutes, expected) => {
    expect(formatElapsed(start, at(minutes))).toBe(expected);
  });

  it('rounds partial minutes down', () => {
    expect(formatElapsed(start, at(6) + 59_000)).toBe('6 min');
  });

  it('never goes negative when the clock is behind', () => {
    expect(formatElapsed(start, at(-3))).toBe('0 min');
  });

  it('returns an empty string for an invalid date', () => {
    expect(formatElapsed('not a date', at(1))).toBe('');
  });
});

describe('formatMessageTime', () => {
  const today = new Date(2026, 10, 23, 10, 0).getTime();

  it('shows the 24h time for a message sent today', () => {
    expect(formatMessageTime(new Date(2026, 10, 23, 9, 5).toISOString(), today, fr)).toBe('09:05');
    expect(formatMessageTime(new Date(2026, 10, 23, 0, 0).toISOString(), today, fr)).toBe('00:00');
  });

  it('adds the day for an older message', () => {
    expect(formatMessageTime(new Date(2026, 10, 22, 21, 42).toISOString(), today, fr)).toBe('22 nov. 21:42');
  });

  it('writes the day the way English does, with a 24h time', () => {
    expect(formatMessageTime(new Date(2026, 10, 22, 21, 42).toISOString(), today, en)).toBe('Nov 22 21:42');
    expect(formatMessageTime(new Date(2026, 10, 23, 0, 0).toISOString(), today, en)).toBe('00:00');
  });

  it('returns an empty string for an invalid date', () => {
    expect(formatMessageTime('pas une date', today, fr)).toBe('');
  });
});

describe('formatThinkingSeconds', () => {
  it('stays silent for the first seconds, then counts', () => {
    expect(formatThinkingSeconds(4_999)).toBeNull();
    expect(formatThinkingSeconds(7_400)).toBe('7 s');
    expect(formatThinkingSeconds(64_000)).toBe('1 min 04 s');
  });
});
