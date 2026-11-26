// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

export type PointShape = 'sphere' | 'rings' | 'cross' | 'check';

/** Shapes drawn facing the viewer: they are never rotated. */
const FLAT_SHAPES: ReadonlySet<PointShape> = new Set(['rings', 'cross', 'check']);

export function isFlatShape(shape: PointShape): boolean {
  return FLAT_SHAPES.has(shape);
}

type Random = () => number;
type Point = readonly [number, number, number];
type Sampler = (index: number, count: number) => Point;

/** Small seeded generator (mulberry32): a shape is the same on every build. */
function createRandom(seed: number): Random {
  let state = seed;
  return () => {
    state = (state + 0x6d2b79f5) | 0;
    let mixed = Math.imul(state ^ (state >>> 15), 1 | state);
    mixed = (mixed + Math.imul(mixed ^ (mixed >>> 7), 61 | mixed)) ^ mixed;
    return ((mixed ^ (mixed >>> 14)) >>> 0) / 4294967296;
  };
}

const GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5));

const sampleSphere: Sampler = (index, count) => {
  const y = 1 - (2 * (index + 0.5)) / count;
  const radius = Math.sqrt(1 - y * y);
  const angle = index * GOLDEN_ANGLE;
  return [0.86 * radius * Math.cos(angle), 0.86 * y, 0.86 * radius * Math.sin(angle)];
};

const sampleRings: Sampler = (index, count) => {
  const centre = Math.max(1, Math.round(count * 0.12));
  if (index < centre) {
    const angle = (index / centre) * 2 * Math.PI;
    return [0.14 * Math.cos(angle), 0.14 * Math.sin(angle), 0];
  }
  const inner = Math.round((count - centre) * 0.38);
  const onInner = index - centre < inner;
  const ringCount = onInner ? inner : count - centre - inner;
  const ringIndex = onInner ? index - centre : index - centre - inner;
  const radius = onInner ? 0.5 : 0.88;
  const angle = (ringIndex / ringCount) * 2 * Math.PI;
  return [radius * Math.cos(angle), radius * Math.sin(angle), 0];
};

const sampleCross: Sampler = (index, count) => {
  const half = Math.ceil(count / 2);
  const onFirst = index < half;
  const along = onFirst ? index / (half - 1) : (index - half) / (count - half - 1);
  const offset = (along * 2 - 1) * 0.7;
  return [offset, onFirst ? offset : -offset, 0];
};

const CHECK_PATH: readonly [Point, Point, Point] = [
  [-0.72, -0.02, 0],
  [-0.24, -0.5, 0],
  [0.76, 0.52, 0],
];

const sampleCheck: Sampler = (index, count) => {
  const [start, corner, end] = CHECK_PATH;
  const shortLength = Math.hypot(corner[0] - start[0], corner[1] - start[1]);
  const longLength = Math.hypot(end[0] - corner[0], end[1] - corner[1]);
  const distance = (index / (count - 1)) * (shortLength + longLength);
  const isOnShortStroke = distance < shortLength;
  const from = isOnShortStroke ? start : corner;
  const to = isOnShortStroke ? corner : end;
  const along = isOnShortStroke ? distance / shortLength : (distance - shortLength) / longLength;
  return [from[0] + (to[0] - from[0]) * along, from[1] + (to[1] - from[1]) * along, 0];
};

const SAMPLERS: Record<PointShape, Sampler> = {
  sphere: sampleSphere,
  rings: sampleRings,
  cross: sampleCross,
  check: sampleCheck,
};

/**
 * `count` points of `shape` as xyz triplets inside the unit ball, y up. The order is shuffled
 * the same way for every shape, so morphing from one to another reads as a swarm regrouping.
 */
export function buildShape(shape: PointShape, count: number): Float32Array {
  const sampler = SAMPLERS[shape];
  const points = new Float32Array(count * 3);
  for (let index = 0; index < count; index++) {
    points.set(sampler(index, count), index * 3);
  }
  const shuffle = createRandom(0x9e37);
  for (let index = count - 1; index > 0; index--) {
    const other = Math.floor(shuffle() * (index + 1));
    for (let axis = 0; axis < 3; axis++) {
      const held = points[index * 3 + axis] ?? 0;
      points[index * 3 + axis] = points[other * 3 + axis] ?? 0;
      points[other * 3 + axis] = held;
    }
  }
  return points;
}
