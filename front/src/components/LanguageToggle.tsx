// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { ReactElement } from 'react';
import { cn } from 'cn';
import { setLanguage } from '@/lib/api-client';
import { currentLanguage, messagesFor, showLanguage } from '@/lib/i18n';
import { useLanguage } from '@/hooks/use-messages';

interface LanguageToggleProps {
  className?: string;
}

/** Switches the language of the whole factory; it shows the language in use, and names the other one in its own words. */
export function LanguageToggle({ className }: LanguageToggleProps): ReactElement {
  const language = useLanguage();
  const target = language === 'fr' ? 'en' : 'fr';
  const { code } = messagesFor(language).language;
  const { switchTo } = messagesFor(target).language;

  const switchLanguage = (): void => {
    const previous = currentLanguage();
    showLanguage(target);
    // The daemon pushes its snapshot once it has the new language; if it refuses, the old one comes back.
    setLanguage(target).catch(() => showLanguage(previous));
  };

  return (
    <button
      type="button"
      lang={target}
      aria-label={switchTo}
      title={switchTo}
      className={cn(
        'flex size-10 flex-none items-center justify-center rounded-full text-[12px] font-medium text-subtle transition-colors hover:bg-secondary hover:text-foreground',
        className,
      )}
      onClick={switchLanguage}
    >
      {code}
    </button>
  );
}
