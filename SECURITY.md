# Security

mcbench-lite runs code that a language model wrote, on your machine, next to your API keys.

One person runs the app on their own machine. It listens on 127.0.0.1 only, unless `MCBENCH_BIND` names another address, and holds that person's provider keys, their Claude Code, Codex and Cursor sign-ins, and their chats. It is not built to be put on a network, and nothing in it makes that safe.

## What a model can reach

Assume the model is hostile, or has been told to be by something it read. It controls everything it writes: build programs, shell commands, tool arguments, titles and the text in the transcript.

| What the model does | Where it runs | What it can reach |
|---|---|---|
| Build programs (JavaScript) | A single-use process under `sandbox-exec` on macOS or bubblewrap on Linux. It can open no file and has no network | The chat's world, and files in the chat's own folder through `mc.readFile` and `mc.writeFile`, which the server performs for it. The server checks everything the program returns |
| `bash` | A Docker container with every capability dropped, a read-only root, and your user and group IDs, so the files it writes are yours | That chat's folder. No environment from the host and no network |
| `bash` with the network, one command at a time | The same image, when the model asks for it | The public internet, with that chat's folder mounted. It cannot reach loopback, private ranges or cloud metadata, and IPv6 is off in that container |
| Claude Code | On your machine, as the signed-in CLI | mcbench's tools. Every built-in tool is off except, in chats, its four task-list tools, which only keep a to-do list the app shows. The app ends the run if Claude Code offers any other tool |
| Codex CLI on macOS | On your machine, in Codex's read-only sandbox, which stops writes and can still read every file your account can | mcbench's tools. The app ends the turn when Codex reports anything other than its text, reasoning, plan, errors and calls to mcbench's tools |
| Codex CLI on Linux | A container whose only network is a proxy to chatgpt.com and auth.openai.com | mcbench's tools. Codex's `apply_patch` remains, and using it ends the turn, as anything other than text, reasoning, plan, errors and mcbench's tools does |
| Cursor | The Cursor SDK inside the server's own process, working in `data/cursor-agent` | Cursor is told to offer only mcbench's tools, and the app ends the run when Cursor starts any other tool. The SDK does not report some of its built-in tools, web search and fetch among them, so the app cannot see those |
| `prompt_library` | The server | Your prompt library, which every chat and bench uses. A model can read every prompt and add new ones, which the library marks with the chat and model that added them. It can change or remove only a prompt it added in that chat |
| Text it writes | The page | Shown as text. The markdown renderer makes no links, images or HTML |

Provider keys are never sent to the page, to a model, to a container or to a log. A provider's error that quotes the key has it replaced before the page or the log sees it. The keys are in the environment of the server and of the programs it starts on your machine: git, gh, java, the engine, and the Cursor SDK, which runs inside the server. On Linux the ChatGPT login file is mounted, writable, into the Codex container, whose network reaches only chatgpt.com and auth.openai.com.

Two rows are weaker than the others: the networked shell command, and the harnesses that run on your machine.

The networked shell command can send out whatever is in the chat's folder, your attachments among them, to any public host. The model decides when to use it, so a file you attach to a chat can leave your machine if the model, or text the model read, chooses to send it.

Claude Code, Codex on macOS and Cursor are not in a container. What confines them is their own flags and, for Codex, its sandbox, which this project does not control. If one of those programs has a bug that gives a model a tool it was told not to have, the model is running as you. For Cursor that also means running with your provider keys in its environment: the SDK offers no way to give its agent a separate environment, and the app ends the run when it sees such a tool start, which may be after it ran. An API key or the ChatGPT sign-in runs mcbench's own loop instead, where the model reaches only the first three rows.

## Other attackers

| Attacker | What they control | What stops them |
|---|---|---|
| A web page in your browser | Requests to 127.0.0.1, and frames | Every request needs the access token. The browser's cookie is set on the app's own `mcbench-<label>.localhost` name, where the label comes from the token. Browsers send cookies to every port of a host name, so a program listening on another port of that name would receive it: it would have to know the label first. A foreign `Host` is refused, a cross-site request to the API is refused, another site cannot frame the app's pages or its attachments, and an attachment is served as a download unless it is an image, a PDF or text |
| Another account or process on the machine | Connections to 127.0.0.1, and files its permissions allow | The same token, 32 random bytes in `data/auth-token`, readable by your account alone. The data folder, the tools folder and `settings.json` are private to your account. Settings cannot name a program for the server to run: a CLI is the one `MCBENCH_CLAUDE_BINARY` or `MCBENCH_CODEX_BINARY` names, else the one on the server's `PATH`, else the one the app installed in its tools folder. The page can ask the server to install Claude Code or Codex CLI by name only; the version, address, checksum and folder are the server's |
| Untrusted data | Attachments, worlds a model made, and the repository you name for Share | A malformed input fails its one request. The engine runs in a child process; requests, reports and run time are capped; each chat's world, attachments and shell folder have a storage budget and the disk keeps a reserve free. A Share repository that holds symbolic links or submodules is refused before anything is written |

