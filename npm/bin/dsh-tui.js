#!/usr/bin/env node
'use strict';

const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

const platformKey = process.platform + '-' + process.arch;
const vendorDir = path.join(__dirname, '..', 'vendor');
const binaryName = process.platform === 'win32' ? 'dsh-tui.exe' : 'dsh-tui';
const binaryPath = path.join(vendorDir, platformKey, binaryName);

if (!fs.existsSync(binaryPath)) {
  let available = [];
  try {
    available = fs
      .readdirSync(vendorDir, { withFileTypes: true })
      .filter((e) => e.isDirectory())
      .map((e) => e.name);
  } catch (_) {
    // vendor dir missing entirely
  }
  console.error(
    'dsh-tui: no bundled binary for this platform (' + platformKey + ').'
  );
  if (available.length > 0) {
    console.error('Bundled platforms: ' + available.join(', '));
  } else {
    console.error('No platform binaries are bundled in this install.');
  }
  console.error('Hint: run scripts/build-npm.sh on this machine to build and package a native binary.');
  process.exit(1);
}

const result = spawnSync(binaryPath, process.argv.slice(2), { stdio: 'inherit' });

process.exit(
  result.status ??
    (result.signal === 'SIGINT' ? 130 : result.signal === 'SIGTERM' ? 143 : 1)
);
