// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { buildShape, isFlatShape, type PointShape } from './point-shapes';

export type WaveMode = 'none' | 'random' | 'center';

export interface PointCloudOptions {
  /** The cloud morphs into the new shape when it changes. */
  shape: PointShape;
  /** Rotation speed in radians per second; flat shapes never rotate. */
  spin: number;
  /** Dot radius in CSS pixels for a point at mid depth. */
  dotSize: number;
  /** Shells of light that travel through the cloud, from random points or from the centre. */
  waves: WaveMode;
  /** Brief horizontal tearing every few seconds. */
  hasGlitch: boolean;
}

interface Wave {
  x: number;
  y: number;
  z: number;
  startedAt: number;
}

const MORPH_MS = 1500;
/** Share of the morph over which the points leave one after another. */
const MORPH_STAGGER = 0.45;
const CAMERA_DISTANCE = 3.4;
const TILT = -0.32;
const TILT_COS = Math.cos(TILT);
const TILT_SIN = Math.sin(TILT);
const WAVE_SPEED = 0.95;
const WAVE_LIFE_MS = 1600;
const WAVE_WIDTH = 0.1;
const RANDOM_WAVE_EVERY_MS = 1100;
const CENTER_WAVE_EVERY_MS = 1500;
const GLITCH_EVERY_MS = 2600;
const GLITCH_MS = 240;

const shapeCache = new Map<string, Float32Array>();

function cachedShape(shape: PointShape, count: number): Float32Array {
  const key = `${shape}:${count}`;
  let points = shapeCache.get(key);
  if (points === undefined) {
    points = buildShape(shape, count);
    shapeCache.set(key, points);
  }
  return points;
}

function at(values: Float32Array, index: number): number {
  return values[index] ?? 0;
}

function easeInOut(progress: number): number {
  return progress < 0.5 ? 4 * progress ** 3 : 1 - (-2 * progress + 2) ** 3 / 2;
}

function noise(seed: number): number {
  const value = Math.sin(seed) * 43758.5453;
  return value - Math.floor(value);
}

/** Draws a cloud of points on a 2D canvas: a 3D shape that rotates, lights up and morphs into the next one. */
export class PointCloudRenderer {
  private readonly context: CanvasRenderingContext2D;
  private readonly count: number;
  private options: PointCloudOptions;
  private width = 0;
  private height = 0;
  private ratio = 1;
  private baseColor = '#000';
  private accentColor = '#000';

  /** Where each point leaves from, in view space. */
  private readonly origin: Float32Array;
  private target: Float32Array;
  private targetShape: PointShape;
  private isTargetFlat: boolean;
  /** Current position of each point in view space. */
  private readonly position: Float32Array;
  /** Per point: screen x, screen y, alpha, light. */
  private readonly screen: Float32Array;
  private readonly seeds: Float32Array;

  private morphStartedAt: number | null = null;
  private lastFrameAt: number | null = null;
  private lastWaveAt = 0;
  private angle = 0;
  private waves: Wave[] = [];
  private isSettled = false;

  constructor(context: CanvasRenderingContext2D, count: number, options: PointCloudOptions) {
    this.context = context;
    this.count = count;
    this.options = options;
    this.targetShape = options.shape;
    this.target = cachedShape(this.targetShape, count);
    this.isTargetFlat = isFlatShape(this.targetShape);
    this.origin = new Float32Array(count * 3);
    this.position = new Float32Array(count * 3);
    this.screen = new Float32Array(count * 4);
    this.seeds = new Float32Array(count);
    for (let index = 0; index < count; index++) {
      this.seeds[index] = noise(index * 12.9898 + 4.1);
      // The cloud first gathers from a scattered ring.
      const angle = noise(index * 7.31) * 2 * Math.PI;
      const radius = 0.9 + noise(index * 3.77) * 0.6;
      this.origin[index * 3] = radius * Math.cos(angle);
      this.origin[index * 3 + 1] = radius * Math.sin(angle) * 0.8;
      this.origin[index * 3 + 2] = (noise(index * 5.13) - 0.5) * 0.8;
    }
  }

