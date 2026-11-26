// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { AgentView } from '@/lib/contract';
import { currentStep } from './steps';

export const COLUMN_WIDTH = 208;
export const COLUMN_GAP = 24;
/** Row header (24) + gap (8) + card (104). */
export const ROW_HEIGHT = 136;
export const ROW_GAP = 32;
export const MIN_ZOOM = 0.4;
export const MAX_ZOOM = 1.5;

/** Below this zoom the card text gets too small to read: the first view stops there. */
const READABLE_ZOOM = 0.85;
/** Space kept between the board and the edges of the canvas when the view is framed. */
export const VIEW_MARGIN = 48;

/**
 * First view of the board: as much of it as fits at a readable zoom, anchored top-left
 * so the first steps are always in sight.
 */
export function initialViewport(contentWidth: number, viewportWidth: number): { x: number; y: number; zoom: number } {
  const fit = (viewportWidth - 2 * VIEW_MARGIN) / Math.max(contentWidth, 1);
  const zoom = Math.min(1, Math.max(READABLE_ZOOM, fit));
  return { x: VIEW_MARGIN, y: VIEW_MARGIN * 0.75, zoom };
}

export interface Bounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

/**
 * "Tout afficher": the whole board, centred horizontally with VIEW_MARGIN on each side and
 * anchored to the top, never zoomed in past 1.
 */
export function fitViewport(bounds: Bounds, viewportWidth: number, viewportHeight: number): { x: number; y: number; zoom: number } {
  const fitWidth = (viewportWidth - 2 * VIEW_MARGIN) / Math.max(bounds.width, 1);
  const fitHeight = (viewportHeight - 2 * VIEW_MARGIN) / Math.max(bounds.height, 1);
  const zoom = Math.max(MIN_ZOOM, Math.min(1, fitWidth, fitHeight));
  return {
    x: (viewportWidth - bounds.width * zoom) / 2 - bounds.x * zoom,
    y: VIEW_MARGIN * 0.75 - bounds.y * zoom,
    zoom,
  };
}

/** Cells of an agent's row: its steps, plus the agent card at the end when no step is current. */
export function rowCellCount(agent: AgentView): number {
  return agent.steps.length + (currentStep(agent) === undefined ? 1 : 0);
}

function rowWidth(cellCount: number): number {
  return Math.max(0, cellCount * (COLUMN_WIDTH + COLUMN_GAP) - COLUMN_GAP);
}

/** Width of the widest row: what the first view and the zoom limits are computed from. */
export function boardWidth(agents: readonly AgentView[]): number {
  return Math.max(0, ...agents.map((agent) => rowWidth(rowCellCount(agent))));
}

export interface RowNodeLayout {
  id: string;
  position: { x: number; y: number };
  width: number;
}

/** Rows stacked top to bottom in the given order; steps run left to right inside each, aligned by position. */
export function layoutRows(agents: readonly AgentView[]): RowNodeLayout[] {
  return agents.map((agent, index) => ({
    id: agent.id,
    position: { x: 0, y: index * (ROW_HEIGHT + ROW_GAP) },
    width: rowWidth(rowCellCount(agent)),
  }));
}
