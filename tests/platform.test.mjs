import assert from "node:assert/strict";
import test from "node:test";
import { baseName, shortcut } from "../src/lib/platform.ts";

test("file names are found in Unix and Windows paths", () => {
  assert.equal(baseName("/bin/zsh"), "zsh");
  assert.equal(baseName("C:\\Program Files\\PowerShell\\7\\pwsh.exe"), "pwsh.exe");
  assert.equal(baseName("cmd.exe"), "cmd.exe");
});

test("shortcuts are written for the current system", () => {
  assert.match(shortcut("K"), /^(⌘K|Ctrl\+K)$/);
});
