// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { Messages } from './messages/fr';

const MINUTE_MS = 60_000;

interface MessageFormats {
  time: Intl.DateTimeFormat;
  day: Intl.DateTimeFormat;
}

const formatsByLocale = new Map<string, MessageFormats>();

function formatsFor(locale: string): MessageFormats {
  let formats = formatsByLocale.get(locale);
  if (formats === undefined) {
    // h23: `hour12: false` shows midnight as "24:00" in some locales.
    formats = {
      time: new Intl.DateTimeFormat(locale, { hour: '2-digit', minute: '2-digit', hourCycle: 'h23' }),
      day: new Intl.DateTimeFormat(locale, { day: 'numeric', month: 'short' }),
    };
    formatsByLocale.set(locale, formats);
  }
  return formats;
}

/**
 * When a chat message was sent, in local time: "21:42" today, "22 nov. 21:42" before (the day is
 * written the way the language does).
 * Empty for an unparseable date.
 */
export function formatMessageTime(iso: string, now: number, messages: Messages): string {
  const sent = new Date(iso);
  if (Number.isNaN(sent.getTime())) {
    return '';
  }
  const formats = formatsFor(messages.locale);
  const time = formats.time.format(sent);
  return sent.toDateString() === new Date(now).toDateString() ? time : `${formats.day.format(sent)} ${time}`;
}

/** Formats a duration the way the board shows it: "6 min", "1 h 04". */
export function formatDuration(durationMs: number): string {
  const minutes = Math.max(0, Math.floor(durationMs / MINUTE_MS));
  if (minutes < 60) {
    return `${minutes} min`;
  }
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return `${hours} h ${rest.toString().padStart(2, '0')}`;
}

/** Formats the time elapsed since `fromIso`; empty for an unparseable date. */
export function formatElapsed(fromIso: string, now: number): string {
  const from = Date.parse(fromIso);
  return Number.isNaN(from) ? '' : formatDuration(now - from);
}

/** Elapsed thinking time, shown only after 5 s: "7 s", "1 min 04 s". */
export function formatThinkingSeconds(elapsedMs: number): string | null {
  const seconds = Math.floor(elapsedMs / 1000);
  if (seconds < 5) return null;
  if (seconds < 60) return `${seconds} s`;
  const rest = seconds % 60;
  return `${Math.floor(seconds / 60)} min ${rest < 10 ? '0' : ''}${rest} s`;
}
