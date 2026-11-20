# `mcbench-lite headless` protocol

The command line and the output of each family of `mcbench-lite headless <family>` (the crate's `lib.rs` describes the families and the frame they share). A change to a family's fields is a change to this file and to its client in the private benchmark in the same commit. Every family is listed here.

On Windows `mcbench-lite.exe` is a window program, so `cmd.exe` and PowerShell do not wait for it when a person types `mcbench-lite headless <family> ...`: the prompt comes back at once, the family's output appears after it in the same window, and `%ERRORLEVEL%` or `$LASTEXITCODE` is not its exit status. To wait for it and read its status, type `start /wait mcbench-lite headless <family> ...` in `cmd.exe`, where `> file` and `| program` also work, and `mcbench-lite headless <family> ... | Out-Host` in PowerShell, where `> file` does not wait and leaves the file empty; a program that starts it with pipes (the benchmark, a script) waits for it and reads its output and status as on macOS and Linux.

## `contract`

```
mcbench-lite headless contract
```

It takes no words after the family and reads nothing on stdin. The budgets are this process's `MCBENCH_VERIFY_MAX_*` variables, read as `server/verification-contract.js` `readBudgets` reads them; as with Node, a sweep runs it once per budget regime, with the regime's variables set (the benchmark's tool contract script: standard with none of them set, unrestricted with `LIFTED`).

On success it prints one JSON object on one line on stdout and exits with status 0:

```
{"budgets":<BUDGETS>,"forms":{"updates/image":<form>,"updates/text":<form>,"strict/image":<form>,"strict/text":<form>}}
<form> = {"tools":<tools>,"systemPrompt":"<text>","attempt":{"sha256":"<hex>","tools":"<hex>","budgets":"<hex>","systemPrompt":"<hex>","importOrder":<n>}}
```

| Field | What it is | Today's source |
|---|---|---|
| `budgets` | `BUDGETS`, its keys in its order: `operations`, `simulationTicks`, `wallMs`, `columns`, `tests`, `requestBytes`, `reportBytes` | `verification-contract.js` `BUDGETS` |
| `forms` | the four forms of the regime, in this order: placement mode `updates` then `strict`, each with image input then without; network access off, not a chat, the standard resolution | `tool-contract.js` `regimeAttempts` |
| `tools` | the tool schemas, an array in `toolsFor`'s order | `agent.js` `toolsFor({ imageInput, placementMode, networkAccess: false })` |
| `systemPrompt` | the system prompt | `prompt.js` `systemPrompt({ imageInput, networkAccess: false })` |
| `attempt` | what an attempt records of this form | `tool-contract.js` `attemptContract({ tools, systemPrompt, budgets })` |
| `attempt.sha256` | sha256 of `JSON.stringify({ tools, budgets, systemPrompt, importOrder })` of the four fields below | |
| `attempt.tools` | sha256 of `JSON.stringify(tools)` | |
| `attempt.budgets` | sha256 of `JSON.stringify(BUDGETS)` | |
| `attempt.systemPrompt` | sha256 of the system prompt's text (UTF-8) | |
| `attempt.importOrder` | the import order revision | the benchmark's import order revision, `IMPORT_ORDER` |

The object is written as `JSON.stringify` writes it, so `JSON.stringify(JSON.parse(line).forms[f].tools)` is the exact text `attempt.tools` hashes, and `JSON.stringify(JSON.parse(line).budgets)` the text `attempt.budgets` hashes. The sweep's sha256 (`sweepContract`) is sha256 of `JSON.stringify` of an object mapping `<regime>/<form>` (`standard/updates/image` ... `unrestricted/strict/text`, standard first, each regime's forms in the order above) to each attempt's `sha256`: today `efc4d688d3dc7feff0f437bcd7a3bac8f79a1ddd86e892f26b992002ff755645`.

| Exit status | When | stderr |
|---|---|---|
| 0 | the object was printed | nothing |
| 1 | a budget variable is refused | `mcbench-lite headless contract: <NAME> must be an integer within 1..<maximum>` (`readBudgets`' message; the first refused variable in `BUDGETS`' order) |
| 2 | words follow `contract` | `mcbench-lite headless contract: takes no arguments, not <words>` |

## `grade`

```
mcbench-lite headless grade <tables folder> <verifier root>
```

`test_build`'s engine verifier for the benchmark's graders. It replaces the benchmark's Paper adapter's `createVerifier(root)` (an `EngineVerifier` of `server/minecraft-verify.js`, one lane) and its `verifier.run(world, request, { reportPath })`: a request line is one experiment, its reply is what `run` resolved with or the error it rejected with, and the report file is written where `run` wrote it. `<tables folder>` is Minecraft's tables (`core/minecraft/1.21.10/`: `blocks.txt`, `loot.json`, `datafix.txt`); `<verifier root>` is the verifier's `root` (a report without a `reportPath` goes to `<root>/reports/<uuid>.json`). The budgets are this process's `MCBENCH_VERIFY_MAX_*`, read as `readBudgets` reads them, unless a request gives its own; the browser-to-Minecraft y offset is `MCBENCH_MC_Y_OFFSET` (64 when unset), as today.

Experiments run one at a time, in the order their lines arrive (`createVerifier`'s verifier has one lane); a grader that wants several at once runs several processes, as the sweep runs several grade workers today. Requests may be sent before earlier ones are answered; replies come in the order the experiments end, which is the order they were sent.

A request, one JSON object on one line:

| Field | Required | What it is | Today's source |
|---|---|---|---|
| `id` | yes | a number or a string, repeated in the reply | |
| `world` | yes | `{"manifest": "<file>", "chunks": "<folder>"}`: the world as the store writes it, `JSON.stringify(world.serializeManifest(write))` in the file and each chunk's `JSON.stringify(pairs)` as `<folder>/<hash>.json`, `hash` the first 24 hex digits of its sha256 (the chunk order, the palette order and the revision are the world's own, so the request's columns and the import's commands are today's) | the grader's `World` (`toWorld` and the like) |
| `test` | yes | the experiment, as the grader passes it to `run` (`{...scenario.request, block_entities, ...}`), checked by `validateRequest` with today's error sentences | `verifier.run(world, test, ...)` |
| `placementMode` | no, `updates` | `updates` or `strict` | `verifier.placementMode = ...` |
| `reportEncoding` | no, `full` | `full` or `rle-v1` | `verifier.reportEncoding = ...` |
| `budgets` | no, this process's | an object of numbers in the grader's key order (`verifier.budgets = {...}`); a key it lacks never refuses, as today | `verifier.budgets` |
| `reportPath` | no | where the report is written (the value of `reportFile` in the reply, as given) | `run`'s `reportPath` |

A reply, one JSON object on one line, written as `JSON.stringify` writes it:

```
{"id":<id>,"ok":true,"result":<what run resolved with>}
{"id":<id>,"ok":false,"error":"<error.message>"[,"code":"VERIFICATION_BUDGET_EXCEEDED","budget":<error.budget>]}
```

`result` has today's keys in today's order: the report's own when it is under 10,000 bytes (else `observationsInFile`, `probesInFile`, `inputsInFile`), then `result.json`'s (`engine`, `minecraftVersion`, `placementMode`, `engineSettings`, `physicsGuard`, ..., `ok`, `reportBytes`), `worldRevision`, `reportFile`, `cost`, `budgets`, `wallMs`, `note` and `timings` (`startupMs`, `simulationMs`, `archiveMs`; no `broker`). `code` and `budget` come with a budget error (`VerificationBudgetError`; `WallBudgetError` adds `phase`, `ticksReached` and `totalTicks`). No progress lines are written: no grader reads `onUpdate` for its result. Cancelling an experiment is not part of the family (graders never cancel); closing stdin lets every experiment sent end, then the process exits.

| Exit status | When | stderr |
|---|---|---|
| 0 | stdin ended and every experiment was answered | nothing |
| 1 | a budget variable is refused, or the engine cannot read the tables | `mcbench-lite headless grade: <readBudgets' message, or the engine's>` |
| 2 | the command line is not two folders, a line is not a JSON object, a field is missing or not one of the table's, or a field's value is not of its kind (a world that cannot be read, an unknown placement mode or encoding) | `mcbench-lite headless grade: <what>` |

Equivalence: `mcbench-lite`'s test `headless_grade` sends every grader experiment of the verifier's differential (`mcbench-tools`' `tests/node/verify-cases.js`: the community builds with the benchmark-verification budgets in both encodings, lowered wall-clock, report and operations budgets, a grader's budgets without `tests`) to the built binary and compares each reply and report file with today's `EngineVerifier`'s (`verify-node.js`) by `verify-compare.js`.

## `turn`

```
mcbench-lite headless turn
```

It takes no words after the family. It reads one JSON object per line on stdin and answers each, in order, with one line on stdout: `{"id", "ok": true, ...fields}` or `{"id", "error": "<sentence>"}`. A `turn` writes `{"id", "event": <event>}` lines before its answer, and a `call` writes `{"id", "event": {"type": "tool_progress", "name", "progress"}}` lines. A line that is not a JSON object, an unknown `op`, a missing field or a field the op does not take ends the process with status 2 and a line on stderr. The budgets are the process's `MCBENCH_VERIFY_MAX_*`, the context budget its `MCBENCH_CONTEXT_MAX_BYTES`.

| `op` | Fields | Answer | Today's source |
|---|---|---|---|
| `create` | `data` (a data folder), `provider`, `model` | `chat`: the new chat's id | `store.createConversation` (`attempt-host.js` `start`) |
| `tools` | `placement` (`updates` or `strict`), `imageInput`, `networkAccess`, `chat`, `resolution` | `tools` (the schemas in `toolsFor`'s order, then `prompt_library` for a chat), `systemPrompt` | `agent.js` `toolsFor`, `prompts.js` `TOOL`, `prompt.js` `systemPrompt` |
| `format` | `name`, `result` (a tool's result object) | `content`: the tool message's text | `agent.js` `toolMessageContent` |
| `call` | `data`, `chat`, `name`, `arguments` (an object), `imageInput`, `turnChat` (the prompt library is offered) | `content`, `result`, and `screenshots` (`{text, images}`, image addresses) or `images` when the call made them; the chat is saved | `agent.js` `executeToolCall`, then `store.save` (`attempt-host.js` `execute`) |
| `turn` | `data`, `chat`, `prompt` (the user's text, appended and saved first), `effort` (a string or null), `placement`, `networkAccess`, `turnChat` | `{}` when the model answered, or `failed` (the error's message), `telemetry` (a failed completion's, or null) and `reply` (the text streamed before it failed) | `harnesses/mcbench.js` `run`: `store.appendMessage`, `runTurn` and its events |

Each event is the object `agent.js`'s `emit` passes, with its `type` (`assistant_delta`, `reasoning_delta`, `tool_call_delta`, `harness_status`, `notice`, `plan`, `completion_retry`, `reasoning`, `assistant`, `tool_start`, `tool_progress`, `tool_result`) and its fields, written as `JSON.stringify` writes it (`mcbench_agent::TurnEvent::to_json`, held to Node's lines by `mcbench-agent`'s `replay_diff`). The tools this process uses are `mcbench-tools`' (`Tools::new` over the data folder's store, engine children and build child of this binary on `MCBENCH_TABLES`, the platform's `bash` sandbox, the data folder's `blocks.json`), with `see_build` drawn by the process's one renderer (`src/renderer.rs`: `mcbench_draw::live::Draw` on the hardware adapter, started on the first call that makes the tools, from the shipped data at `MCBENCH_TABLES` and the baked textures at `MCBENCH_TEXTURES`; a `see_build` call before it has started waits for it, and one after a failed start answers `Screenshot initialization: <the renderer's sentence>`). So `MCBENCH_TABLES` names a shipped data folder (`minecraft/<id>` of a data folder, which holds the engine's tables too) and `MCBENCH_TEXTURES` its textures (`textures/<id>`); without `MCBENCH_TEXTURES` a request that makes the tools answers `MCBENCH_TEXTURES names no folder: see_build draws with the baked textures (textures/<id> of a data folder)`. The providers are the data folder's (`src/providers.rs`, the same the app's core makes): an API provider with the key the settings save or its environment variable; `claude-code` and `codex-cli` run the CLI the app runs (`MCBENCH_CLAUDE_BINARY` or `MCBENCH_CODEX_BINARY`, else the program on `PATH`, else the tools folder's, `MCBENCH_TOOLS_DIR` or `<data>/tools`) with the CLI's own sign-in (`CLAUDE_CONFIG_DIR` or the home folder; `CODEX_HOME` or `~/.codex`), refused with today's sentence when it is not signed in or not installed; one of each per data folder for the process. `codex-cli` turns run in Codex's box on Linux (the root filesystem `MCBENCH_SANDBOX_ROOTFS` and `MCBENCH_SANDBOX_ROOTFS_SHA256` name, unpacked into the data folder's `sandbox/`, and the static `mcbench-sandbox-agent` beside this binary or named by `MCBENCH_SANDBOX_AGENT_STATIC`), on the host on macOS, and are refused on Windows (no launch yet, `mcbench-providers`' sentence). `examples/real-turn.sh` runs one real Claude Code turn.

The renderer. `see_build` is drawn by this process's own renderer, started at the first request from the shipped data at `MCBENCH_TABLES` and the baked textures at `MCBENCH_TEXTURES` (`headless setup` makes both and prints them); its start writes one line to stderr, `[renderer] mcbench-draw <version>: <adapter> (<backend>, <device type>, hardware|software)`, or `[renderer] <why it did not start>`. A process without `MCBENCH_TEXTURES` still runs turns and every other tool; a `see_build` call there answers `Screenshot initialization: MCBENCH_TEXTURES names no folder: see_build draws with the baked textures (textures/<id> of a data folder)`.

## `shots`

```
mcbench-lite headless shots
```

`see_build`'s pictures of a world given as files. It takes no words after the family. It starts its renderer at once (as `turn` does: `Draw` on the hardware adapter from the shipped data at `MCBENCH_TABLES` and the textures at `MCBENCH_TEXTURES`), and its engines (the settles) at the first request, as `turn` makes them (`MCBENCH_TABLES`, this program as the engine children, the build child). The budgets are the process's `MCBENCH_VERIFY_MAX_*`. Requests are served one at a time, in order; closing stdin ends the process with status 0.

A request, one JSON object on one line:

| Field | What it is |
|---|---|
| `id` | a number or a string, repeated in the reply |
| `world` | `{"manifest": "<file>", "chunks": "<folder>"}`: the world as `grade` takes it (its settle ticks, when the manifest has them, are the shots' default tick) |
| `arguments` | `see_build`'s arguments (`shots`, `attach`), checked with today's sentences |
| `out` | the folder each shot's JPEG is written to, as `<n>.jpg` from 1 (made when missing) |

A reply, one JSON object on one line, written as `JSON.stringify` writes it:

```
{"id":<id>,"ok":true,"result":<result>}
{"id":<id>,"error":"<sentence>"}
```

`result` is what `executeSeeBuild` answers a model, in today's keys and order (`ok`, `worldRevision`, `minecraft`, `shots`; or `ok: false` and `error`), each shot's `image` being the file written instead of a `/work/images` path; `attach` and the tool message's `note` are not part of it. The settles and the renderer's worlds are kept across requests as one conversation's. `error` is a request that could not be served: a world whose files cannot be read, the engines or a file of `out` that cannot be made.

| Exit status | When | stderr |
|---|---|---|
| 0 | stdin ended | the renderer's one line when it has started: `[renderer] mcbench-draw <version>: <adapter> (<backend>, <device type>, hardware\|software)`, or `[renderer] <why it did not start>` |
| 1 | `MCBENCH_TABLES` or `MCBENCH_TEXTURES` names no folder | `mcbench-lite headless shots: <NAME> names no folder: see_build draws with <what>` |
| 2 | words follow `shots`, a line is not a JSON object, a field is missing or not of its kind, or a request has a field not in the table | `mcbench-lite headless shots: <what>` |

## `setup`

```
mcbench-lite headless setup <data folder>
```

The app's first two setup steps on a data folder, with no window and no display, so a machine that runs the benchmark can make the two folders `turn` and `shots` read: `data`, the shipped Minecraft data unpacked into `<data folder>/minecraft/<id>`, then `textures`, Minecraft's textures fetched from Mojang's client jar and baked into `<data folder>/textures/<id>` (`mcbench_setup::start::data_and_textures`, the two steps `mcbench_setup::start` runs for the app, with the same client). The model's shell step is not run: the sandbox is the app's, and `turn` makes its own. The shipped data is the program's embedded bundle (feature `shipped-data`, on in every release build), unpacked as the app unpacks it. The data folder is the one word after the family (not `MCBENCH_DATA_DIR`), made absolute, and made when missing; its one-instance lock (`server.pid`, `mcbench_setup::InstanceLock`) is held while the steps run, so it never unpacks under a running app. Run again on a folder that has both folders, it fetches nothing and prints the same line. It reads nothing on stdin.

Then the model's shell's files, as the app's setup step fetches them (`mcbench_setup::shell::files`: in a shipped build the pinned root filesystem, and the guest base on macOS and Windows, downloaded once into `<data folder>/sandbox-assets/` and checked against `native/sandbox-assets.lock`), so the machine is ready before its first turn and `turn` fetches nothing; a download that fails fails setup with its sentence. A build with no model's shell leaves that step unavailable, which is not a failure.

On success it prints one JSON object on one line on stdout, written as `JSON.stringify` writes it, and exits with status 0:

```
{"tables":"<data folder>/minecraft/<id>","textures":"<data folder>/textures/<id>"}
```

`tables` is the folder to name as `MCBENCH_TABLES`, `textures` the one to name as `MCBENCH_TEXTURES`. Progress goes to stderr, one line each time a step's state changes: `mcbench-lite headless setup: <title>: <state>`, the title being the step's (`Minecraft's data`, `Minecraft's textures`, `Model's shell`) and the state `running` (with `, <done> of <total> bytes` while the textures download), `done in <seconds> s` or `failed`.

| Exit status | When | stderr |
|---|---|---|
| 0 | both folders are there and the line was printed | the progress lines |
| 1 | a step failed, or another live process holds the data folder | the progress lines, then `mcbench-lite headless setup: <sentence>`: the failed step's sentence as the setup screen shows it (a build without the shipped data: `This build has no shipped Minecraft data: build it with --features shipped-data and MCBENCH_DATA_BUNDLE naming the bundle`), or `<data folder>/server.pid is held by process <pid>, which is still running` |
| 2 | not exactly one word follows `setup` | `mcbench-lite headless setup: usage: mcbench-lite headless setup <data folder>` |
