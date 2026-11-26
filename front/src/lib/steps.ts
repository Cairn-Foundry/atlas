// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { AgentStepView, AgentView, StepUsage } from '@/lib/contract';
import type { Messages } from './messages/fr';

export function currentStep(agent: AgentView): AgentStepView | undefined {
  return agent.steps.find((step) => step.state === 'current');
}

/** "pass 2" for a repeated review, null for a step run once. */
export function passLabel(pass: number, messages: Messages): string | null {
  return pass > 1 ? messages.steps.pass(pass) : null;
}

/** Milliseconds the agent actually worked on a step, waiting for the master excluded; null while it has not ended. */
export function activeMs(step: AgentStepView): number | null {
  if (step.startedAt === null || step.endedAt === null) {
    return null;
  }
  const span = Date.parse(step.endedAt) - Date.parse(step.startedAt) - step.waitMs;
  return Number.isNaN(span) ? null : Math.max(0, span);
}

/** What each kind of token costs next to a fresh input token, from the public API price ratios. */
const OUTPUT_WEIGHT = 5;
const CACHE_WRITE_WEIGHT = 1.25;
const CACHE_READ_WEIGHT = 0.1;

/**
 * What a step cost, in input-token equivalents. A session re-reads its whole conversation from
 * the cache on every turn: adding those reads up at face value makes a small step look huge,
 * while they are billed a tenth of a fresh token.
 */
export function weightedTokens(usage: StepUsage): number {
  return Math.round(
    usage.inputTokens + usage.outputTokens * OUTPUT_WEIGHT + usage.cacheWriteTokens * CACHE_WRITE_WEIGHT + usage.cacheReadTokens * CACHE_READ_WEIGHT,
  );
}

const millionFormatsByLocale = new Map<string, Intl.NumberFormat>();

function millionFormatFor(locale: string): Intl.NumberFormat {
  let format = millionFormatsByLocale.get(locale);
  if (format === undefined) {
    format = new Intl.NumberFormat(locale, { maximumFractionDigits: 1 });
    millionFormatsByLocale.set(locale, format);
  }
  return format;
}

/** Compact token count: "850", "850 k", "1,2 M" (a decimal point in English). */
export function formatTokens(count: number, messages: Messages): string {
  if (count < 1000) {
    return String(count);
  }
  if (count < 999_500) {
    return `${Math.round(count / 1000)} k`;
  }
  return `${millionFormatFor(messages.locale).format(count / 1_000_000)} M`;
}
