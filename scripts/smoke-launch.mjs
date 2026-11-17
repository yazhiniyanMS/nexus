// Starts a built NEXUS binary with a throwaway data folder and checks that it stays up and
// creates its database. This proves the app launches; it does not look at the window.
// Usage: node scripts/smoke-launch.mjs <path-to-binary>
import { spawn } from "node:child_process";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const binary = process.argv[2];
if (!binary || !existsSync(binary)) {
  console.error(`Binary not found: ${binary}`);
  process.exit(2);
}
const data = mkdtempSync(join(tmpdir(), "nexus-smoke-"));
const child = spawn(binary, [], { env: { ...process.env, NEXUS_DATA_DIR: data }, stdio: ["ignore", "inherit", "inherit"] });
let exited = null;
child.on("exit", (code, signal) => (exited = { code, signal }));
child.on("error", (e) => (exited = { code: -1, signal: String(e) }));

const started = Date.now();
const db = join(data, "nexus.db");
const timer = setInterval(() => {
  const elapsed = (Date.now() - started) / 1000;
  const finish = (ok, message) => {
    clearInterval(timer);
    console.log(message);
    if (!exited) child.kill();
    setTimeout(() => {
      try {
        rmSync(data, { recursive: true, force: true });
      } catch {
        /* the app may still be releasing files */
      }
      process.exit(ok ? 0 : 1);
    }, 1500);
  };
  if (exited) return finish(false, `FAIL: the app exited after ${elapsed.toFixed(1)}s (${JSON.stringify(exited)})`);
  // Up for 8 seconds with a database on disk: it started and ran its migrations.
  if (existsSync(db) && elapsed > 8) return finish(true, `OK: running after ${elapsed.toFixed(1)}s, database created at ${db}`);
  if (elapsed > 60) return finish(false, "FAIL: no database after 60s");
}, 500);