  setOptions(options: PointCloudOptions): void {
    this.options = options;
    this.isSettled = false;
  }

  setColors(baseColor: string, accentColor: string): void {
    if (baseColor === this.baseColor && accentColor === this.accentColor) return;
    this.baseColor = baseColor;
    this.accentColor = accentColor;
    this.isSettled = false;
  }

  resize(width: number, height: number, ratio: number): void {
    this.width = width;
    this.height = height;
    this.ratio = ratio;
    this.isSettled = false;
  }

  frame(timeMs: number): void {
    const elapsedMs = this.lastFrameAt === null ? 0 : Math.min(64, timeMs - this.lastFrameAt);
    this.lastFrameAt = timeMs;
    this.morphStartedAt ??= timeMs;

    const wanted = this.options.shape;
    if (wanted !== this.targetShape) {
      this.origin.set(this.position);
      this.targetShape = wanted;
      this.target = cachedShape(wanted, this.count);
      this.isTargetFlat = isFlatShape(wanted);
      this.morphStartedAt = timeMs;
      this.isSettled = false;
    }

    const morph = (timeMs - this.morphStartedAt) / MORPH_MS;
    if (this.isSettled && morph > 1 && !this.isMoving()) return;
    this.angle += (this.options.spin * elapsedMs) / 1000;
    this.spawnWaves(timeMs);
    this.draw(timeMs, morph, true);
    this.isSettled = true;
  }

  /** The final state of the shape, for viewers who asked for reduced motion. */
  renderStill(): void {
    const wanted = this.options.shape;
    this.targetShape = wanted;
    this.target = cachedShape(wanted, this.count);
    this.isTargetFlat = isFlatShape(wanted);
    this.waves = [];
    this.draw(0, 1, false);
  }

  private isMoving(): boolean {
    const { spin, waves, hasGlitch } = this.options;
    return (spin !== 0 && !this.isTargetFlat) || waves !== 'none' || hasGlitch;
  }

  private spawnWaves(timeMs: number): void {
    this.waves = this.waves.filter((wave) => timeMs - wave.startedAt < WAVE_LIFE_MS);
    const mode = this.options.waves;
    if (mode === 'none') return;
    const every = mode === 'center' ? CENTER_WAVE_EVERY_MS : RANDOM_WAVE_EVERY_MS;
    if (timeMs - this.lastWaveAt < every) return;
    this.lastWaveAt = timeMs;
    if (mode === 'center') {
      this.waves.push({ x: 0, y: 0, z: 0, startedAt: timeMs });
      return;
    }
    const source = Math.floor(Math.random() * this.count) * 3;
    this.waves.push({ x: at(this.target, source), y: at(this.target, source + 1), z: at(this.target, source + 2), startedAt: timeMs });
  }

  private waveLight(timeMs: number, x: number, y: number, z: number): number {
    let light = 0;
    for (const wave of this.waves) {
      const age = timeMs - wave.startedAt;
      const gap = (Math.hypot(x - wave.x, y - wave.y, z - wave.z) - (age / 1000) * WAVE_SPEED) / WAVE_WIDTH;
      light = Math.max(light, Math.exp(-gap * gap) * (1 - age / WAVE_LIFE_MS));
    }
    return light;
  }

