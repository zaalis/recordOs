#!/usr/bin/env node
// Compatibility entry point for the native Record OS installer build.
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
if (process.platform !== 'win32') throw new Error('Build the Windows installer on Windows.');
const script = fileURLToPath(new URL('./build-record-os-installer.ps1', import.meta.url));
const result = spawnSync('powershell.exe', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', script], { stdio: 'inherit' });
process.exit(result.status ?? 1);
