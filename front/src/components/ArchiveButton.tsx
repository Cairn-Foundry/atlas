// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { ReactElement } from 'react';
import { archiveAgent } from '@/lib/api-client';
import { useMessages } from '@/hooks/use-messages';
import { ApiActionButton, type ButtonVariant } from './ApiActionButton';

interface ArchiveButtonProps {
  agentId: string;
  variant: ButtonVariant;
  className: string;
}

/** Takes a finished agent off the board; the board drops it when the daemon announces it. */
export function ArchiveButton({ agentId, variant, className }: ArchiveButtonProps): ReactElement {
  const messages = useMessages();
  return <ApiActionButton label={messages.actions.archive} variant={variant} className={className} isSingleUse run={() => archiveAgent(agentId)} />;
}
