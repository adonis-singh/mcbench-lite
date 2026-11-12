# mcbench-lite

A local web app in which AI models build in Minecraft. You chat with a model, it builds in a Minecraft world shown in a 3D viewer beside the chat, and follow-ups change the same world. Bench mode gives a prompt to several models at once so you can compare their builds and vote on them.

![The app: the chat on the left, the model's build in a 3D Minecraft world on the right](docs/screenshot.jpg)

## How it differs from MC-Bench

In [MC-Bench](https://github.com/mc-bench) a model gets a prompt and writes one program in a single response, and the result is what people vote on. Here the model works as an agent. I have given it tools, and it uses them in a loop until it decides the build is done.

| Tool | Utility |
|---|---|
| `build` | Runs a JavaScript program against the world |
| `see_build` | Takes screenshots of the build and looks at them |
| `test_build` | Runs the build in a Minecraft engine to check redstone and other behaviour |
| `bash` | Runs scripts and calculations in a sandboxed shell |

It also runs on your own machine, with the models and harnesses you connect.

## Why an agent?

A single response shows what a model can write without seeing the result. Models are now, by far, mainly used as agents. They can act, look at what happened and correct it. A build has to be looked at to be judged, so here the model can look at it too, and test it. I've found this reflects model capability much more realistically.

Everything below has been written by Claude and reviewed by me.

## The engine

The engine in `core/` is a Rust reimplementation of Minecraft 1.21.10's block update code: redstone, pistons, hoppers, water and lava, light, TNT. On 82 community builds and 700 random worlds, every reply and file it writes is byte for byte what a reference engine built from Minecraft's own server classes writes. On the same 82 builds, that reference engine's test reports match a real Paper server's. What the engine does not model fails with `unported: …` and is never approximated. Mobs are not modelled. Random ticks, weather and the day cycle are off, so crops do not grow and it is always noon.

A faster way to light the running world was written and not shipped, because the light came out different in 1 of 82 builds.

## Network use

There is no hosted version and no telemetry. The app contacts the providers you connect, and openrouter.ai for its public list of models. On the first start it downloads from rust-lang.org (through rustup), from Paper and Mojang, and from GitHub (BlueMap and misode/mcmeta). It contacts Docker Hub, PyPI and Debian when it builds the shell's image, and GitHub and your Pages site when you share a build. Claude Code is started with its non-essential traffic off. The Cursor SDK ships with its own analytics code, which the app neither uses nor turns off.

## What you need

The app runs on macOS and Linux (other platforms are refused at startup). You can bully Claude to get it to work for Windows and submit a PR; I have no intention of doing so myself.

| What | Why | How to get it |
|---|---|---|
| Node.js 22.13 or newer (up to 26) | runs the app | [nodejs.org](https://nodejs.org) or `brew install node` |
| rustup | builds the Minecraft engine, natively and as WebAssembly for the browser; it installs the pinned Rust (1.99.1, `core/rust-toolchain.toml`) and its WebAssembly target by itself on the first start | [rustup.rs](https://rustup.rs) (Homebrew's `rust` package is not enough: it has no rustup) |
| A JDK, Java 25 or newer (GraalVM is not needed) | reads Minecraft's tables from the server jar and makes the 3D viewer's assets, once | [adoptium.net](https://adoptium.net), or `brew install --cask temurin`; `java -version` and `javac -version` must work |
| Docker | the model's bash shell runs in a container, and so does Codex CLI on Linux | [Docker Desktop](https://www.docker.com/products/docker-desktop/) on a Mac, Docker Engine on Linux |
| Chromium headless shell (about 95 MB download) | renders the model's `see_build` screenshots | `npx playwright-core install --only-shell chromium` (Setup below) |
| git and gh, signed in | only for Share | `brew install git gh`, then `gh auth login` |

The app starts without Docker, Chromium, git or gh and says what is missing when it is first needed. Without rustup or a JDK it starts, and the terminal and the 3D viewer's pane say which one to install.

On Linux, build programs also need bubblewrap at `/usr/bin/bwrap`, `prlimit` from util-linux and unprivileged user namespaces (see `server/build-sandbox.js`). On macOS they run under the built-in `sandbox-exec`.

## Setup

```bash
git clone https://github.com/adonis-singh/mcbench-lite.git
cd mcbench-lite
npm install
npx playwright-core install --only-shell chromium
install -m 600 .env.example .env    # optional: API keys (or add them later in Settings)
npm start
```

Open the address the server prints, `http://mcbench-<label>.localhost:<port>/?token=…` (`npm run url` prints it again). The token is made once per data folder (`data/auth-token`, readable by your account alone) and signs the browser in, so no other account or process on the machine can use the app. The port is 3020 unless `PORT` says otherwise.

## The first start

The page opens at once; the first start then prepares the rest in the background, and the terminal says what it is doing, one line per step, and `[startup] ready` when everything is prepared. With a fast connection it takes about two minutes on a recent Mac, most of it compiling the engine. Each step needs to happen only once, so later starts should take seconds.

| Step | Download | Where it goes |
|---|---|---|
| Rust 1.99.1 and its WebAssembly target, through rustup | about 550 MB on disk | `~/.rustup` |
| Paper 1.21.10, and through it the Minecraft server and its libraries from Mojang | about 55 MB, then about 135 MB | `data/minecraft-live/paper` |
| Minecraft's block, loot and data fixer tables, read from that server jar | none | `core/minecraft/1.21.10` |
| The Rust engine, compiled natively and to WebAssembly with those tables | none | `core/target` |
| BlueMap 5.22's command-line tool | about 7 MB | `data/minecraft-live/bluemap` |
| The Minecraft 1.21.10 client jar from Mojang (the viewer's textures and models) | about 31 MB | `data/minecraft-live/bluemap/data` |
| Minecraft's block data from misode/mcmeta | about 4 MB | `data/assets` |

The sandbox's Docker image is built the first time a model runs bash (a few minutes).

The repository does not contain Minecraft code, jars, textures or data tables. The engine is a reimplementation that is built with Minecraft's tables (every block, state, collision shape, item and loot table, and the data fixer's rules), and the app reads those tables on your machine from the server jar it downloaded, with the project's own Java code, into `core/minecraft/1.21.10/`, which git ignores.

The app downloads the Minecraft server (through Paper) and the Minecraft client jar from Mojang's servers to your machine, and accepts the Minecraft End User Licence Agreement on your behalf: it writes `eula=true` for Paper and sets BlueMap's `accept-download: true`. Read the EULA at https://www.minecraft.net/eula; by running the app you agree to it.

## Connecting an AI

While no AI is connected, the first screen has a **Connect an AI** button. Otherwise open Settings (the gear at the bottom left) and choose Connect an AI. Then pick a model with the model button in the prompt box; typing in its search box searches every connected provider. A provider needs one of these:

| Provider | How to connect |
|---|---|
| OpenRouter, OpenAI, Anthropic, Groq, Google, QwenCloud, Cursor | an API key, in Settings, in `.env` or exported in the shell that starts the app (`OPENROUTER_API_KEY`, `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GROQ_API_KEY`, `GOOGLE_API_KEY`, `QWENCLOUD_API_KEY`, `CURSOR_API_KEY`); Cursor can sign in with `npm run cursor:login` instead |
| Claude Code (your Claude subscription) | install the `claude` CLI and sign in once with `claude auth login` |
| Codex CLI (your ChatGPT subscription, Codex's own agent) | install the `codex` CLI and sign in once with `codex login` |
| ChatGPT (your ChatGPT subscription, mcbench's own agent) | sign in once with `npm run codex:login` |

ChatGPT sign-in uses the Codex CLI login in `$CODEX_HOME/auth.json` (default `~/.codex/auth.json`, made by `npm run codex:login`); the server uses that login directly, in its own process, so nothing listens on a port for it. Settings shows which logins are present. Keys are kept in `data/settings.json` and never sent to the browser.

An API key is billed by its provider. The sign-ins use your subscription and count against its limits; the app shows the plan's limit warning when the provider sends one.

## Using it

The app has two modes, Build and Bench, and its logo reads "mcbuild" in Build and "mcbench" in Bench.

**Build** (⌘1) is a chat: ask for a build ("a cozy medieval house"), and the model writes build programs, looks at its work and tests redstone; the build appears in the live world beside the chat, where you can pause, step, or fly around. A click uses a lever, button, door, trapdoor, fence gate, repeater, comparator, note block, daylight detector, redstone dust or decorated pot, as in Minecraft. Player mode puts you in the world as a creative Minecraft player who can walk and fly. Follow-ups edit the same world, and "Edit and branch" on a message goes back to it. Select blocks in the viewer (Alt+S) to refer to them in your next message. A message can also carry up to 8 attached files: images are shown to the model when it accepts images, and every file is in its shell's `/work/attachments`.

**Bench** (⌘2) sends one prompt to 2 to 8 models at once and shows their builds side by side in one world; follow-ups go to all of them as rounds. One model can run at several reasoning efforts in the same bench. With **Blind** on, the builds are named A, B, C until you reveal the models: vote for the best build of each round, or a tie. The Leaderboard is an Elo rating over the blind votes in benches that are not archived: every model starts at 1000, the picked build beats each other build of its round (K = 24), a tie is a draw between all of them, and a model at two efforts has two entries. The Leaderboard is local: it holds your own votes on your own prompts.

The **prompt library** is in Bench: saved prompts with tags, in which `{{fill-ins}}` are parts you fill in each time. Type "/" in the prompt box to insert one. "Improve with AI" rewrites a prompt with a model you choose, and "Run bench" starts a new bench with the prompt in the prompt box.

The **resolution** picker in the prompt box asks for larger, more detailed builds: Standard (1×), High (2×), Very high (5×) or Maximum (10×). Your last choice is kept.

**Share** publishes a chat's build, or a bench's builds, as a public page to a GitHub Pages repository you name in Settings (`owner/name`), with `git` and `gh`. The page is a static copy of the 3D viewer, in which anyone can fly or orbit around the build as it was when published. A bench's page has a button per model. On X the link shows as a card that plays the viewer, and the share box has a "Post on X" button. Settings lists what you have published and can remove any of it. A shared build's public page includes Minecraft's block textures, made from that client jar, as a BlueMap map does.

The viewer's toolbar also downloads a build as a WorldEdit schematic (`.schem`) or as a Minecraft world to open in the game. ⌘K searches every chat and bench.

## What the model can do on your machine

The model works through the app's tools, and each is confined:

| Tool | Where it runs | What it can reach |
|---|---|---|
| Build programs (JavaScript) | A sandbox that can open no file and has no network: `sandbox-exec` on macOS, bubblewrap on Linux | The chat's world, and files in that chat's own folder, which the server reads and writes for it |
| `bash` | A Docker container | That chat's own folder. No network, except one-shot commands the model asks for. Those reach the public internet with that folder mounted, and cannot reach your machine or your local network |
| Claude Code and Codex CLI | On your machine, as the signed-in CLI (Codex in a container on Linux) | Only mcbench's tools. Claude Code's own file and shell tools are off; Codex runs read-only, and its turn is ended if it uses a tool of its own |

Your provider keys and sign-in tokens are never given to the model. `SECURITY.md` says what the app defends against and what it does not.

## Licence

mcbench-lite's code is under the MIT licence (`LICENSE`): anyone may use, change and redistribute it, as long as they keep the licence notice, which names this repository. `NOTICE.md` lists what is not covered (BlueMap's material and the ports of Java's math) and says what the app downloads from Mojang.

Not an official Minecraft product. Not approved by or associated with Mojang or Microsoft.

## Contributing

[CONTRIBUTING.md](CONTRIBUTING.md) has what you need to work on the code: the commands, the rules, where things are and how to check the engine against Minecraft.
