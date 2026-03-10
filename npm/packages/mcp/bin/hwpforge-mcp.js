#!/usr/bin/env node
"use strict";

const { execFileSync } = require("child_process");
const os = require("os");

const PLATFORMS = {
  "darwin-arm64": "@hwpforge/mcp-darwin-arm64",
  "darwin-x64": "@hwpforge/mcp-darwin-x64",
  "linux-x64": "@hwpforge/mcp-linux-x64",
  "linux-arm64": "@hwpforge/mcp-linux-arm64",
  "win32-x64": "@hwpforge/mcp-win32-x64",
};

const platform = os.platform();
const arch = os.arch();
const key = `${platform}-${arch}`;
const pkg = PLATFORMS[key];

if (!pkg) {
  console.error(
    `Error: @hwpforge/mcp does not support ${platform}-${arch} yet.\n` +
    `Supported platforms: ${Object.keys(PLATFORMS).join(", ")}\n\n` +
    `Alternative: cargo install hwpforge-bindings-mcp`
  );
  process.exit(1);
}

const ext = platform === "win32" ? ".exe" : "";
let binPath;
try {
  binPath = require.resolve(`${pkg}/bin/hwpforge-mcp${ext}`);
} catch {
  console.error(
    `Error: Platform package ${pkg} is not installed.\n` +
    `Try: npm install @hwpforge/mcp --force\n\n` +
    `Alternative: cargo install hwpforge-bindings-mcp`
  );
  process.exit(1);
}

try {
  execFileSync(binPath, process.argv.slice(2), { stdio: "inherit" });
} catch (e) {
  if (e.status !== null) process.exit(e.status);
  throw e;
}
