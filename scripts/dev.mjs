import { spawn } from "node:child_process";
import net from "node:net";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.dirname(fileURLToPath(import.meta.url));
const site = path.join(root, "..");
const backend = path.join(site, "backend");

const UI_PORT_START = 3001;
const UI_PORT_TIMEOUT_MS = 30_000;

/** Free only when both stacks are free — Next and Node disagree otherwise. */
function portFree(port, host) {
  return new Promise((resolve) => {
    const probe = net.createServer();
    probe.once("error", () => resolve(false));
    probe.once("listening", () => probe.close(() => resolve(true)));
    probe.listen(port, host);
  });
}

async function firstFreePort(start) {
  for (let port = start; port < start + 50; port += 1) {
    if ((await portFree(port, "127.0.0.1")) && (await portFree(port, "::"))) {
      return port;
    }
  }
  return start;
}

function run(name, command, args, cwd, env, stdio = "inherit") {
  // npx and cargo are .cmd shims on Windows; spawn cannot resolve them without a shell.
  const child = spawn(command, args, {
    cwd,
    env,
    stdio,
    shell: process.platform === "win32",
  });
  child.on("exit", (code, signal) => {
    if (signal) {
      process.exit(1);
    }
    if (code && code !== 0) {
      console.error(`${name} exited with ${code}`);
      process.exit(code);
    }
  });
  return child;
}

// Next quietly moves to the next free port when the one it was given is taken.
// The backend's proxy target used to be a hardcoded 3001, so whenever another
// project held 3001 the backend proxied to *that* project and every page came
// back 500. Ask Next for a port, then read back the port it actually bound and
// point the backend at that.
const explicitOrigin = process.env.CUBIC_UI_ORIGIN;
const requestedPort = explicitOrigin
  ? Number(new URL(explicitOrigin).port) || UI_PORT_START
  : await firstFreePort(UI_PORT_START);

const web = run(
  "web",
  "npx",
  ["next", "dev", "--turbopack", "--port", String(requestedPort), "--hostname", "127.0.0.1"],
  site,
  process.env,
  ["inherit", "pipe", "inherit"]
);

/** Resolves with the port Next reports, or the requested one if it never says. */
function actualUiPort() {
  return new Promise((resolve) => {
    let settled = false;
    const done = (port) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve(port);
    };
    const timer = setTimeout(() => done(requestedPort), UI_PORT_TIMEOUT_MS);

    web.stdout.on("data", (chunk) => {
      const text = chunk.toString();
      process.stdout.write(text);
      if (settled) return;
      const match = text.match(/https?:\/\/[^\s]*?:(\d{2,5})\b/);
      if (match) done(Number(match[1]));
    });
  });
}

const uiPort = await actualUiPort();
const uiOrigin = explicitOrigin
  ? `${new URL(explicitOrigin).protocol}//${new URL(explicitOrigin).hostname}:${uiPort}`
  : `http://127.0.0.1:${uiPort}`;

if (uiPort !== requestedPort) {
  console.log(`[dev] port ${requestedPort} was taken — Next bound ${uiPort}`);
}

const env = {
  ...process.env,
  AUTH_SITE_URL: process.env.AUTH_SITE_URL || "http://localhost:3000",
  RUST_BACKEND_PORT: process.env.RUST_BACKEND_PORT || "3000",
  CUBIC_UI_ORIGIN: uiOrigin,
};

console.log(`[dev] backend :${env.RUST_BACKEND_PORT}  ->  UI proxy ${uiOrigin}`);

const rust = run("backend", "cargo", ["run"], backend, env);

function shutdown() {
  rust.kill("SIGTERM");
  web.kill("SIGTERM");
}

process.on("SIGINT", shutdown);
process.on("SIGTERM", shutdown);
