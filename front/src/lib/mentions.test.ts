// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from 'vitest';
import { makeAgent } from '@/test/fixtures';
import { activeMentionQuery, insertMention, mentionCandidates, mentionedAgentIds, splitMentions } from './mentions';

const toy1 = makeAgent({ id: 'a1', ticketRef: 'TOY-1', title: 'Ajouter une ligne au README' });
const toy12 = makeAgent({ id: 'a12', ticketRef: 'TOY-12', title: 'Export CSV' });
const proj = makeAgent({ id: 'p7', ticketRef: 'PROJ-7', title: 'Relances par e-mail' });
const agents = [toy12, proj, toy1];

describe('activeMentionQuery', () => {
  it('finds the mention typed at the caret', () => {
    expect(activeMentionQuery('@', 1)).toEqual({ start: 0, query: '' });
    expect(activeMentionQuery('Relance @TO', 11)).toEqual({ start: 8, query: 'TO' });
  });

  it('ignores an @ inside a word, after a space was typed, or before the caret', () => {
    expect(activeMentionQuery('mail@to', 7)).toBeNull();
    expect(activeMentionQuery('@TOY-1 et', 9)).toBeNull();
    expect(activeMentionQuery('a @TO b', 1)).toBeNull();
  });
});

describe('mentionCandidates', () => {
  it('matches ticket ref or title, case-insensitive, ref prefixes first', () => {
    expect(mentionCandidates(agents, 'to').map((agent) => agent.ticketRef)).toEqual(['TOY-1', 'TOY-12']);
    expect(mentionCandidates(agents, 'e-mail').map((agent) => agent.ticketRef)).toEqual(['PROJ-7']);
    expect(mentionCandidates(agents, '')).toHaveLength(3);
    expect(mentionCandidates(agents, '', 2)).toHaveLength(2);
    expect(mentionCandidates(agents, 'zzz')).toEqual([]);
  });
});

describe('insertMention', () => {
  it('replaces the typed query with the ticket ref and a space', () => {
    expect(insertMention('Relance @TO', { start: 8, query: 'TO' }, 11, 'TOY-1')).toEqual({ text: 'Relance @TOY-1 ', caret: 15 });
  });

  it('keeps the text after the caret without doubling spaces', () => {
    expect(insertMention('@pr merci', { start: 0, query: 'pr' }, 3, 'PROJ-7')).toEqual({ text: '@PROJ-7 merci', caret: 8 });
  });

  it('keeps the line break that follows the mention', () => {
    expect(insertMention('@pr\nsuite', { start: 0, query: 'pr' }, 3, 'PROJ-7')).toEqual({ text: '@PROJ-7 \nsuite', caret: 8 });
  });
});

describe('mentionedAgentIds', () => {
  it('returns the agents whose token is still in the text, without prefix collisions', () => {
    expect(mentionedAgentIds('Relance @TOY-12, puis @proj-7.', agents)).toEqual(['a12', 'p7']);
    expect(mentionedAgentIds('@TOY-1 et @TOY-1', agents)).toEqual(['a1']);
    expect(mentionedAgentIds('mail@TOY-1 TOY-1', agents)).toEqual([]);
  });
});

describe('splitMentions', () => {
  it('splits known mentions out of the text', () => {
    expect(splitMentions('Relance @TOY-1, merci', agents)).toEqual([
      { type: 'text', value: 'Relance ' },
      { type: 'mention', agent: toy1, value: '@TOY-1' },
      { type: 'text', value: ', merci' },
    ]);
  });

  it('leaves unknown mentions and plain text alone', () => {
    expect(splitMentions('@INCONNU-3 et rien', agents)).toEqual([{ type: 'text', value: '@INCONNU-3 et rien' }]);
    expect(splitMentions('', agents)).toEqual([]);
  });
});
