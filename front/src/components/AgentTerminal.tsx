// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useEffect, useRef, type ReactElement } from 'react';
import { FitAddon } from '@xterm/addon-fit';
import { Terminal } from '@xterm/xterm';
import '@xterm/xterm/css/xterm.css';
import { TERMINAL_SCROLLBACK_LINES, type TerminalClientEvent } from '@/lib/contract';
import { websocketUrl } from '@/lib/daemon-connection';
import { messages } from '@/lib/i18n';
import { parseTerminalServerEvent } from '@/lib/socket-events';
import { useMessages } from '@/hooks/use-messages';

interface AgentTerminalProps {
  agentId: string;
  /** Changes when a new session attaches, so the terminal reconnects to it. */
  sessionId: string;
}

function cssToken(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

const FONT_FAMILY = "'JetBrains Mono Variable', ui-monospace, monospace";
const FONT_SIZE = 13;

/** Opens a terminal on the agent's session in `container`; returns what closes it. */
function openTerminal(container: HTMLElement, agentId: string): () => void {
  const terminal = new Terminal({
    fontFamily: FONT_FAMILY,
    fontSize: FONT_SIZE,
    lineHeight: 1.35,
    cursorBlink: true,
    scrollback: TERMINAL_SCROLLBACK_LINES,
    theme: {
      background: cssToken('--terminal'),
      foreground: cssToken('--terminal-foreground'),
      cursor: cssToken('--terminal-foreground'),
    },
  });
  const fit = new FitAddon();
  terminal.loadAddon(fit);
  terminal.open(container);
  fit.fit();

  const socket = new WebSocket(websocketUrl(`/ws/terminal/${encodeURIComponent(agentId)}`));
  const send = (event: TerminalClientEvent): void => {
    if (socket.readyState === WebSocket.OPEN) {
      socket.send(JSON.stringify(event));
    }
  };
  const sendSize = (): void => send({ type: 'resize', cols: terminal.cols, rows: terminal.rows });

  // The daemon waits for this first size before it sends the screen, drawn for that size.
  socket.addEventListener('open', sendSize);
  socket.addEventListener('message', (event: MessageEvent<string>) => {
    const payload = parseTerminalServerEvent(event.data);
    if (payload === null) return;
    if (payload.type === 'output') {
      terminal.write(payload.data);
    } else {
      // Read now, not at open: the terminal stays connected when the language changes.
      terminal.write(`\r\n\x1b[2m${messages().terminal.sessionEnded}\x1b[0m\r\n`);
    }
  });
  const input = terminal.onData((data) => send({ type: 'input', data }));
  const observer = new ResizeObserver(() => {
    const { cols, rows } = terminal;
    fit.fit();
    if (terminal.cols !== cols || terminal.rows !== rows) sendSize();
  });
  observer.observe(container);
  terminal.focus();

  return () => {
    observer.disconnect();
    input.dispose();
    socket.close();
    terminal.dispose();
  };
}

export function AgentTerminal({ agentId, sessionId }: AgentTerminalProps): ReactElement {
  const label = useMessages().terminal.label;
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (container === null) {
      return;
    }
    let isCancelled = false;
    let close: (() => void) | null = null;
    // The terminal measures its cells when it opens: with the web font still loading it would
    // measure the fallback font, and every column would be off once the real one arrives.
    const open = (): void => {
      if (!isCancelled) close = openTerminal(container, agentId);
    };
    document.fonts.load(`${FONT_SIZE}px ${FONT_FAMILY}`).then(open, open);

    return () => {
      isCancelled = true;
      close?.();
    };
  }, [agentId, sessionId]);

  return <div ref={containerRef} role="region" aria-label={label} className="size-full" />;
}
