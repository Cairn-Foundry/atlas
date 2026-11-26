// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useEffect, useMemo, useRef, type ReactElement } from 'react';
import { onFrame } from '@/lib/frame-loop';
import { PointCloudRenderer, type PointCloudOptions } from '@/lib/point-cloud-renderer';
import { useMediaQuery } from '@/hooks/use-media-query';
import { useTheme } from '@/hooks/use-theme';

interface PointCloudProps extends Partial<Omit<PointCloudOptions, 'shape'>> {
  shape: PointCloudOptions['shape'];
  /** Number of points; fixed for the life of the component. */
  count: number;
  /** Sets the size, the dot colour (text colour) and, through `--cloud-accent`, the colour of the light. */
  className?: string;
}

/**
 * A shape made of points, drawn on a canvas: it rotates, lights up and morphs when `shape` changes.
 * Purely decorative; with reduced motion it shows the still shape.
 */
export function PointCloud({
  shape,
  count,
  className,
  spin = 0.5,
  dotSize = 1,
  waves = 'none',
  hasGlitch = false,
}: PointCloudProps): ReactElement {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const rendererRef = useRef<PointCloudRenderer | null>(null);
  const isReducedMotion = useMediaQuery('(prefers-reduced-motion: reduce)');
  /** The dot colours come from CSS: read them again when the theme changes. */
  const theme = useTheme();
  const options = useMemo<PointCloudOptions>(
    () => ({ shape, spin, dotSize, waves, hasGlitch }),
    [shape, spin, dotSize, waves, hasGlitch],
  );
  const optionsRef = useRef(options);

  useEffect(() => {
    const canvas = canvasRef.current;
    // No ResizeObserver means no real layout (tests): there is nothing to draw on.
    if (canvas === null || typeof ResizeObserver === 'undefined') return;
    const context = canvas.getContext('2d');
    if (context === null) return;
    const renderer = new PointCloudRenderer(context, count, optionsRef.current);
    rendererRef.current = renderer;

    // At least 2 so the dots stay sharp when the board is zoomed in.
    const ratio = Math.max(2, window.devicePixelRatio);
    const resizeObserver = new ResizeObserver(() => {
      canvas.width = Math.round(canvas.clientWidth * ratio);
      canvas.height = Math.round(canvas.clientHeight * ratio);
      renderer.resize(canvas.width, canvas.height, ratio);
      if (isReducedMotion) renderer.renderStill();
    });
    resizeObserver.observe(canvas);

    let isOnScreen = true;
    const intersectionObserver = new IntersectionObserver((entries) => {
      isOnScreen = entries.at(-1)?.isIntersecting ?? true;
    });
    intersectionObserver.observe(canvas);

    const stopFrames = isReducedMotion
      ? null
      : onFrame((timeMs) => {
          if (isOnScreen) renderer.frame(timeMs);
        });
    return () => {
      stopFrames?.();
      resizeObserver.disconnect();
      intersectionObserver.disconnect();
      rendererRef.current = null;
    };
  }, [count, isReducedMotion]);

  useEffect(() => {
    optionsRef.current = options;
    const canvas = canvasRef.current;
    const renderer = rendererRef.current;
    if (canvas === null || renderer === null) return;
    const style = getComputedStyle(canvas);
    renderer.setOptions(options);
    renderer.setColors(style.color, style.getPropertyValue('--cloud-accent').trim() || style.color);
    if (isReducedMotion) renderer.renderStill();
  }, [options, className, isReducedMotion, theme]);

  return <canvas ref={canvasRef} aria-hidden="true" className={className} />;
}
