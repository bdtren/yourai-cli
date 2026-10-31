#!/usr/bin/env node
// Launcher: runs the prebuilt yourai binary from the matching platform package.
const { spawnSync } = require("child_process");

const platforms = {
  "darwin-arm64": "@yourai/cli-darwin-arm64",
  "darwin-x64": "@yourai/cli-darwin-x64",
  "linux-x64": "@yourai/cli-linux-x64",
  "linux-arm64": "@yourai/cli-linux-arm64",
  "win32-x64": "@yourai/cli-win32-x64",
  "win32-arm64": "@yourai/cli-win32-arm64",
};

const key = `${process.platform}-${process.arch}`;
const pkg = platforms[key];
if (!pkg) {
  console.error(`yourai: unsupported platform ${key}`);
  process.exit(1);
}

const exe = process.platform === "win32" ? "yourai.exe" : "yourai";
let binary;
try {
  binary = require.resolve(`${pkg}/bin/${exe}`);
} catch {
  console.error(
    `yourai: the platform package ${pkg} is missing.\n` +
      `Reinstall without --omit=optional, or use the curl/PowerShell installer:\n` +
      `  https://yourai.bdtren.com/cli/install.sh`
  );
  process.exit(1);
}

const result = spawnSync(binary, process.argv.slice(2), { stdio: "inherit" });
if (result.error) {
  console.error(`yourai: ${result.error.message}`);
  process.exit(1);
}
if (result.signal) {
  process.kill(process.pid, result.signal);
}
process.exit(result.status ?? 1);
