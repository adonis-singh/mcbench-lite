// Runs the canvas work of a page in the headless shell over the DevTools
// protocol and prints the JSON it returns (argv: shell, page, port).
import { spawn } from "node:child_process";
const [shell, page, port] = process.argv.slice(2);
const dir = new URL(".", import.meta.url).pathname;
const child = spawn(shell, ["--headless", "--no-sandbox", "--allow-file-access-from-files", `--user-data-dir=${dir}profile`, `--remote-debugging-port=${port}`, "about:blank"], { stdio: "ignore", env: { HOME: dir + "home", XDG_RUNTIME_DIR: dir + "rt", PATH: "/usr/bin:/bin" } });
try {
  let targets;
  for (let i = 0; i < 100; i++) { try { targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json(); break; } catch { await new Promise(r => setTimeout(r, 100)); } }
  const t = targets.find(t => t.type === "page");
  const ws = new WebSocket(t.webSocketDebuggerUrl);
  await new Promise(r => ws.onopen = r);
  let id = 0; const pending = new Map();
  ws.onmessage = (m) => { const d = JSON.parse(m.data); if (pending.has(d.id)) { pending.get(d.id)(d); pending.delete(d.id); } };
  const call = (method, params) => new Promise(r => { const i = ++id; pending.set(i, r); ws.send(JSON.stringify({ id: i, method, params })); });
  await call("Page.enable", {});
  await call("Page.navigate", { url: "file://" + page });
  await new Promise(r => setTimeout(r, 1500));
  const res = await call("Runtime.evaluate", { expression: "window.done", awaitPromise: true, returnByValue: true });
  process.stdout.write(JSON.stringify(res.result.result.value ?? res.result));
  ws.close();
} finally { child.kill(); }
