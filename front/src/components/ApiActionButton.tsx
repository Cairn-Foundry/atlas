// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useState, type ReactElement } from 'react';
import { useMessages } from '@/hooks/use-messages';
import { Button } from '@/components/ui/button';

export type ButtonVariant = 'ghost' | 'outline';

interface ApiActionButtonProps {
  label: string;
  run: () => Promise<void>;
  variant: ButtonVariant;
  className?: string | undefined;
  /** Stays disabled once the call has succeeded: the thing it acts on is about to go away. */
  isSingleUse?: boolean;
}

/** A button that calls the API and says in place when the call failed, so the user can retry. */
export function ApiActionButton({ label, run, variant, className, isSingleUse = false }: ApiActionButtonProps): ReactElement {
  const messages = useMessages();
  const [hasFailed, setHasFailed] = useState(false);
  const [isDone, setIsDone] = useState(false);
  const [isPending, setIsPending] = useState(false);

  const click = (): void => {
    setHasFailed(false);
    // A second click while the first call runs would be refused, and read as a failure of the first.
    setIsPending(true);
    run()
      .then(
        () => setIsDone(isSingleUse),
        () => setHasFailed(true),
      )
      .finally(() => setIsPending(false));
  };

  return (
    <Button variant={variant} className={className} disabled={isDone || isPending} onClick={click}>
      <span aria-live="polite">{hasFailed ? messages.actions.failedRetry : label}</span>
    </Button>
  );
}
