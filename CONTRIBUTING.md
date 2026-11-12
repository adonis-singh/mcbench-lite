# Contributing

Models wrote nearly all of this code, and a model may write your pull request. Run the change before you send it, and say in the pull request what you saw. A pull request that nobody ran gets closed.

Using the app is in the [README](README.md).

## What gets merged and what does not

| Change | Answer | Why |
|---|---|---|
| Something the engine refuses as `unported`, ported and identical to the reference | Yes | The engine then refuses less |
| A bug fix with a test that fails without it | Yes | Without the test I will ask for one |
| A Windows port | Yes, with a real sandbox | I will not write it. It needs a sandbox for build programs (`server/build-sandbox.js`) and a Codex launcher. A port that turns the sandbox off is refused, because a model's program could then read your files |
| An approximation in `core/`: redstone that is close enough, a faster algorithm that changes update order | No | The model would be told its build works when it breaks in the game |
| A fallback, a compatibility branch, a default that hides an error | No | A second path is rarely run, so its bugs are found late |
| A hosted mode, accounts, telemetry | No | It is a local app for one person |
| Minecraft's jars, textures or tables, or anything under `data/` | No | Mojang's files are not ours to distribute: the app downloads the jars and makes the tables on each machine. `data/` is your own chats and keys |

Open an issue before a large change. I will not review a rewrite I did not ask for.

## Rules

The engine behaves as Minecraft does: block behaviour, tick timing, update order, inputs and observations. A change to `core/` is checked against the reference before it is merged (see Checking the engine). Speed work keeps the stock path and is compared with it: a faster way to light the running world was dropped because the light came out different in 1 of 82 builds.

There are no fallbacks. When something is wrong, the code fails with a message that says what is wrong and what to do.

A thing with states has one state field, named states and a table of legal moves, as in `web/share.mjs` and `web/archive.mjs`. Do not add a boolean beside an existing state.

Comments say what the code does and why, in plain words. What changed goes in the commit message.

A change in behaviour comes with a test. A change to the page is looked at in a browser, and the pull request has the screenshot.

## Setting up

Follow the README's Setup first.

| Command | What it does |
|---|---|
| `npm run dev` | Runs the app and restarts the server when `server/` or `web/` changes |
| `npm test` | Runs the tests (`node --test`) |
| `node scripts/prepare-minecraft.js` | Makes Minecraft's tables in `core/minecraft/` without starting the app. Building the engine and running the tests need them |

Some tests are skipped until you give them what they need:

| Tests | Need |
|---|---|
| Schematic export | Python with `nbtlib`: `uv venv .venv && uv pip install --python .venv/bin/python nbtlib` |
| `npm run test:sandbox` | Docker |
| `npm run test:ui`, `npm run test:settle`, `npm run test:entities` | The Chromium headless shell and the first start's downloads. `test:ui` talks to a running app: the one on port 3020 unless `MCBENCH_UI_URL` names another, with `MCBENCH_DATA_DIR` set to that app's data folder (the test sends its token) |
| `npm run test:minecraft` | GraalVM 25 (it runs a real Paper server as the reference) |

Use a data folder of your own for experiments, never the one your own chats are in: `MCBENCH_DATA_DIR=<folder> PORT=3127 node server/index.js`. A data folder belongs to one running server and has its own `settings.json`, saved keys and access token. Keys in `.env` or the shell, and the Claude Code, Codex and Cursor logins, apply to every data folder.

Options the Settings page does not have are environment variables. `.env.example` lists the ones a user would change, with their defaults.

## Where things are

| Folder | What is in it |
|---|---|
| `server/` | The Node server. `index.js` is the HTTP API; `agent.js` the turn loop and the model's tools; `llm.js` the API providers; `claude-code.js`, `codex-cli.js` and `cursor.js` the harnesses; `store.js` the SQLite store; `sandbox.js` and `build-sandbox.js` the sandboxes; `minecraft-verify.js` runs `test_build`; `share.js` publishes builds |
| `web/` | The page: Preact and htm with no build step. `app.js` is the interface; the small `.mjs` files beside it hold its state machines and have tests; `live-world.mjs` runs the engine in the browser; `bluemap-bridge.js` feeds the viewer |
| `core/` | The Rust engine, a reimplementation of Minecraft's block-update code. It builds natively (for `test_build` and screenshots) and to WebAssembly (for the live world in the page) |
| `renderer/` | A Rust port of BlueMap's tile mesher, committed as WebAssembly. Rebuild it with `node scripts/build-renderer.js` |
| `scripts/` | Parity checks, benchmarks and `prepare-minecraft.js` |
| `sandbox/` | The Docker image of the model's shell |
| `test/` | The tests |

In a turn, the model calls `build` with a JavaScript program, which runs in a sandbox against a copy of the chat's voxel world (`server/world.js`). The native engine settles the result as Minecraft would, and the page's own copy of the engine, WebAssembly in a worker, places and ticks it in the 3D viewer. The viewer is BlueMap's web app, drawn from tiles the mesher makes in the browser.

## Checking the engine

A change that must not change a byte (speed work, a refactor) is checked against an earlier build: `node scripts/core-parity.js --fuzz 300 --base <earlier engine binary>` runs 300 random worlds through both and compares every byte. It needs neither GraalVM nor the corpus below.

A port changes bytes on purpose, so it is checked against the Java reference engine (`server/McbenchEngine.java`) instead: the same command without `--base`. The reference runs Minecraft's own code and needs GraalVM 25 under `~/.local/graalvm` or named by `MCBENCH_GRAALVM_HOME`. The app never runs the Java engine.

The 82 community builds the README's numbers come from are not in the repository. Some modes of the parity and benchmark scripts run on a corpus of builds that you supply: set `MCBENCH_CORPUS` to a directory in which every `imports/<build>/artifact.json` is one build. A mode that needs it says so and stops.

Report a vulnerability the way `SECURITY.md` says.
