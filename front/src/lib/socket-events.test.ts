// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from 'vitest';
import { parseServerEvent, parseTerminalServerEvent } from './socket-events';

describe('parseServerEvent', () => {
  it('reads a known event', () => {
    expect(parseServerEvent('{"type":"orchestrator","busy":true}')).toEqual({ type: 'orchestrator', busy: true });
  });

  it.each(['not json', '42', 'null', '{"busy":true}', '{"type":7}', '{"type":"unknown"}', '{"type":"toString"}'])(
    'drops %j',
    (frame) => {
      expect(parseServerEvent(frame)).toBeNull();
    },
  );
});

describe('parseTerminalServerEvent', () => {
  it('reads output and exit', () => {
    expect(parseTerminalServerEvent('{"type":"output","data":"ls"}')).toEqual({ type: 'output', data: 'ls' });
    expect(parseTerminalServerEvent('{"type":"exit","code":0}')).toEqual({ type: 'exit', code: 0 });
  });

  it('drops an event of the main stream', () => {
    expect(parseTerminalServerEvent('{"type":"reset"}')).toBeNull();
  });
});
