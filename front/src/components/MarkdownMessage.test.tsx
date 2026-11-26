// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { makeAgent } from '@/test/fixtures';
import { MarkdownMessage } from './MarkdownMessage';

afterEach(cleanup);

function renderMarkdown(text: string, onOpenAgent = vi.fn()) {
  const view = render(<MarkdownMessage text={text} agents={[makeAgent({ id: 'a1', ticketRef: 'TOY-1' })]} onOpenAgent={onOpenAgent} />);
  return { ...view, onOpenAgent };
}

describe('MarkdownMessage', () => {
  it('renders bold, lists and inline code instead of raw markers', () => {
    const { container } = renderMarkdown('Je gère **un seul projet** :\n\n- **toy2**\n  - Dépôt : `/code/toy2`\n- docs');
    expect(container.querySelector('strong')?.textContent).toBe('un seul projet');
    expect(container.querySelectorAll('.atlas-markdown > ul > li')).toHaveLength(2);
    expect(container.querySelectorAll('li ul > li')).toHaveLength(1);
    expect(container.querySelector('code')?.textContent).toBe('/code/toy2');
    expect(container.textContent).not.toContain('**');
  });

  it('opens links in a new tab without giving the page access to the opener', () => {
    renderMarkdown('La [MR](https://gitlab.example.com/mr/1) est ouverte.');
    const link = screen.getByRole('link', { name: 'MR' });
    expect(link.getAttribute('target')).toBe('_blank');
    expect(link.getAttribute('rel')).toBe('noopener noreferrer');
  });

  it('keeps its elements when the factory sends new agents, so a selection survives', () => {
    const { container, rerender } = renderMarkdown('Voir **@TOY-1** ici.');
    const paragraph = container.querySelector('p');
    rerender(<MarkdownMessage text="Voir **@TOY-1** ici." agents={[makeAgent({ id: 'a1', ticketRef: 'TOY-1' })]} onOpenAgent={vi.fn()} />);
    expect(container.querySelector('p')).toBe(paragraph);
    expect(screen.getByRole('button', { name: /TOY-1/ })).toBeTruthy();
  });

  it('ignores raw HTML', () => {
    const { container } = renderMarkdown('Avant <img src=x onerror="alert(1)"> après');
    expect(container.querySelector('img')).toBeNull();
  });

  it('keeps @mentions clickable inside Markdown, but not inside code', () => {
    const { onOpenAgent, container } = renderMarkdown('- **@TOY-1** attend\n\n`@TOY-1` reste du code');
    fireEvent.click(screen.getByRole('button', { name: '@TOY-1' }));
    expect(onOpenAgent).toHaveBeenCalledWith('a1');
    expect(screen.getAllByRole('button')).toHaveLength(1);
    expect(container.querySelector('code')?.textContent).toBe('@TOY-1');
  });

  it('turns headings into small section titles', () => {
    const { container } = renderMarkdown('# Projets\n\ntexte');
    expect(container.querySelector('h1')).toBeNull();
    expect(container.querySelector('h3')?.textContent).toBe('Projets');
  });
});
