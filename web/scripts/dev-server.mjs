import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const web = path.resolve(here, "..");
const root = path.resolve(web, "..");
const home = await mkdtemp(path.join(tmpdir(), "boggled-e2e-"));
const port = process.env.BOGGLED_PORT || "4173";
const sessionFile = path.join(web, ".e2e-session.json");
if (process.env.BSS_MSE_REFERENCE) {
  await writeFile(path.join(home, "config.json"), JSON.stringify({ pack_path: path.resolve(process.env.BSS_MSE_REFERENCE) }));
}

console.log(`boggled home ${home}`);
const server = spawn("cargo", ["run", "-p", "boggled", "--offline", "--", "serve"], {
  cwd: root,
  env: {
    ...process.env,
    BOGGLED_HOME: home,
    BOGGLED_PORT: String(port),
    BOGGLED_IDLE_SECS: "0",
  },
  stdio: "inherit",
  windowsHide: true,
});

const serverJson = path.join(home, "server.json");
let info = null;
for (let attempt = 0; attempt < 240; attempt += 1) {
  if (server.exitCode != null) {
    console.error(`boggled serve exited with ${server.exitCode}`);
    process.exit(1);
  }
  if (existsSync(serverJson)) {
    info = JSON.parse(await readFile(serverJson, "utf8"));
    if (info.token && info.port) break;
  }
  await delay(500);
}
if (!info?.token) {
  console.error("server.json did not appear");
  shutdown();
  process.exit(1);
}

await writeFile(
  sessionFile,
  JSON.stringify({ home, port: Number(info.port), token: info.token, url: "" }),
);

console.log(`boggled listening on ${info.port}`);
const viteBin = path.join(web, "node_modules", "vite", "bin", "vite.js");
const vite = spawn(process.execPath, [viteBin, "--host", "127.0.0.1", "--port", "5173", "--strictPort"], {
  cwd: web,
  env: { ...process.env, BOGGLED_API: `http://127.0.0.1:${info.port}` },
  stdio: "inherit",
  windowsHide: true,
});

let stopping = false;
function shutdown() {
  if (stopping) return;
  stopping = true;
  for (const child of [vite, server]) {
    if (!child.pid) continue;
    if (process.platform === "win32") {
      spawn("taskkill", ["/PID", String(child.pid), "/T", "/F"], {
        stdio: "ignore",
        shell: true,
        windowsHide: true,
      });
    } else {
      child.kill("SIGTERM");
    }
  }
}

process.on("exit", shutdown);
process.on("SIGINT", () => {
  shutdown();
  process.exit(0);
});
process.on("SIGTERM", () => {
  shutdown();
  process.exit(0);
});

vite.on("exit", (code) => {
  shutdown();
  process.exit(code ?? 0);
});
server.on("exit", (code) => {
  if (stopping) return;
  console.error(`boggled serve exited with ${code}`);
  shutdown();
  process.exit(code ?? 1);
});
