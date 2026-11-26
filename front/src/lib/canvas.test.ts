// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from 'vitest';
import { makeAgent, makeStep } from '@/test/fixtures';
import { COLUMN_GAP, COLUMN_WIDTH, MIN_ZOOM, ROW_GAP, ROW_HEIGHT, VIEW_MARGIN, boardWidth, fitViewport, initialViewport, layoutRows, rowCellCount } from './canvas';

describe('layoutRows', () => {
  const steps = (count: number) => Array.from({ length: count }, (_, index) => makeStep({ id: `s${index}` }));

  it('stacks the rows top to bottom in the given order', () => {
    const layout = layoutRows([makeAgent({ id: 'a' }), makeAgent({ id: 'b' })]);
    expect(layout.map((row) => row.position)).toEqual([
      { x: 0, y: 0 },
      { x: 0, y: ROW_HEIGHT + ROW_GAP },
    ]);
  });

  it('sizes each row from its own number of steps', () => {
    const layout = layoutRows([makeAgent({ id: 'a', steps: steps(2) }), makeAgent({ id: 'b', steps: steps(5) })]);
    expect(layout.map((row) => row.width)).toEqual([2 * COLUMN_WIDTH + COLUMN_GAP, 5 * COLUMN_WIDTH + 4 * COLUMN_GAP]);
  });

  it('counts the agent card as one more cell when no step is current', () => {
    const done = [makeStep({ id: 'a', state: 'done' }), makeStep({ id: 'b', state: 'done' })];
    expect(rowCellCount(makeAgent({ steps: done }))).toBe(3);
    expect(rowCellCount(makeAgent({ steps: steps(2) }))).toBe(2);
    expect(layoutRows([makeAgent({ steps: done })])[0]?.width).toBe(3 * COLUMN_WIDTH + 2 * COLUMN_GAP);
    expect(boardWidth([makeAgent({ steps: done }), makeAgent({ steps: steps(3) })])).toBe(3 * COLUMN_WIDTH + 2 * COLUMN_GAP);
  });

  it('takes the widest row for the board width, and 0 for an empty board', () => {
    expect(boardWidth([makeAgent({ steps: steps(2) }), makeAgent({ steps: steps(3) })])).toBe(3 * COLUMN_WIDTH + 2 * COLUMN_GAP);
    expect(boardWidth([])).toBe(0);
  });
});

describe('initialViewport', () => {
  it('shows everything at zoom 1 when it fits', () => {
    expect(initialViewport(600, 1200)).toEqual({ x: VIEW_MARGIN, y: VIEW_MARGIN * 0.75, zoom: 1 });
  });

  it('zooms out to fit, but never below a readable zoom', () => {
    expect(initialViewport(1600, 1424).zoom).toBeCloseTo(0.85);
    expect(initialViewport(1000, 946).zoom).toBeCloseTo(0.85);
    expect(initialViewport(1000, 996).zoom).toBeCloseTo(0.9);
  });
});

describe('fitViewport', () => {
  const board = { x: 0, y: 0, width: 1600, height: 700 };

  it('keeps the margin on both sides and centres the board', () => {
    const view = fitViewport(board, 1080, 836);
    expect(view.zoom).toBeCloseTo((1080 - 2 * VIEW_MARGIN) / 1600);
    const left = view.x;
    const right = 1080 - (view.x + board.width * view.zoom);
    expect(left).toBeCloseTo(VIEW_MARGIN);
    expect(right).toBeCloseTo(VIEW_MARGIN);
  });

  it('never zooms in past 1, and centres a small board', () => {
    const view = fitViewport({ x: 0, y: 0, width: 400, height: 300 }, 1080, 836);
    expect(view.zoom).toBe(1);
    expect(view.x).toBeCloseTo(340);
  });

  it('accounts for the bounds origin and the zoom floor', () => {
    expect(fitViewport({ x: 100, y: 20, width: 100000, height: 700 }, 1080, 836).zoom).toBe(MIN_ZOOM);
    const view = fitViewport({ x: 100, y: 20, width: 400, height: 300 }, 1080, 836);
    expect(view.x).toBeCloseTo(240);
    expect(view.y).toBeCloseTo(VIEW_MARGIN * 0.75 - 20);
  });
});
