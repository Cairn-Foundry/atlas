// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

// Builds the daemon the desktop shell ships and copies it where Tauri expects a sidecar
// (binaries/<name>-<target triple>).
// Usage: node scripts/prepare-sidecar.mjs [--release]
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync, renameSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const frontDir = dirname(dirname(fileURLToPath(import.meta.url)));
const cratesDir = join(frontDir, '..', 'crates');
// `tauri build --debug` still runs the build command: the daemon then follows the shell and stays a debug one.
const isRelease = process.argv.includes('--release') && process.env.TAURI_ENV_DEBUG !== 'true';

function run(command, args) {
  execFileSync(command, args, { cwd: cratesDir, stdio: 'inherit', shell: process.platform === 'win32' });
}

function hostTriple() {
  const version = execFileSync('rustc', ['-vV'], { encoding: 'utf8' });
  const host = version.split('\n').find((line) => line.startsWith('host: '));
  if (!host) throw new Error('rustc -vV ne donne pas la cible de la machine');
  return host.slice('host: '.length).trim();
}

const triple = process.env.TAURI_ENV_TARGET_TRIPLE ?? hostTriple();
if (triple === 'universal-apple-darwin') {
  throw new Error('Un binaire universel n\'est pas pris en charge : construire aarch64-apple-darwin et x86_64-apple-darwin séparément');
}
const extension = triple.includes('windows') ? '.exe' : '';

run('cargo', ['build', '-p', 'atlas-daemon', '--target', triple, ...(isRelease ? ['--release'] : [])]);

const built = join(cratesDir, 'target', triple, isRelease ? 'release' : 'debug', `atlas-daemon${extension}`);
const sidecar = join(cratesDir, 'atlas-desktop', 'binaries', `atlas-daemon-${triple}${extension}`);
mkdirSync(dirname(sidecar), { recursive: true });
// Copied aside then renamed: a daemon still running from the previous copy keeps its own file.
const staging = `${sidecar}.tmp`;
copyFileSync(built, staging);
renameSync(staging, sidecar);
console.log(`[atlas] daemon prêt : ${sidecar}`);