## What is not defended

A program running as you can read the token, your keys and every chat. Keys saved in Settings are plain text in `data/settings.json`, readable by your account only. Keys in `.env`, which only you create (`install -m 600 .env.example .env`), are plain text too, and the app refuses to start when other accounts can read or write it. There is no keychain integration.

The address the server prints on every start contains the access token, so your terminal's scrollback and any log you redirect it to hold the token. It only works from your own machine, and from there it gives full access to the app. The browser's cookie lasts 400 days. To revoke both, delete `data/auth-token` and restart.

Share makes a build public on your GitHub Pages repository: its page, its blocks, its title and Minecraft's block textures. Removing it in Settings deletes its page and blocks from the site. The viewer's folder, which holds the textures, stays. Commits are made as your GitHub login, the title is in the commit message, and all of it stays in the repository's history until you rewrite it or delete the repository.

The shell's image is built on your machine while the server starts, when Docker is running, from the `python:3.12-slim` tag and the current pip and apt packages. Nothing in it is pinned to a digest.

The programs in the tools folder are checked against their pinned sha256 once, when they are downloaded, and run as you from then on. A program running as you can change them, as it can change anything else of yours.

On Linux the shell's container shares the host's kernel, so a kernel bug is a way out. On macOS it runs inside Docker Desktop's virtual machine.

An error response carries the error's message, which can include a path on your machine. Only a local client that holds the token can read it. A model sees the paths of its own folder as `/work`, but Claude Code's and Codex's own prompts name their working folder, which is under your home folder.

Claude Code keeps each chat's transcript under `~/.claude/projects`. Deleting a chat deletes its current transcript there; transcripts of sessions the chat replaced earlier stay.

Only the parity tools start the Paper reference server, never the app. While it runs, its game port accepts any local client under any name (offline mode), and `MCBENCH_MC_BIND` can put it on a network.

## What the app downloads

The app installs the programs it needs into its tools folder, `~/.mcbench-lite/tools` or the folder `MCBENCH_TOOLS_DIR` names, which only your account can read. Each is pinned in `server/tools.js` to one version and, for macOS and Linux on ARM and Intel, to the sha256 of its download, taken from the publisher's own checksum file where it publishes one and confirmed by downloading. A download whose sha256 differs fails, and nothing of it is kept. A program is unpacked into a folder named after its version, which appears whole, and nothing outside the tools folder is changed: no `PATH`, no shell profile, no `~/.rustup`.

| Program | Version | From | Checksum from | When |
|---|---|---|---|---|
| Temurin JDK | 25.0.5+8 | github.com/adoptium/temurin25-binaries | Adoptium's `.sha256.txt` beside each file | the first start, unless `MCBENCH_JAVA` and `MCBENCH_JAVAC` name a JDK |
| rustup (`rustup-init`) | 1.29.3 | static.rust-lang.org/rustup/archive | rustup's `.sha256` beside each file | the first start |
| Rust and its WebAssembly target | 1.99.1 (`core/rust-toolchain.toml`) | static.rust-lang.org, through rustup | rustup's own checks of its downloads | the first start |
| Chromium's headless shell | the build playwright-core names, 149.0.7827.55 | cdn.playwright.dev (Chrome for Testing's builds; Playwright's own build on ARM Linux) | none is published: the sha256 of each file as downloaded, the same as Google's Chrome for Testing files where those exist | the first start |
| Claude Code | 2.1.311 | Anthropic's release bucket on storage.googleapis.com | the release's `manifest.json` | when you ask for it in Connect an AI and have no `claude` |
| Codex CLI | 0.167.1 | github.com/openai/codex releases | GitHub's sha256 of each release file | when you ask for it in Connect an AI and have no `codex` |

Paper's jar, BlueMap's jar and misode/mcmeta's block data go into the data folder, each pinned by sha256 (`server/paper-files.js`, `server/bluemap.js`, `server/assets.js`). Paper's launcher then downloads the Minecraft server from Mojang, and BlueMap's tool the Minecraft client jar; the app does not check those itself. The shell's image is not pinned (above).

## Reporting a vulnerability

Report it privately through the repository's Security tab ("Report a vulnerability"), not in a public issue. Say what an attacker controls and what they get. The most serious kind is a way for a model to reach a file outside its chat's folder, your machine or local network, or a key.

There is no bounty and no promised response time: this is one person's project. Only the current `main` is supported.
