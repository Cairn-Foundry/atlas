// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import { DaemonGate } from './components/DaemonGate';
import { tellShellLanguage } from './lib/daemon-connection';
import { currentLanguage, readLanguage, showLanguage, subscribeLanguage } from './lib/i18n';
import { readTheme, showTheme } from './lib/theme';
import '@fontsource-variable/inter/wght.css';
import '@fontsource-variable/jetbrains-mono/wght.css';
import './index.css';

const root = document.getElementById('root');
if (root === null) {
  throw new Error('Element #root not found');
}
// Shown, not remembered: until the master picks a theme, each launch follows the system.
showTheme(readTheme());
// The startup screen speaks the last language the daemon spoke; the daemon's snapshot takes over after.
showLanguage(readLanguage());
tellShellLanguage(currentLanguage());
subscribeLanguage(() => tellShellLanguage(currentLanguage()));
createRoot(root).render(
  <StrictMode>
    <DaemonGate>
      <App />
    </DaemonGate>
  </StrictMode>,
);
