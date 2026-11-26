// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

type FrameListener = (timeMs: number) => void;

const listeners = new Set<FrameListener>();
let handle: number | null = null;

function run(timeMs: number): void {
  handle = listeners.size > 0 ? requestAnimationFrame(run) : null;
  for (const listener of listeners) listener(timeMs);
}

/** Calls `listener` on every animation frame. All animated canvases share this single loop. */
export function onFrame(listener: FrameListener): () => void {
  listeners.add(listener);
  handle ??= requestAnimationFrame(run);
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0 && handle !== null) {
      cancelAnimationFrame(handle);
      handle = null;
    }
  };
}
