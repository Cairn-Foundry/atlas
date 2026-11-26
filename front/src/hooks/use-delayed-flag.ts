// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useEffect, useState } from 'react';

/** True only once `value` has stayed true for `delayMs`: avoids flashing transient states. */
export function useDelayedFlag(value: boolean, delayMs: number): boolean {
  const [isShown, setIsShown] = useState(false);

  useEffect(() => {
    if (!value) {
      setIsShown(false);
      return;
    }
    const timer = setTimeout(() => setIsShown(true), delayMs);
    return () => clearTimeout(timer);
  }, [value, delayMs]);

  return value && isShown;
}
