// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

import { beforeEach } from 'vitest';
import { setDaemonConnection } from '@/lib/daemon-connection';
import { showLanguage } from '@/lib/i18n';

export const TEST_DAEMON_URL = 'http://daemon.test';
export const TEST_DAEMON_TOKEN = 'jeton-de-test';

setDaemonConnection({ url: TEST_DAEMON_URL, token: TEST_DAEMON_TOKEN });

// jsdom reports en-US, and the assertions are written against the French texts.
beforeEach(() => showLanguage('fr'));