  private draw(timeMs: number, morph: number, isAnimated: boolean): void {
    const { context, count, width, height, ratio, options } = this;
    if (width === 0 || height === 0) return;
    context.clearRect(0, 0, width, height);

    const unit = Math.min(width, height) / 2 / 1.1;
    const centerX = width / 2;
    const centerY = height / 2;
    const cos = Math.cos(this.angle);
    const sin = Math.sin(this.angle);
    const isGlitching = isAnimated && options.hasGlitch && timeMs % GLITCH_EVERY_MS < GLITCH_MS;
    const glitchTick = Math.floor(timeMs / 70);
    const isMorphing = morph < 1;

    for (let index = 0; index < count; index++) {
      const slot = index * 3;
      const seed = at(this.seeds, index);
      const targetX = at(this.target, slot);
      const targetY = at(this.target, slot + 1);
      const targetZ = at(this.target, slot + 2);

      let x = targetX;
      let y = targetY;
      let z = targetZ;
      if (!this.isTargetFlat) {
        const turnedX = targetX * cos + targetZ * sin;
        const turnedZ = targetZ * cos - targetX * sin;
        x = turnedX;
        y = targetY * TILT_COS - turnedZ * TILT_SIN;
        z = targetY * TILT_SIN + turnedZ * TILT_COS;
      }

      let light = isAnimated ? this.waveLight(timeMs, targetX, targetY, targetZ) : 0;
      if (isMorphing) {
        const local = Math.min(1, Math.max(0, morph * (1 + MORPH_STAGGER) - seed * MORPH_STAGGER));
        const eased = easeInOut(local);
        const arc = Math.sin(Math.PI * eased);
        const burst = 1 + 0.22 * arc;
        x = (at(this.origin, slot) + (x - at(this.origin, slot)) * eased) * burst;
        y = (at(this.origin, slot + 1) + (y - at(this.origin, slot + 1)) * eased) * burst;
        z = (at(this.origin, slot + 2) + (z - at(this.origin, slot + 2)) * eased) * burst;
        light = Math.max(light, arc * 0.75);
      } else if (isAnimated) {
        const breath = 1 + 0.018 * Math.sin(timeMs * 0.0016 + seed * 40);
        x *= breath;
        y *= breath;
        z *= breath;
      }
      this.position[slot] = x;
      this.position[slot + 1] = y;
      this.position[slot + 2] = z;

      if (isGlitching) {
        const band = Math.floor((y + 1) * 3.5);
        x += (noise(band * 12.9898 + glitchTick * 78.233) - 0.5) * 0.6;
      }

      const scale = CAMERA_DISTANCE / (CAMERA_DISTANCE - z);
      const depth = Math.min(1, Math.max(0, (z + 1) / 2));
      const screenSlot = index * 4;
      this.screen[screenSlot] = centerX + x * scale * unit;
      this.screen[screenSlot + 1] = centerY - y * scale * unit;
      this.screen[screenSlot + 2] = Math.min(1, 0.3 + 0.95 * depth);
      this.screen[screenSlot + 3] = light;
    }

    const dot = options.dotSize * ratio;
    context.fillStyle = this.baseColor;
    for (let index = 0; index < count; index++) {
      const slot = index * 4;
      const alpha = at(this.screen, slot + 2);
      context.globalAlpha = alpha * (1 - 0.7 * at(this.screen, slot + 3));
      context.beginPath();
      context.arc(at(this.screen, slot), at(this.screen, slot + 1), dot * (0.55 + 0.6 * alpha), 0, 2 * Math.PI);
      context.fill();
    }
    context.fillStyle = this.accentColor;
    for (let index = 0; index < count; index++) {
      const slot = index * 4;
      const light = at(this.screen, slot + 3);
      if (light < 0.04) continue;
      const alpha = at(this.screen, slot + 2);
      const radius = dot * (0.55 + 0.6 * alpha) * (1 + 0.9 * light);
      context.globalAlpha = light * 0.07;
      context.beginPath();
      context.arc(at(this.screen, slot), at(this.screen, slot + 1), radius * 2.8, 0, 2 * Math.PI);
      context.fill();
      context.globalAlpha = Math.min(1, light * (0.4 + alpha));
      context.beginPath();
      context.arc(at(this.screen, slot), at(this.screen, slot + 1), radius, 0, 2 * Math.PI);
      context.fill();
    }
    context.globalAlpha = 1;
  }
}
