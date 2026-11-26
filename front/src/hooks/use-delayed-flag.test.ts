// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { act, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useDelayedFlag } from './use-delayed-flag';

describe('useDelayedFlag', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('turns true only after the value stayed true for the delay', () => {
    const { result } = renderHook(({ value }) => useDelayedFlag(value, 1500), { initialProps: { value: true } });
    expect(result.current).toBe(false);
    act(() => vi.advanceTimersByTime(1499));
    expect(result.current).toBe(false);
    act(() => vi.advanceTimersByTime(1));
    expect(result.current).toBe(true);
  });

  it('drops back to false at once when the value turns false', () => {
    const { result, rerender } = renderHook(({ value }) => useDelayedFlag(value, 1500), { initialProps: { value: true } });
    act(() => vi.advanceTimersByTime(2000));
    rerender({ value: false });
    expect(result.current).toBe(false);
  });
});
