# Inventory: the page's components before `App` (`web/app.js` lines 1 to 3405)

This file records every component, hook, helper and constant defined in `web/app.js` lines 1 to 3405, and the `web/*.mjs` modules they use, as the page behaves on branch `s4-inventory` (`web/app.js` 7,241 lines). `App` (lines 3406 to the end), `style.css`, the viewer's iframe document and the server's routes are recorded by other agents; where a component depends on what `App` passes, this file records the prop and what the component does with it. User-visible strings are quoted exactly in backticks. Line numbers are of `web/app.js` unless another file is named. Visual details that live only in `style.css` (colours, sizes beyond the icon sizes given in the code, animations other than the timings in the code) are not recorded here.

## 1. Summary tables

### 1.1 Icon names (`Icon`, `web/app.js:474-532`)

Every icon is a 24×24 line drawing in the text colour, stroke 1.75, round caps and joins, `aria-hidden`. "Use sites in this range" lists the lines before 3406 that draw it; "App" means `App` also draws it (lines from a search of `web/app.js`, for the other inventory to confirm).

| Name | Depicts | Use sites in this range | Also in App |
|---|---|---|---|
| `send` | an up arrow (arrow head and shaft) | none | yes (6504, 6518) |
| `eye` | an eye: lens outline with a round pupil (the first definition, line 477; a second at 507 is unreachable) | none | yes (6005 tool icon for `see_build`/`view_image`, 6694, 6994) |
| `eye-off` | an eye struck through | none | yes (6994) |
| `trophy` | a trophy cup with handles and base | `BenchResultRow` 3044, 3048 | yes |
| `play` | a filled right-pointing triangle | `LiveControls` 3382 | no |
| `pause` | two filled rounded bars | `LiveControls` 3382 | no |
| `step` | a filled triangle with a bar after it (step forward) | `LiveControls` 3385 | no |
| `restart` | a circular arrow turning back to the left | `ArchivedRow` 2812, `LiveControls` 3388 | no |
| `stop` | a filled rounded square | `BenchResultRow` 3046 | yes |
| `settings` | a gear | none | yes |
| `copy` | two overlapping rounded squares | `CopyButton` 646, `PublishedBuilds` 2894 | yes |
| `check` | a check mark | `PlanCard` 162, `CopyButton` 646, `SetupStatus` 1188, `ModelPicker` 1515, `EffortPicker` 1576, `PromptsSection` 2049, `FindFilter` 2164, `PromptEditor` 2484, `BenchModelChip` 2753, `BenchResultRow` 3019 | yes |
| `compose` | a square with a pencil (new chat) | none | yes |
| `panel` | a square split by a left bar (sidebar) | none | yes |
| `archive` | an archive box | none | yes |
| `star` | a five-point star outline | `ModelsSection` 861, `ModelPicker` 1447, 1522, `PromptPalette` 1798, `PromptList` 1955 | no |
| `sun` | a sun with rays | none | yes (theme) |
| `moon` | a crescent moon | none | yes (theme) |
| `search` | a magnifier | `ModelPicker` 1479, `PromptPalette` 1779, `PromptsSection` 2015 | yes |
| `x` | a diagonal cross | `SetupStatus` 1189, `PromptList` 1964, `FilterChips` 2201, `PromptEditor` 2381, 2439, `SelectionChip` 2552, `BenchModelChip` 2738, `ConfirmButton` users `PublishedBuilds` 2904 and `SharePanel` 2987 | yes |
| `equal` | two horizontal lines | none | yes |
| `trash` | a bin with lid and two lines | `ArchivedRow` 2820 | no |
| `share` | three linked nodes | none | yes |
| `build` | an isometric cube | `BlueMapViewer` 3311, 3361 | yes |
| `plus` | a plus | `ModelPicker` 1469, `PromptsSection` 2059, `PromptEditor` 2363, 2411, `BenchModelChip` 2758 | yes |
| `grip` | a 2×4 grid of dots | `ModelsSection` 859 | no |
| `back` | a left chevron | none | yes |
| `forward` | a right chevron | `ConnectPane` 1169, `ModelPicker` 1529, `PublishedBuilds` 2898 | yes |
| `chevron-right` | a right chevron, the same shape as `forward` placed one unit to the left | `PromptsSection` 2009 | yes |
| `chevron-down` | a down chevron | `ResolutionPicker` 238, `ModelPicker` 1437, `EffortPicker` 1560, `PromptEditor` 2476, `BenchModelChip` 2745 | yes |
| `terminal` | a prompt chevron and an underscore | none | yes (6005, tool icon for `bash`) |
| `hammer` | a hammer | none | yes (6006, tool icon for `build`) |
| `refresh` | two curved arrows in a circle | `ProviderPane` 1093, `BenchResultRow` 3024 | no |
| `up` | an up arrow | none found | none found |
| `down` | a down arrow | none found | none found |
| `prompts` | an open book with lines | none | yes |
| `plug` | an electric plug | `ConnectPane` 1155 | yes |
| `wrench` | a wrench | none | yes (6008, tool icon for other tools) |
| `file` | a page with folded corner and lines | `PromptPalette` 1794, 1821, `PromptEditor` 2428 | yes |
| `bench` | three bars of a podium on a base line | none (passed as a `footer.icon` by App, 6480) | yes |
| `select` | a dashed selection frame (corners and edge ticks) | none found | none found by a literal search (it may be built from a variable) |
| `swap` | two opposite horizontal arrows | `ModeSwitcher` 2711 | yes |
| `edit` | a pencil | `PromptPalette` 1796 | no |
| `open` | a box with an arrow out of its corner | `PublishedBuilds` 2892, `BenchResultRow` 3050 | yes |
| `filter` | three lines of decreasing length | `PromptsSection` 2034, `FindFilter` 2141 | no |
| `fork` | a branching fork (two nodes joining to one) | none | yes (6704, 6718) |
| `sparkles` | a large and two small four-point sparkles | `AddedByMark` 1738, `PromptEditor` 2472 | no |

The `chevron-right` and `forward` paths are identical in shape (`m9 6 6 6-6 6` versus `m10 6 6 6-6 6`, offset by one unit). An unknown name draws nothing in a sized SVG.

### 1.2 Providers, harnesses and logos

Provider ids, labels, auth and harness flags are the server's (`server/settings.js:26-37`); logos are mapped by `web/vendor-icon.mjs`; brand colours by `BRAND` (`web/app.js:540-549`). Logo files are in `web/icons/` (`anthropic`, `block` (the app's tab icon), `claude`, `codex`, `cursor`, `deepseek`, `gemini`, `groq`, `meta`, `mistral`, `moonshot`, `openai`, `openrouter`, `qwen`, `xai`), drawn as a CSS mask.

| Id | Label | Auth | Harness | Key variable | Provider logo (`logoName`) | Brand colour | Model logos (`vendorIcon`) |
|---|---|---|---|---|---|---|---|
| `openrouter` | `OpenRouter` | `api-key` | no | `OPENROUTER_API_KEY` | `openrouter` | `#7072f5` | by model id (see below), else `openrouter` |
| `openai` | `OpenAI` | `api-key` | no | `OPENAI_API_KEY` | `openai` | text colour | always `openai` |
| `codex` | `ChatGPT` | `cli-login` | no | none | `codex` | text colour | always `codex` |
| `anthropic` | `Anthropic` | `api-key` | no | `ANTHROPIC_API_KEY` | `claude` | `#d97757` | always `anthropic` |
| `groq` | `Groq` | `api-key` | no | `GROQ_API_KEY` | `groq` | `#f55036` | by model id, else `groq` |
| `google` | `Google` | `api-key` | no | `GOOGLE_API_KEY` | `gemini` | gradient | always `gemini` |
| `qwencloud` | `QwenCloud` | `api-key` | no | `QWENCLOUD_API_KEY` | `qwen` | gradient | by model id, else `openrouter` |
| `cursor` | `Cursor` | `api-key-or-login` | yes | `CURSOR_API_KEY` | `cursor` | text colour | by model id, else `cursor` |
| `claude-code` | `Claude Code` | `cli-login` | yes | none (binary `claude`, `MCBENCH_CLAUDE_BINARY`, config `CLAUDE_CONFIG_DIR`) | `claude` | `#d97757` | always `anthropic` |
| `codex-cli` | `Codex CLI` | `cli-login` | yes | none (binary `codex`, `MCBENCH_CODEX_BINARY`, config `CODEX_HOME`) | `codex` | text colour | always `codex` |

`vendorIcon(provider, modelId)` for the providers that resell other makers' models: first by id prefix (`openai/` → `openai`, `anthropic/` → `anthropic`, `x-ai/` → `xai`, `google/` → `gemini`, `moonshotai/` → `moonshot`, `deepseek/` → `deepseek`, `qwen/` → `qwen`, `mistralai/` → `mistral`, `meta-llama/` → `meta`), then by a substring of the lowercased id in this order (`llama` → `meta`, `qwen` → `qwen`, `kimi` → `moonshot`, `moonshot` → `moonshot`, `deepseek` → `deepseek`, `mistral` → `mistral`, `mixtral` → `mistral`, `gemma` → `gemini`, `gemini` → `gemini`, `gpt` → `openai`, `whisper` → `openai`, `claude` → `anthropic`, `grok` → `xai`), else `groq` for Groq, `cursor` for Cursor, `openrouter` for everything else (QwenCloud included, when no hint matches). The `anthropic` model logo has no brand colour (text colour); the `claude` provider logo is orange. Tests `test/vendor-icon.test.js`: `the google provider, and every model it serves, draw with the Gemini logo`; `the other providers keep their logos`; `every logo a provider or model can name is in web/icons`.

### 1.3 Model-picker rules (`ModelPicker`, D.3; `ModelsSection`, E.5)

Which providers: only providers enabled in Settings appear on the rail, in the server's order, harnesses after a divider; an enabled provider that is not connected is shown disabled with ` — not configured` or ` — CLI not signed in` in its title. `usable` = connected and enabled. Opening tab: the chosen model's provider if usable, else Favorites if any favourite is of a usable provider, else the first usable provider. With no usable provider the picker shows `No AI connected yet` and `Connect an AI`. Which models: each provider's catalog (`GET /api/models?provider=<id>`) minus the models hidden in Settings, in the order saved in Settings (`modelOrder`, then the rest in catalog order). Favorites tab: favourited models of usable providers, provider by provider. OpenRouter tab without a search: favourited OpenRouter models, then at most 30 from `openai/`, `anthropic/`, `x-ai/`, `google/` (in saved/catalog order); the rest only by search. Search: across every usable provider, case-insensitive substring of name or id, each row labelled with its provider; clicking a tab clears it; it survives closing. `requireTools`: only models whose catalog entry has `tools: true` are listed (the caller sets it). Display name: the catalog name without a `Vendor: ` prefix; the trigger shows the id after its last `/`. Favourite key `<provider>::<model id>`, toggled by the star in the picker, in Settings' models list, saved by `favorites.mjs`. Multi-select mode (bench): rows toggle and show a check; the picker stays open. Footer: an optional last row that leaves the list. Nothing disables an individual model row; whole providers are disabled on the rail. Models hidden in Settings never appear, not even through a search or Favorites. The limit chip is not part of the picker: `LimitChip` (C.4) shows the last request's rate-limit state (`Near <window> limit` / `<window> limit reached`, hover `Resets at <time>`); where App places it is App's.

### 1.4 Effort and resolution options

Efforts: the list is the model's (`GET /api/model-info`, `caps.efforts`, in the server's order, words shown verbatim, for example `low`, `medium`, `high`, `xhigh`, `none`, `on` depending on the provider). The picker is offered only when `caps.reasoning` and the list is not empty. `''` means not set: the picker shows `caps.defaultEffort` (else the first). The effort sent is `effortFor(caps, effort)`: the chosen effort if the model lists it, else `''` (the model's default). In a bench chip the effort shown is the chip's or the default, and the plus adds the same model at an effort no chip of that model already has.

Resolutions (`web/resolution.mjs`, shared with the server):

| Value | Label | Short | Feature blocks | Across | Help text under the slider |
|---|---|---|---|---|---|
| 1 | `Standard` | `1×` | none | none | `The model chooses the size.` |
| 2 | `High (2×)` | `2×` | 20 | 100 | `About 20 blocks for a main feature such as an eye or a window. About 100 blocks across the whole build.` |
| 5 | `Very high (5×)` | `5×` | 50 | 250 | `About 50 blocks for a main feature such as an eye or a window. About 250 blocks across the whole build.` |
| 10 | `Maximum (10×)` | `10×` | 100 | 500 | `About 100 blocks for a main feature such as an eye or a window. About 500 blocks across the whole build.` |

Any other value throws `<value> is not a resolution (1, 2, 5, 10)` (test `the stops are Standard, High, Very high and Maximum, and any other value throws`; help test `each stop's help in the picker says only what that stop does`). A list row shows the short tag only above Standard. On a bench comparison the picker is read-only. `storedResolution(key)` reads a saved stop from `localStorage` (keys passed by App), defaulting to Standard.

### 1.5 Settings and their defaults (as far as these components show them)

| Setting | Where shown | Default | Stored |
|---|---|---|---|
| Provider on/off (`enabled`) | `SettingsNavItem` toggle | on | server settings (`data/settings.json`), through the Settings draft (App saves it) |
| API key (`apiKey`) | `ProviderPane` Account | none (environment variable used when set; a saved key takes precedence) | server settings; never sent back to the page |
| Hidden models (`hiddenModels`) | `ModelsSection` toggles and Hide/Show all | none hidden | server settings |
| Model order (`modelOrder`) | `ModelsSection` grip drag and arrow keys | catalog order | server settings |
| Favourite models (`modelFavorites`) | stars in `ModelsSection` and `ModelPicker` | none | server, saved per toggle (`favorites.mjs`), at most 500 kept by the server |
| Title model (`titleModel`) | General pane (App's) | null: each chat's own model writes its title | server settings (`withTitleModel`) |
| Share repository (`shareRepo`) | General or Sharing pane (App's); used by `SharePanel` and `PublishedBuilds` | null: sharing off | server settings (`withShareRepo`; must match `owner/name`) |
| Shortcuts | `BindsSection` | see 1.6 | App's (where App stores them is in the App inventory) |
| Clear leaderboard | `DangerZone` | not a setting; an action | server |
| Last model | read at load from `localStorage['mcb-last-model']` | `{ provider: '', model: '' }` | written by App |
| Prompt generation model | `PromptEditor` | `localStorage['mcb-prompt-ai-model']`, else the last model | `localStorage['mcb-prompt-ai-model']` |
| AI works from | `PromptEditor` split button menu | `text` (`Improve the text`) | `localStorage['mcb-improve-source']` |
| Prompts section folded | `PromptsSection` | unfolded | `localStorage['mcb-prompts-open']` (`open` / `closed`) |

### 1.6 Keyboard handled inside these components

| Where | Keys | What it does |
|---|---|---|
| Rebindable (`BIND_ACTIONS`, run by App) | `Mod+1` Switch to Build; `Mod+2` Switch to Bench; `Mod+K` Search everything; `Mod+Shift+O` New chat or bench; `Mod+B` Show or hide the sidebar; `Alt+S` Select blocks in the viewer (Mod = Cmd on Mac, Ctrl elsewhere) | recorded by `BindsSection`; when they run while layers are open is `layers.mjs` `bindRuns` |
| `BindsSection` while recording | any combination with Mod, Alt, Mac Ctrl, or an F key; Escape | records it; Escape cancels; every key is swallowed while recording |
| Every `usePopover` picker (resolution, model, effort, bench effort menu, prompt tag menu, find filter) | Escape | closes (with the exit animation) and stops the Escape from going further |
| `ResolutionPicker` | arrows, Home, End, Page Up/Down on the focused slider | step through the four stops |
| `ModelsSection` grip | ArrowUp, ArrowDown | move the model one row and save |
| `SettingsNavItem` | Enter, Space | open the provider's pane |
| `FindFilter` menu | ArrowDown, ArrowUp (wrap), Home, End; Escape; Tab out | move focus; close and refocus the button; close |
| `PromptsSection` search input | Escape | ends the search |
| `PromptPalette` search input | every key goes to `onKey` (App: arrows move, Enter or Tab inserts, Escape closes) | |
| `PromptEditor` tag input | Enter, `,`, Backspace on empty, blur | add tag; add tag; remove last tag; add tag |
| `PromptEditor` note box | Enter (no Shift, not composing); Shift+Enter | send; newline |
| `PromptEditor` menu or note | Escape anywhere | closes both |
| `SignInRows` code form | Enter | submits the code |
| `ComposerInput` | every key goes to `onKeyDown` (App) | |
| `BlueMapViewer` | none in the page; the iframe passes `bind`, `type` and `escape` messages up | |

### 1.7 Sign-in flows per provider

| Provider | Methods | CLI program | Flow (what the person sees, from `SignInRows`) | Routes polled |
|---|---|---|---|---|
| Claude Code | `Claude subscription` (gold) or `Anthropic Console` | `claude` (`Claude Code 2.1.311`) | Not signed in → (install if missing: `Installing Claude Code` with MB progress, ` The sign-in starts when it is done.`) → `Starting the sign-in` → `Open the sign-in page` with the link and `Sign in there. The page then shows a code: paste it below.` → `Code from the sign-in page` with a paste field → signed in: `<account>` / `<plan> · Claude login through Claude Code` with `Sign out` | `GET /api/provider-logins/claude-code` every 1 s while starting or waiting; `GET /api/cli-installs/claude` every 1 s while installing |
| ChatGPT (`codex`) and Codex CLI (`codex-cli`), one login | `Sign in` | `codex` (`Codex CLI 0.169.0`) | Not signed in → (install if missing) → starting → waiting with the link and a `One-time code` row (device code with `Copy code`), text `Sign in there and enter the one-time code below. This pane updates when you are done.` → done | as above with the pane's id; `/api/cli-installs/codex` |
| Cursor | `Sign in`, beside the API key | none (SDK login) | Row titled `Cursor login`, help `Sign in with your Cursor account instead of a key. The sign-in page opens in a new tab.` → starting → waiting (`Sign in there. This pane updates when you are done.`) → `Signed in with Cursor` / `Used when no key is set.` with `Sign out` | `GET /api/provider-logins/cursor` every 1 s while running |
| Key providers | none | none | `API key` row only | none |

A login present with its CLI missing (`cliAccount` = `install`) shows the account with `<CLI> is not installed: the app installs it, a <n> MB download, to use this login.` and `Install <CLI>`; after the install Settings are read again and find it connected. Failures appear inside the row's help as `<CLI> was not installed: <error>` or the sign-in's error; the server ends a sign-in not finished in 10 minutes. `ConnectPane` starts the same flow from its buttons via `startWith`. Sign out is `POST /api/provider-logouts` with no confirm.

### 1.8 Counts

Components: 49 (listed in the checklist). Hooks: 5 (`useShared`, `useModelInfo`, `useMedia`, `usePopover`, `useCopy`). Helper functions, constants and module-level listeners recorded: 64. Modules: 23, of which 18 are recorded in full (`composer`, `draft`, `filters`, `favorites`, `settings-panel`, `setup-status`, `resolution`, `clipboard`, `upload-image`, `vendor-icon`, `prompt-sync`, `share`, `published`, `shared`, `bench-details`, `command`, `layers`, `archive`) and 5 used only by App are noted in P.6 (`transcript`, `place`, `address`, `live-world`, `live-host`).

## Part A. Module-level helpers and constants, lines 1 to 649

### A.1 Imports and server helpers (`web/app.js:1-73`)

Lines 1 to 24 import from `/vendor/preact-htm.mjs` and the page modules `transcript`, `shared`, `layers`, `draft`, `place`, `address`, `prompt-sync`, `resolution`, `filters`, `favorites`, `settings-panel`, `setup-status`, `share`, `bench-details`, `upload-image`, `vendor-icon`, `archive`, `published`, `command`, `clipboard`, `composer`, `live-world`, `live-host`. `transcript`, `place`, `address`, `live-world` and `live-host` are used by `App` (other agent) and are not part of this inventory beyond noting that nothing before line 3406 except `itemsFromMessages` and `BlueMapViewer`/`LiveControls` touches them (checked below where used).

| Helper | Lines | Behaviour |
|---|---|---|
| `api(path, options)` | 26-31 | `fetch`, parses the body as JSON always (a non-JSON body throws a parse error), throws `Error(body.error)` or `Error("HTTP <status>")` when `!res.ok`, else returns the body. Every route call in this range goes through it unless noted. |
| `readLines(res, signal, onMessage)` | 35-47 | Reads an NDJSON stream: splits on `\n`, parses each non-blank line as JSON and calls `onMessage`; returns when the stream ends or `signal.aborted`. The last partial line is kept in the buffer; a final line without a trailing newline is dropped (it is never flushed at `done`). |
| `shared` / `load(url, options)` | 50-51 | One `Shared` cache (`web/shared.mjs`, see module section) built on `api`. |
| `useShared(urls)` | 54-65 | Subscribes the component to each non-null url in the cache; on mount and whenever the set of urls changes it calls `load(url)` for each and ignores errors (`.catch(() => {})`). Returns per url `{ value, error }` (or `{ value: undefined, error: null }` before the first load), `null` for null urls. |
| `modelsUrl(provider)` | 67 | `/api/models?provider=<provider>` |
| `modelInfoUrl(provider, model)` | 68-69 | `/api/model-info?provider=<p>&model=<m>`, or `null` when either is empty. |
| `useModelInfo(provider, model)` | 71-73 | The cached model-info value or `null`. |
| `effortFor(caps, effort)` | 77 | The effort sent with a request: `effort` when `caps.reasoning` and `caps.efforts` contains it, else `''` (model default). |

### A.2 `renderInline` and `Markdown` / `renderMarkdown` (`web/app.js:81-142`)

`Markdown({ text })` memoises `renderMarkdown(text)` per distinct text (comment 99-100: a streaming turn re-renders per token).

`renderMarkdown` normalises lines first: a blank line is skipped when it is the first line or follows another blank line; trailing blank lines are removed. So leading blank lines vanish, runs of blank lines collapse to one, trailing blank lines vanish. Then each line is classified, checked in this order: bullet `^\s*[-*•]\s+(.*)`, numbered `^\s*\d+[.)]\s+(.*)`, heading `^\s*#{1,4}\s+(.*)` (only one to four `#`). Consecutive bullet lines form one unordered list; consecutive numbered lines one ordered list; switching kind starts a new list; any other line (including a blank line) ends the list. The output is a `div.md` containing, per block: `ul`/`ol` of `li`, a heading as `div.md-h` (all heading levels look the same), every other line as `div.md-line` (a blank line becomes an empty `div.md-line`). Ordered lists do not keep the source numbers (an `ol` restarts at 1). No HTML string is ever rendered.

`renderInline` handles, with one regex (line 84): `` `code` `` (no newline, no backtick inside) becomes `code`; `**bold**` becomes `strong`; `*italic*` becomes `em` only when the asterisk hugs the text (no space after the opening star, no space before the closing star); `_italic_` becomes `em` only at word edges (not preceded or followed by a word character or underscore). Nesting is not supported (the bold inner text is plain). Links, headings inside lists, block quotes, fenced code blocks and tables are not handled: a fenced block shows as lines with literal backticks except that a pair of backticks on one line becomes inline code.

### A.3 `formatElapsed(ms)` (`web/app.js:145-150`)

`42s` under a minute, `3m 5s` from one minute on (no hours unit: 2 hours reads `120m 0s`). Negative input clamps to `0s`.

### A.4 `LIMIT_WINDOWS` and `formatTokens` (`web/app.js:177-181`)

`LIMIT_WINDOWS`: `five_hour` → `5-hour`, `seven_day` → `weekly`, `seven_day_opus` → `weekly Opus`, `seven_day_sonnet` → `weekly Sonnet`. Any other type → `usage` (in `LimitChip`).

`formatTokens(count)`: from 1,000,000 → `1M` / `1.2M` (one decimal unless an exact multiple of a million); from 1,000 → rounded thousands `12k`; else the plain number.

### A.5 `CODE_TOKEN`, `Code` and `highlightCode` (`web/app.js:314-333`)

A one-regex JavaScript highlighter used for tool programs. Classes in priority order: comment (`//…` to end of line, `/*…*/`) → `tk-c`; string (single, double or backtick quoted; an unterminated quote runs to end of line, or for backticks to end of text) → `tk-s`; number (hex `0x…` or decimal with optional fraction) → `tk-n`; keyword from the fixed list `const let var function return if else for while of in new typeof module exports true false null undefined break continue throw try catch` → `tk-k`; an identifier followed by optional spaces and `(` → `tk-f` (function call). Everything else is plain text. `Code({ text })` memoises per text. The same highlighter is applied whatever the language (bash commands and JSON included, when the caller passes them).

### A.6 `itemsFromMessages(messages)` (`web/app.js:336-401`)

Rebuilds the transcript's display items from stored OpenAI-format messages. The item kinds and what each takes from the message:

| Message | Item(s) produced |
|---|---|
| `role: 'user'`, array content, `msg.seeBuild` true | `{ kind: 'shots', images }` (the `image_url` parts) |
| `role: 'user'`, array content | `{ kind: 'user', text, attachments, selections, messageId }`: text parts joined by `\n`, `attachment` parts, `selection` parts |
| `role: 'user'`, string content | `{ kind: 'user', text, attachments: [], selections: [], messageId }` |
| `role: 'error'`, not `dismissed` | `{ kind: 'error', text, messageId }`; a dismissed error is dropped |
| `role: 'assistant'` | in order: `{ kind: 'usage', telemetry, rateLimit }` if `telemetry` (never rendered inline; it drives the context meter and the limit chip); one `{ kind: 'notice', level, text }` per `notices` entry; `{ kind: 'plan', items }` if `plan`; `{ kind: 'reasoning', text }` if `reasoning`; `{ kind: 'assistant', text, final }` if `content` (`final` is true when the message has no tool calls); then one `tool` item per tool call |
| `role: 'tool'` | fills the tool item with the same call id (an unmatched result is ignored) |

Tool item fields: `callId`, `tool` (function name), `summary`, `program`, `status` (`ok` initially), `detail` (`''`). `program` is `args.program` if a string, else `args.command` if a string, else for `test_build` the arguments pretty-printed as JSON. `summary` is `args.summary` if given, else by tool: `see_build` → `Look at build (<views>)` where each shot gives `shot.view`, or `camera` when it has an `eye`, or `iso_se`, joined by `, `, or `default` when there are no shots; `bash` → `$ ` plus the first line of the command cut to 80 characters; `test_build` → `Test build in Minecraft`, plus ` (<n> tests)` when more than one test; `view_image` → `Look at /work/<path>`; any other tool → its name. When the arguments are not JSON the summary stays `build` (line 366; this is the default for every tool whose arguments fail to parse, not only build tools).

Tool result parsing: JSON content → `status` = `cancelled` if `result.cancelled`, else `ok` if `result.ok`, else `failed`; `diff` copied when ok and present; `output` copied when a string; for `test_build` the output is the whole result pretty-printed; `detail` = `''` when ok, else `result.error`, else `''` when there is output, else `failed`. Non-JSON content: for `bash` the content is the output (stored as terminal text); for any other tool nothing changes (status stays `ok`).

### A.7 `branchNavigation(messageTree)` (`web/app.js:403-424`)

From `messageTree.messages`, considers only messages with `branchable` true, groups them by `parentId`, and for every group of two or more sorted by `position` gives each message `{ index, count, previousId, nextId }` (0-based index; `null` at the ends). The result is a map by message id; the caller (App) draws the branch arrows. A single child has no navigation entry.

### A.8 Attachment limits and readers (`web/app.js:440-470`)

| Constant | Value |
|---|---|
| `MAX_IMAGE_EDGE` | 1568 px (longest edge after downscale) |
| `MAX_ATTACHMENT_BYTES` | 100 MiB per file |
| `MAX_ATTACHMENTS_BYTES` | 200 MiB per message |

`checkAttachmentLimits(current, added)` throws `A message supports at most 8 attachments` when the count would exceed 8, and `Attachments exceed 200 MB total` when the summed `size` exceeds 200 MiB. `readUpload(file)` throws `"<name>" is empty` for a zero-byte file and `"<name>" exceeds 100 MB` above 100 MiB; JPEG, PNG, GIF and WebP files are decoded, drawn with `paintUpload` (`web/upload-image.mjs`) at most 1568 px on the long edge, on a white background (so transparency arrives white, not black; tests `test/upload-image.test.js`: `an upload is painted on white before the image, so transparency is not black`, `an upload longer than the edge limit scales down whole, and the white fills all of it`), and re-encoded as JPEG quality 0.88 (`type` becomes `image/jpeg`; the name is kept, so `a.png` is sent as JPEG data with the name `a.png`); every other file (SVG included) is read unchanged as a data URL by `readBlobAsDataUrl` (`web/clipboard.mjs`), with type `application/octet-stream` when the browser gives none. Returns `{ name, type, size, data }` where `size` is the original file size, not the re-encoded size. An animated GIF becomes its first frame (createImageBitmap behaviour).

### A.9 `favoriteKey`, `BRAND`, `orderModels`, `displayName`, `modelLabel`, `CURATED_VENDORS`, `CURATED_LIMIT` (`web/app.js:534-576`)

`favoriteKey(provider, model)` = `<provider>::<model>`. `BRAND` gives the logo's fill per logo name: `claude` `#d97757`, `openrouter` `#7072f5`, `groq` `#f55036`, `qwen` gradient 135deg `#7b6cf6`→`#5b4fe0`, `gemini` gradient 135deg `#4285f4 15%`, `#9b72cb 55%`, `#d96570 90%`, `meta` gradient 135deg `#0081fb`→`#0064e1`, `mistral` gradient 180deg `#ffd800`, `#ff8205 50%`, `#e10500`, `deepseek` `#4d6bfe`; all others (OpenAI, Codex, Cursor, xAI, Moonshot and the rest) take the text colour.

`orderModels(models, order)`: models in `order` first in that order; the others after, in catalog order. `displayName(model)` drops a leading `Vendor: ` prefix from `model.name`. `modelLabel(id)` is the part after the last `/`. `CURATED_VENDORS` = `openai/`, `anthropic/`, `x-ai/`, `google/`; `CURATED_LIMIT` = 30 (see the model-picker rules).

### A.10 `storedJson`, `storedResolution`, `ago` (`web/app.js:601-626`)

`storedJson(key, fallback)` reads `localStorage[key]` as JSON; a missing key, `null` or unparsable value gives `fallback`. `storedResolution(key)` returns the saved resolution stop value if it is one of the four stops, else 1 (Standard). `ago(iso)`: under 10 s `just now`, under 60 s `<n>s ago`, under an hour `<n>m ago`, otherwise `<n>h ago` (no days: two days reads `48h ago`).

## Part B. Small shared pieces

### B.1 `Icon` (`web/app.js:474-532`)

| Field | What |
|---|---|
| Source | `web/app.js:474-532` |
| What it shows | A 24×24 viewBox SVG at `size` px, no fill, stroke `currentColor`, stroke width 1.75, round caps and joins, `aria-hidden="true"`. The glyph per `name` is listed in the Icon table at the top. An unknown name draws an empty SVG of the given size (no error). |
| Props | `name`, `size` (px; passed as a string in most call sites) |
| Known defects | `eye` is defined twice (lines 477 and 507); the second definition is unreachable, so `eye` is always the first drawing (rounded lens path with a radius-3 pupil). |

### B.2 `Logo` (`web/app.js:551-558`)

A `span.vlogo` square of `size` px whose shape is the mask `/icons/<logoName(name)>.svg` and whose colour is `BRAND[logoName]` when listed, else the text colour. `aria-hidden`. Props `name` (a provider, harness, vendor or model id; mapped by `logoName`, see `vendor-icon.mjs`) and `size`.

### B.3 `AppMark` (`web/app.js:581-599`)

The app's logo: a 64×64 viewBox SVG, a wire hexagon cube outline in the text colour (opacity .32 inner back edges, full-opacity outer edges, stroke 2.4) with a small green block inside (faces `#3ecf6b`, `#229a4b`, top `#7df0a0`, edges `#0b3d1c` 0.7) given a Gaussian-blur glow (`stdDeviation` 2.2). Same drawing as `web/icons/block.svg` (the browser tab icon). The filter id is the fixed string `app-mark-glow`, so several marks on one page share one id. Prop `size`.

### B.4 `Toggle` (`web/app.js:628-637`)

| Field | What |
|---|---|
| What it shows | A switch button (`button.toggle`, class `on` when on) with an inner knob `span`; no visible text. |
| Controls | Click: stops the click from reaching parents (so a toggle inside a clickable row does not select the row) and calls `onChange(!on)`. Space and Enter act as click (native button). |
| Accessibility | `role="switch"`, `aria-checked` = `on`, `aria-label` = `label`. |
| Props | `on` (bool), `onChange(next)`, `label` (accessible name only). |

### B.5 `CopyButton` (`web/app.js:639-649`)

| Field | What |
|---|---|
| What it shows | `button.copy-btn` with the `copy` icon at 13 px; for 1,200 ms after a successful copy the icon is `check` and the title is `Copied`. |
| Controls | Click: writes `text` to the clipboard with `navigator.clipboard.writeText`; on failure nothing happens and nothing is shown. |
| Timers | 1,200 ms back to the copy icon. |
| Accessibility | `title` = `label` (or `Copied`), `aria-label` = `label` (does not change to Copied). |
| Props | `text`, `label`. |
## Part C. Transcript pieces

### C.1 `PlanCard` (`web/app.js:154-169`)

| Field | What |
|---|---|
| What it shows | A card `div.plan-card`. Head row: `Tasks` on the left, `<done> of <total>` on the right (`plan-count`), where done counts items with `status === 'done'`. Then an ordered list, one row per item: a mark slot (the `check` icon at 11 px when the item is done, empty otherwise; `aria-hidden`) and the item's `text`. Each row carries its `status` as its class (the statuses the harnesses send; only `done` is tested in this code; the look of the others, for example in-progress versus pending, is set by `style.css`). |
| Controls | None. |
| States | Empty list shows `0 of 0` and no rows. |
| Props | `items`: `[{ status, text }]` from `msg.plan`. |

### C.2 `NoticeLine` (`web/app.js:173-175`)

A line `div.notice-line <level>` with a dot (`aria-hidden`) and `text`. Used for harness notices outside the conversation (a model fallback, context compaction, a warning; comment 171-172). `level` is whatever the server sends; its colour comes from `style.css`. No controls.

### C.3 `ContextMeter` (`web/app.js:184-196`)

| Field | What |
|---|---|
| What it shows | A 16 px ring: a grey track circle of radius 6.5 and a fill arc proportional to `min(used / window, 1)`, starting at 12 o'clock and going clockwise. Above 85 % the element has class `high` (coloured by CSS). |
| Tooltip | `data-tip` = `<pct>% of context · <used> of <window> tokens`, with `pct` rounded and token counts by `formatTokens` (for example `42% of context · 84k of 200k tokens`). `data-tip` is the page's custom tooltip attribute (drawn by CSS/App, not a native title). |
| Accessibility | The SVG is `aria-hidden`; the span has no role or label, so screen readers get nothing. |
| Props | `used` (tokens on the last request), `window` (the model's context window). A `window` of 0 gives `Infinity`, clamped to 100 %. |

### C.4 `LimitChip` (`web/app.js:289-295`)

| Field | What |
|---|---|
| What it shows | Nothing when `info` is missing or `info.status === 'allowed'`. Otherwise a chip: `<window> limit reached` when `status === 'rejected'` (class `rejected`), else `Near <window> limit`, where `<window>` is from `LIMIT_WINDOWS` by `info.rateLimitType` or `usage`. Examples: `5-hour limit reached`, `Near weekly Opus limit`, `Near usage limit`. |
| Tooltip | `data-tip` = `Resets at <time>` with the local time of `info.resetsAt` (seconds since epoch) formatted hour numeric, minute two digits (for example `Resets at 3:40 PM`); empty when there is no reset time. |
| Props | `info`: the `rateLimit` of the last usage item (`{ status, rateLimitType, resetsAt }`). |

Note: the comment on line 198 (`The subscription's usage limit, shown only once it matters.`) belongs to `LimitChip` but sits above the `ResolutionPicker` comment (a misplaced comment, no behaviour).

### C.5 `Ticker` (`web/app.js:297-310`)

| Field | What |
|---|---|
| What it shows | Three animated dots (`working-dots`, the animation is CSS) in an `aria-hidden` slot, then either `Working for <elapsed>` or `<status> · <elapsed>` when `status` is given; elapsed from `formatElapsed(now - startedAt)`. |
| Timers | A 1,000 ms interval updates the clock; it restarts (and the clock resets to now) when `startedAt` changes. |
| Props | `startedAt` (ms epoch), `status` (optional text from the turn, for example a harness's current activity). |

### C.6 `DiffStat` (`web/app.js:427-435`)

Git-style counts for a build's diff: plus = `added + changed`, minus = `removed + changed` (a changed block counts once each way). Shows `+<plus>` (class `p`) when plus > 0 and `-<minus>` (class `m`) when minus > 0; nothing at all when both are 0. Prop `diff`: `{ added, removed, changed }` (missing fields count as 0).

### C.7 `Markdown` and `Code`

See A.2 and A.5.

## Part D. Pickers

### D.1 `ResolutionPicker` (`web/app.js:203-280`)

Uses `usePopover(110)` (defined later in the range; see its section) and `web/resolution.mjs`.

| Field | What |
|---|---|
| What it shows, read-only | A disabled trigger button showing the stop's short label (`stop.short`), class `raised` when the value is above 1 (Standard). Title and aria-label `Resolution: <label> (set when the bench was made)`. No chevron. |
| What it shows, editable | Trigger: the stop's short label, then a `chevron-down` icon at 12 px. Title and aria-label `Resolution: <label>`. Class `raised` above Standard, `open` while open. When open, a popover `div.resolution-picker-menu` (class `closing` during the close animation): head row with a label `Resolution` (for the slider) and the current stop's full label on the right; a range slider 0 to 3 in steps of 1 at the current stop; a row of four tick buttons, each showing the full label for the Standard stop (value 1) and the short label for the others, the current one with class `selected`; then a help paragraph: all four help texts are in the page, only the current stop's is visible (others have class `hidden` and `aria-hidden="true"`), text from `resolutionHelp(entry)`. |
| Controls | Trigger click: opens, or closes (with the animation) when open. Slider input (drag, click, arrow keys, Home/End, Page keys as the native range gives): calls `onChange` with the new stop's value on every input; the popover stays open. Tick button click: `onChange(entry.value)`; popover stays open. Outside click and Escape: handled by `usePopover` (see there). |
| States | Closed, open, closing (110 ms close animation), read-only. |
| Focus | When it opens the slider receives focus (`useEffect` on `open`), so the arrow keys step one stop at once. |
| Accessibility | Trigger `aria-haspopup="dialog"`, `aria-expanded`. Menu `role="dialog"`, `aria-label="Resolution"`. Slider `id="composer-resolution"`, label `Resolution` via `for`, `aria-valuetext` = the stop's full label, `aria-describedby="composer-resolution-help"` (the visible help paragraph carries that id). The fixed id means two pickers open at once would clash (only one is open at a time in practice). |
| Props | `value` (a stop value), `readOnly` (bench comparison: the bench's stop, cannot be changed), `onChange(value)`. |

### D.2 `ResolutionTag` (`web/app.js:283-287`)

On a list row: nothing at Standard (value 1); otherwise a `span.resolution-tag` with the stop's short label and title `Resolution: <label>`. Prop `value`.
## Part F. Setup

### F.1 `SetupStatus` (`web/app.js:1184-1225`; `web/setup-status.mjs`)

The first start on the new build's screen. `status` is `setupStatus(view)` of `GET /api/setup` (App reads it; the retry `POST /api/setup/retry` is App's `onRetry`).

| Field | What |
|---|---|
| What it shows | Heading `Getting ready`; sub-line `The first start downloads and builds what the app needs. This happens once.` A progress bar filled to `done / total` of the needed (non-optional) steps and the count `<done> of <total> done` (`role="status"`, so screen readers announce changes). When a needed step failed: `A step failed.` and a `Try again` button. Then an ordered list, one row per step: a mark, the label (with an `Optional` tag for optional steps), the value at the end, and a note line under it whose web addresses are links opening in a new tab (`linkParts`). Last: when no AI is usable (`noAI`), a link-styled button `Connect an AI while this runs`; otherwise the line `You can send your first build now. It starts when the app is ready.` |
| Marks per step state | `waiting` a dot; `running` three working dots; `done` the `check` icon 13 px; `failed` the `x` icon 13 px; `unavailable` a dash. All `aria-hidden`. |
| Row text (`stepRow`, `setup-status.mjs:21-33`) | Label: the step's `doing` text while running, else its `title`. Value: `Waiting`, the download progress while running (`<n> of <m> MB` or `<n> MB`, empty when no progress yet), `Done`, `Failed`, `Not available`. Note: while running the step's `detail`, otherwise its `error`, or none. Any other state throws `A setup step cannot be <state>`. |
| Controls | `Try again` → `onRetry()`. `Connect an AI while this runs` → `onConnect()`. Note links open in a new tab with `rel="noreferrer"`. |
| States | Running; a step failed (the failed banner); all done (App stops showing this screen once `ready`); no AI connected versus connected (the last line). An optional step that failed or is unavailable does not count as a failure (`test/setup-status.test.js`, `the count is of the steps the app needs; an optional step that cannot be done is no failure`). |
| Props | `status` (`{ ready, done, total, failed, rows }`), `noAI`, `onConnect()`, `onRetry()`. |
| Oddities | `total` of 0 would make the bar width `NaN%`. |

`heldLine(status)` (`setup-status.mjs:50-53`, used by App under the composer): `Not sent yet: a step failed. Try it again above.` when a step failed, else `Sends when the app is ready. <done> of <total> done.`

`linkParts(text)` (`setup-status.mjs:58-68`): an address is `http://` or `https://` up to a space or `)`, and a trailing `.`, `,` or `)` is left to the sentence (test `an error's web addresses become links, without the bracket or full stop after them`). `downloadSize(bytes)` = `<round(bytes/1e6)> MB`; `megabytes({ done, total })` = `<done> of <total> MB`, or `<done> MB` without a total (tests `a download says how far it is…`, `a download's size is its bytes in MB…`). MB here is 10^6 bytes.

## Part G. Layout helpers and popovers

### G.1 `SIDEBAR`, `NARROW`, `useMedia` (`web/app.js:1229-1247`)

`SIDEBAR` = width 236 px default, resize range 200 to 420 px, and a drag below 140 px collapses it (App implements the resize). `NARROW` = `(max-width: 980px)`: in narrow windows, phones and upright tablets the sidebar is a bar over the world and its list a drawer over the page (comment 1231-1233; layout in `style.css`). `useMedia(query)` keeps a boolean in step with `matchMedia`.

### G.2 Initial model selections (`web/app.js:1249-1250`)

`initialModelSelection` = `localStorage['mcb-last-model']` as JSON, default `{ provider: '', model: '' }`. `initialPromptModelSelection` = `localStorage['mcb-prompt-ai-model']`, defaulting to the first. Both are read once when the page loads (module level). Where they are written is App's (and `PromptEditor`/`PromptPalette` below, if they write; see there).

### G.3 `usePopover(exitMs)` (`web/app.js:1254-1301`)

The shared behaviour of every anchored picker (`ResolutionPicker` 110 ms, `ModelPicker` 120 ms, `EffortPicker` 110 ms, and others below). `setOpen(next)` opens or closes at once and cancels a close still animating. `closePicker()` marks `closing` (the popover gets class `closing`, CSS plays the exit) and closes after `exitMs`; a second Escape or click during the animation does nothing more. While open, the document listens to `mousedown` and `keydown`: Escape closes (and stops the event from reaching other listeners, so for example a modal behind it does not also close), and a mouse press outside the picker's root element closes. A press inside never closes. Touch: only `mousedown` is listened to, so a tap outside closes it only through the compatibility mouse events the browser sends after a tap. Focus is not moved back to the trigger on close.

## Part D, continued. Pickers

### D.3 `ModelPicker` (`web/app.js:1303-1538`)

| Field | What |
|---|---|
| Source | `web/app.js:1303-1538`; `usePopover(120)`; `useShared`; `usable`, `CONNECT` (`settings-panel.mjs`); `vendorIcon`, `logoName` (`vendor-icon.mjs`); `orderModels`, `displayName`, `modelLabel`, `favoriteKey`, `CURATED_VENDORS`, `CURATED_LIMIT`. |
| Trigger | A button: the chosen model's vendor `Logo` 14 px (only when a model is chosen), the label (the model id after its last `/`, or `emptyLabel`, default `Choose <label lowercased>`), a `chevron-down` 12 px. Title: the full model id, or the empty label. Class `empty` when no model is chosen. Click toggles; opening first switches the tab to the opening tab (the chosen model's provider may have changed with the chat). |
| Popover layout | A provider rail (`nav`, `aria-label="Providers"`) and a main column. On open the popover is nudged sideways to stay 8 px inside the window, and the rail's line count is chosen: the stylesheet may let the popover grow to one rail line; where it cannot, the rail takes the fewest lines its providers fit in, spread evenly (CSS variables `--nudge`, `--rail`, `--lines`, `--per-line`, lines 1357-1380). Then the search input receives focus. |
| Rail | First a Favorites button (`star` 16 px, title `Favorites`). Then one button per provider that is enabled in Settings (providers turned off are not on the rail at all), in the server's order (OpenRouter, OpenAI, ChatGPT, Anthropic, Groq, Google, QwenCloud, Cursor, Claude Code, Codex CLI), each its `Logo` 18 px; harness providers carry a `›_` badge, and a divider (`role="separator"`, title `Harnesses`) is drawn before the first harness. A provider that is enabled but not connected is drawn disabled. Titles: `<label>`, plus ` harness: its own agent loop with mcbench tools attached` for a harness, plus ` — not configured` (key provider not connected) or ` — CLI not signed in` (CLI login not connected); ` — disabled in Settings` cannot occur on the rail since disabled providers are filtered out. Last, a `plus` 16 px button, title and aria-label `Set up providers`: closes the picker and calls `onSetup('connect')`. The active tab has class `active`, but no tab is marked active while there is a search query. |
| Opening tab | The chosen model's provider if usable; else `favorites` if any favourite belongs to a usable provider; else the first usable provider; else none. If the active tab's provider stops being usable while mounted, the tab moves to the opening tab. |
| No AI connected | The main column shows `No AI connected yet` and a gold `Connect an AI` button (closes, `onSetup('connect')`); no search. |
| Search | Icon `search` 15 px and an input, placeholder `Search models...`, no label. With a query (trimmed, lowercased): every usable provider's catalog is searched, matching the model's `name` or `id` as a substring; each row then also shows the provider's label; a tab click clears the query. The query is kept between openings (it is not cleared on close). |
| Listed models | Per shown provider: its catalog minus its hidden models, in its saved order (`orderModels`), tagged with the provider. Favorites tab without a query: only favourited models (of usable providers; hidden models are not listed even if favourited). Then, when `requireTools`, only models whose `tools === true`. OpenRouter tab without a query: the favourited OpenRouter models first, then up to 30 more from `openai/`, `anthropic/`, `x-ai/`, `google/` in order; everything else on OpenRouter is reachable only by search. |
| Row | A choice button (vendor `Logo` 16 px from `vendorIcon(provider, modelId)`, the display name, the provider label when searching, and in multi-select mode a `check` 13 px mark, class `on` when chosen; title the model id; `aria-pressed` in multi-select mode only) and a star button (`star` 14 px, class `favorite`, title `Add favorite` / `Remove favorite`, calls `onToggleFavorite(<provider>::<model>)`; the picker stays open). Row class `selected` when it is the chosen model (single mode) or in `selected` (multi mode). |
| Choosing | Single mode: `onChange({ provider, model })` and the picker closes. Multi mode (`onToggle` given): `onToggle({ provider, model })` and the picker stays open. |
| Footer | When `footer` is given (`{ label, icon, onSelect }`): a last button with `Icon footer.icon` 15 px, the label and a `forward` 13 px; click closes the picker and calls `footer.onSelect()`. |
| States | Closed; open; closing (120 ms); no AI; loading (`Loading models...` shown only while some shown catalog is loading and no model is listed yet); error (the first failing catalog's message, prefixed `<provider label>: ` when searching); empty (`No model matches` with a query, `No favorite models yet.` on Favorites, `No matching models.` otherwise); listed. |
| Keyboard | Escape closes (via `usePopover`). Typing goes to the search, which has focus on open. There is no arrow-key navigation through the list and Enter in the search does nothing; Tab walks the rail and the rows. |
| `openNow` / `onOpened` | When `openNow` becomes true: calls `onOpened()`, switches to the opening tab and opens, as a trigger click would. |
| Persistence | Reads `GET /api/models?provider=<id>` for each shown provider through the shared cache (catalogs prefetched elsewhere, so they are usually there at once). Writes nothing itself. |
| Props | `providers`, `value` (`{ provider, model }`), `onChange`, `favorites`, `onToggleFavorite(key)`, `requireTools`, `label`, `emptyLabel`, `selected` (array, multi mode), `onToggle`, `footer`, `onSetup(pane)`, `openNow`, `onOpened`. |
| Accessibility | The trigger has no `aria-haspopup`/`aria-expanded` (unlike the effort and resolution pickers); the popover has no role. Rail buttons have titles but no aria-labels (except the set-up button). |

### D.4 `EffortPicker` (`web/app.js:1540-1583`)

| Field | What |
|---|---|
| Trigger | The current effort's label (the effort word itself, for example `low`, `high`, exactly as the catalog lists it) and a `chevron-down` 12 px; class `open` while open. `aria-label` = `label` (default `Reasoning effort`), `aria-haspopup="listbox"`, `aria-expanded`. |
| Current value | `value` if it is one of `caps.efforts`; otherwise (`''` means not set) the model's `caps.defaultEffort`; otherwise the first effort. |
| Menu | `role="listbox"`, `aria-label` = `label`; one `role="option"` button per effort in the order of `caps.efforts`, `aria-selected` on the current one, which also shows a `check` 14 px. Click: `onChange(effort)` and close. |
| Timers | 110 ms close animation. |
| Keyboard | Escape closes. No arrow keys in the list (Tab only). |
| Props | `caps` (`{ efforts, defaultEffort }`), `value`, `onChange(effort)`, `label`. When the picker is shown at all (only for models with `reasoning`) is the caller's decision. |
## Part E. Settings and sign-in

The provider entries these components read come from `GET /api/settings` (server `details()`, `server/settings.js:211-234`; the route is the other agent's). Per provider the fields used here are: `label`, `connected`, `enabled`, `envKey` (API-key providers and Cursor), `auth` (`api-key`, `cli-login`, `api-key-or-login`), `harness`, `hiddenModels`, `modelOrder`, `keySource` (`saved`, `env` or null), `apiKey` (the draft's typed key; the server never sends the saved one), `signedIn`, `account`, `plan`, `version`, `runtime` (`binary`, `binaryEnv`, `homeEnv`, `binaryValue`, `resolvedHome`), `signIn` (`{ methods, program: { name, title, bytes }, current, installed }`), `summary`, `endpoint`, `protocol` (the last two are not set in `server/settings.js`; where the server adds them is the other agent's part, and `ProviderPane` shows them only when present, except `Protocol`, which it always shows).

### E.1 `SettingsSection` (`web/app.js:651-658`)

A `section.s-section`: a head row with an `h4` title and an optional `action` element on the right, then a card `div.s-card` holding the children. Props `title`, `action`, `children`.

### E.2 `SettingsRow` (`web/app.js:661-671`)

A row `div.s-row` with a text column (title, then help when given) and, when children are given, a control column `div.s-row-control` carrying `data-tip` = `tip`. When `title` is a string the row carries `data-setting=<title>`, which is how Settings search jumps to a row (comment line 660; the search itself is App's). Props `title` (string or element), `help` (string or element), `tip`, `children`.

### E.3 `DangerZone` (`web/app.js:674-710`)

| Field | What |
|---|---|
| What it shows | Section `Danger zone`, one row titled `Clear leaderboard` with help `Deletes every vote, blind or not, so the leaderboard starts over. Benches and their builds stay.` Control: a red (`btn danger`) button `Clear leaderboard`, or `No votes` when the count is 0. When armed: the question, then `Cancel` and a red `Delete votes`. Question text: `Delete the only vote?` for 1, `Delete all <n> votes?` otherwise, `Delete every vote?` when the count could not be read. |
| Controls | `Clear leaderboard` click arms the confirm. `Cancel` disarms. `Delete votes` calls `onClearVotes()`; on success it disarms and sets the count to 0 (button reads `No votes`, disabled); on failure `onError(error)` and the confirm stays armed. |
| States | Counting (`votes` null): button disabled. Count 0: disabled `No votes`. Count n: enabled. Count unknown (read failed): enabled, question `Delete every vote?`. Armed. Clearing: both confirm buttons disabled. There is no timeout on the armed state. |
| Persistence | On mount `GET /api/leaderboard`, reads `data.votes`; failure stores the error and calls `onError(error)`. The delete itself is `onClearVotes` (App's). |
| Props | `onClearVotes()` (returns a promise), `onError(error)`. |

### E.4 `SettingsNavItem` (`web/app.js:712-729`)

| Field | What |
|---|---|
| What it shows | One provider in the Settings sidebar: its `Logo` at 18 px; the label, followed by `v<version>` (class `ver`) when known; below, the server's `summary` line (muted unless connected and enabled); a `Toggle` on the right. Classes `active` (the selected pane) and `off` (disabled). |
| Summary texts (server, `server/settings.js:239-248`) | `Disabled`; `CLI not installed` (CLI login present, CLI missing); `Signed in · <plan>` or `Signed in as <account>`; `Key saved`; `Key from <ENV_KEY>`; `Signed in with Cursor`; `No key or login` (Cursor); `No key`; `Not signed in`. |
| Controls | Click, Enter or Space on the row: `onSelect(id)` (Enter and Space have their default prevented). Toggle: `onToggle(id, on)`, and the click does not select the row (Toggle stops propagation). |
| Accessibility | The row is `role="button"`, `tabindex="0"`; the toggle's label is `Offer <label> in the model picker`. The row has no `aria-current`/`aria-selected` for the active pane. |
| Props | `id`, `connection`, `active`, `onSelect(id)`, `onToggle(id, on)`. |

### E.5 `ModelsSection` (`web/app.js:731-875`)

| Field | What |
|---|---|
| Source | `web/app.js:731-875`; uses `useShared`, `orderModels`, `favoriteKey`, `displayName`, `cliAccount` (`settings-panel.mjs`). `MODEL_ROWS` = 150 (line 731). |
| What it shows, not connected | Section `Models` with one line: `Add a key to list <label>'s models.` for a key provider; `Install <program title> to list <label>'s models.` when the CLI login exists but the CLI is missing; `Sign in to list <label>'s models.` otherwise. |
| What it shows, connected | Section `Models`. Intro: `Hidden models stay out of the model picker. Order and favorites apply wherever this provider is listed.` Toolbar: a text input (id `model-filter-<id>`, placeholder `Filter models`); a link button `Hide all` / `Show all` with no filter, `Hide matches` / `Show matches` with one (`Show` when every match is hidden); a count `<visible> of <total> shown` (visible = not hidden, counted over the whole catalog, not the filter). Then `Loading models…` while the catalog has not arrived, or the error text (class `error`) when it failed. Then up to 150 rows, each: a grip button (`grip` icon 14 px), a star button (`star` icon 14 px, class `on` when a favourite), the display name (vendor prefix stripped), the model id, a `Fast` tag when `model.fast` (title `model.fastNote` or `Fast mode available`), and a `Toggle` (on = shown in the picker). Below: `<n> more. Filter to find them.` when more than 150 match; `No models match.` when the catalog is loaded and nothing matches. Hidden rows have class `hidden` (dimmed by CSS). |
| Filter | Case-insensitive substring of the model's `name` or `id`, after trimming the query. The query resets to empty when the provider (`id`) changes. |
| Controls | Filter input: filters on each keystroke. Hide/Show link: hides (or shows) every current match at once; disabled when nothing matches. Star: `onToggleFavorite(<provider>::<model>)`; title `Add favorite` / `Remove favorite`. Toggle: hides or shows that one model; label `Show <name> in the model picker`. Grip, mouse/pointer: primary button only; press captures the pointer, the row follows the pointer vertically (clamped to the list's first and last row), the rows between its origin and the landing row slide one row towards the origin; release (or pointer cancel) drops it at `from + round(dy / rowPitch)` and saves the order if it moved. Grip, keyboard: ArrowUp / ArrowDown move the model one row up or down among the rows shown and save at once (ignored at the ends; default prevented only when it moves). |
| Reorder rule | `place(modelId, to)` (771-777): the model is put before the model now shown at row `to` (among the shown rows without it), or right after the last shown row when `to` is past the end, in the full saved order; models filtered out keep their places. The new full order goes to `onChange('modelOrder', ids)`. |
| States | Not connected; loading; error; loaded; filtered; more than 150; no match; dragging (list class `dragging`, the dragged row class `dragged`, rows offset by `transform: translateY(...)`); settling (class `settling` for two animation frames after a drop, so rows jump to their new places without animating). |
| Timers | Drop: `settling` is cleared after two `requestAnimationFrame`s. |
| Persistence | Reads `GET /api/models?provider=<id>` through the shared cache when connected. When `checkedAt` changes (the `Checked …` button in `ProviderPane`), re-reads it with `{ fresh: true }`, keeping the current list on screen meanwhile. Writes go through `onChange('hiddenModels', [...])`, `onChange('modelOrder', [...])` (the Settings draft, saved by App) and `onToggleFavorite`. |
| Accessibility | Grip `aria-label` `Reorder <name>: drag, or use the up and down arrow keys`, title `Drag to reorder`. The star has a title but no aria-label or pressed state. The filter input has a placeholder but no label. |
| Props | `id`, `connection` (`connected`, `envKey`, `signIn.program.title`, `label`, `hiddenModels`, `modelOrder`), `favorites` (array of keys), `onToggleFavorite(key)`, `onChange(field, value)`, `checkedAt`. |
| Oddities | A drag in progress when Settings closes is let go of on unmount (line 743) without saving. The row pitch is measured from the first two rows, so rows of unequal height would misplace the drop. Only the first 150 matches can be dragged; with a filter, a model can only be placed relative to visible rows. |

### E.6 Settings text constants (`web/app.js:877-913`)

`RUNTIME_NOTES` rows shown at the end of `ProviderPane`'s Runtime section, per provider:

| Provider | Title | Help |
|---|---|---|
| `claude-code` | `Tools` | `Built-in tools removed; mcbench's tools over MCP are the only ones.` |
| `claude-code` | `Sessions` | `One Claude Code session per chat, resumed on later turns.` |
| `codex-cli` | `Tools` | `Shell, file, web and agent features off; mcbench's tools over MCP.` |
| `codex-cli` | `Isolation` | `Linux: a container with no network but the model API. macOS: on the host, read-only sandbox.` |
| `codex-cli` | `Sessions` | `One Codex thread per chat, resumed on later turns.` |
| `cursor` | `Tools` | `mcbench's tools are the agent's only custom tools; no rules, skills or MCP servers load.` |
| `cursor` | `Sessions` | `A fresh agent each turn, given the conversation so far.` |
| `codex` | `Requests` | `Sent with your Codex login from inside the mcbench server; nothing listens on a port.` |

`SIGN_IN_METHODS`: `claudeai` → `Claude subscription`, `console` → `Anthropic Console`.

`SIGN_IN_HELP`: `claude-code` → `Sign in with your Claude subscription or with an Anthropic Console account. The sign-in page opens in a new tab.`; `codex` and `codex-cli` → `Sign in with your ChatGPT account. The sign-in page opens in a new tab and asks for a one-time code shown here.`; `cursor` → `Sign in with your Cursor account instead of a key. The sign-in page opens in a new tab.`

`LOGIN_THROUGH`: `claude-code` → `Claude login through Claude Code`; `codex` and `codex-cli` → `ChatGPT login through the Codex CLI`.

`SIGN_IN_NEXT`: `claude-code` → `Sign in there. The page then shows a code: paste it below.`; `codex` and `codex-cli` → `Sign in there and enter the one-time code below. This pane updates when you are done.`; `cursor` → `Sign in there. This pane updates when you are done.`

### E.7 `SignInRows` (`web/app.js:934-1079`)

The server runs the provider's own login (`server/provider-login.js`) and the CLI install (`server/cli-install.js`). Claude Code's login offers two methods (`claudeai`, `console`); the ChatGPT login (`codex` and `codex-cli` share it) and Cursor's (SDK) offer none. `program` is `claude` (title `Claude Code 2.1.311`) for Claude Code, `codex` (title `Codex CLI 0.169.0`) for ChatGPT and Codex CLI, and null for Cursor (`server/provider-login.js:59-82`, `server/tools.js:125,138`). `installed` is true when the server read a CLI version.

| Field | What |
|---|---|
| Props | `id`, `connection` (`signIn.methods`, `signIn.program`, `signIn.installed`, `signIn.current`, `envKey`, `account`, `plan`, `connected`, `signedIn`), `startWith` (`{ method }` when a button in Connect an AI opened this pane), `onStarted()` (tells App the startWith was consumed), `onInstalled()` and `onSignedIn()` (both re-read Settings in `ProviderPane`), `onError(error)`. |
| Persistence | On mount and on provider change: shows `connection.signIn.current`, clears the code field, then `GET /api/provider-logins/<id>` and, when there is a program, `GET /api/cli-installs/<program>`. While the sign-in is `starting` or `waiting`: `GET /api/provider-logins/<id>` every 1,000 ms; when it answers `done`, `onSignedIn()`. While the install is `running`: `GET /api/cli-installs/<program>` every 1,000 ms; `done` → `onInstalled()`; `failed` → forgets what was to follow. Start: `POST /api/provider-logins` body `{ provider, method }` (method undefined, so absent in JSON, when there is no choice). Install: `POST /api/cli-installs` body `{ program }`. Cancel: `DELETE /api/provider-logins/<id>`. Code: `POST /api/provider-logins/<id>/input` body `{ text }`, then clears the field. Every failure goes to `onError` (App shows it); a failed install start also forgets what was to follow. |
| The one button | `begin(method)` (984-988): when the CLI is installed, starts the sign-in. When not, remembers what follows the install (`{ signIn: true, method }`, or `{ signIn: false }` for a login whose CLI alone is missing) and starts the install. When Settings, re-read after the install, report the CLI installed, the remembered sign-in starts and then the memory is cleared (978-982). |
| `startWith` | When set (from `ConnectPane` via App), calls `onStarted()` and then `begin(startWith.method)` once. |

The states and what each shows, in the order the code checks them:

| State | Condition | Row title | Help / content | Control |
|---|---|---|---|---|
| Installing | install `running`, or install `done` while a follow-up is remembered | `Installing <program title>` | `Starting the download.` before progress; `Downloading, <n> of <m> MB.` or `Downloading, <n> MB.` (`megabytes`); `Installed.` once done; plus ` The sign-in starts when it is done.` when a sign-in follows | three working dots, no button (cannot be cancelled) |
| Starting | sign-in `starting` | `Starting the sign-in` | `Waiting for the sign-in address.` | `Cancel` |
| Waiting | sign-in `waiting` | `Open the sign-in page` | `SIGN_IN_NEXT[id]`, then the address as a link (opens in a new tab, `rel="noreferrer"`) and a `CopyButton` labelled `Copy address` | `Cancel` |
| Waiting, with a device code | also `userCode` | second row `One-time code` | `Enter this code on the sign-in page.` | the code in a chip (`cmd-chip`) with a `CopyButton` labelled `Copy code` |
| Waiting, wanting input | also `wantsInput` | row `Code from the sign-in page` | `signIn.message` (the CLI's last printed line) or `Paste the code the page shows after you sign in.` | a form: monospace input (id `sign-in-code-<id>`, no autocomplete, no spellcheck, placeholder `Paste the code`) and a gold `Submit` button disabled while the trimmed code is empty; Enter submits |
| Login present, CLI missing | no key provider and `cliAccount` = `install` | `<account>` or `Signed in` | `<plan> · <LOGIN_THROUGH>. <program title> is not installed: the app installs it, a <n> MB download, to use this login.` plus the failure text when any | gold `Install <program title>` |
| Not signed in | otherwise | `Cursor login` for Cursor (it has a key too), else `Not signed in` | `SIGN_IN_HELP[id]`, plus ` The app installs <program title> first, a <n> MB download.` when not installed, plus the failure text | one button per method (`Claude subscription` gold, `Anthropic Console` plain) for Claude Code; a single gold `Sign in` for the others |

Failure text (shown in a `span.s-signin-error` inside the help): `<program title> was not installed: <error>` when the install failed, else the sign-in's own `error` when it failed (for example the server's `The sign-in was not finished within 10 minutes, so it was ended. Start it again when you are ready.`, `server/provider-login.js:122`). A `cancelled` or `done` sign-in falls through to the not-signed-in row with no failure text.

Oddities: if Cursor's entry ever reports `installed` false, the not-signed-in help reads `program.title` of a null program and throws (Cursor has no program); the code does not guard it and it is ambiguous whether the server can send that. The polling effects depend on `[id, running]` / `[id, installing]` only, so they stop as soon as the state leaves running. `onError` on each 1-second poll means a server outage during a sign-in reports one error per second.

### E.8 `ProviderPane` (`web/app.js:1081-1146`)

| Field | What |
|---|---|
| What it shows | Head: `Logo` 22 px, the label as `h3`, `v<version>` when known, and a re-check button: `refresh` icon 13 px and `Checked <ago(checkedAt)>` (title `Check logins, keys and versions again`). Then three sections. |
| Account section | For a provider without a key that is signed in (`cliAccount` = `connected`): row `<account>` or `Signed in`, help `<plan> · <LOGIN_THROUGH>` (parts that are missing are left out), button `Sign out`. Without a key and not signed in: `SignInRows`. For a key provider: row `API key` with help `Saved in data/settings.json. Paste a new key to replace it.` (saved key), `Read from <ENV_KEY>. A key saved here takes precedence.` (environment key), or `Stored in data/settings.json and never sent to the browser. <ENV_KEY> works too.` (none); a password input (id `key-<id>`, monospace, no autocomplete), value the draft's `apiKey`, placeholder `••••••••••••` when a key is saved, else `Paste your key`. For a key provider that also has a sign-in (Cursor): when signed in, row `Signed in with Cursor`, help `Used when no key is set.`, button `Sign out`; otherwise `SignInRows`. |
| Models section | `ModelsSection`. |
| Runtime section | For a CLI harness (`runtime` present): row `Binary`, help `Set by <binaryEnv> in the server's environment; unset, <binary> on PATH, else the one the app installed.`, value the env value or the binary name (monospace); row `Config directory`, help `Set by <homeEnv> in the server's environment. Login and sessions live here.`, value `resolvedHome`. Row `Endpoint` with the endpoint when present. Row `Protocol` with `connection.protocol` (always shown, even if empty). For a provider that is not a harness and is not `codex`: row `Loop`, help `mcbench's own turn loop calls this API with mcbench's tools.` Then the `RUNTIME_NOTES` rows. |
| Controls | Re-check: `onRecheck()`. Sign out (both places): `POST /api/provider-logouts` body `{ provider: id }`, then `onRecheck()`; failure → `onError`. Key input: `onChange('apiKey', text)` per keystroke (saved with the Settings draft by App). |
| Props | `id`, `connection`, `favorites`, `onToggleFavorite`, `onChange`, `checkedAt` (ISO time of the last settings read), `onRecheck()`, `startWith`, `onStarted`, `onError`. |
| Oddities | `Checked <ago>` is computed at render time and does not tick by itself; it updates only when the pane re-renders. Sign out has no confirm. |

### E.9 `ConnectPane` (`web/app.js:1153-1177`; `settings-panel.mjs` `CONNECT_WAYS`, `connectWays`, `usable`, `cliAccount`)

| Field | What |
|---|---|
| What it shows | Head: `plug` icon 20 px and `Connect an AI`. Intro `mcbench builds with a model you connect. Choose one way in; you can add others later.` Then one section per way (`settings-panel.mjs:72-97`), with its title, its `need` text (text between backticks is drawn as `code`; none of today's texts contain backticks) and one button per pane. Each pane button: `Logo` 16 px, the provider label, a state text, a `forward` icon 13 px; class `connected` when `usable` (connected and enabled). State text: `Sign in` for a CLI login with no login, else the server's `summary`. |
| Ways | `api-key` `An API key`, need `Get a key on the provider's website and paste it into its pane. OpenRouter is the one key that reaches most models; the others reach their own. Usage is billed by that provider.`, panes OpenRouter, OpenAI, Anthropic, Google, Groq, QwenCloud. `claude` `Your Claude subscription, through Claude Code`, need `Sign in with your Claude subscription, or an Anthropic Console account from the Claude Code pane. The app installs Claude Code first if this machine does not have it. Usage counts against that plan or account.`, pane Claude Code. `chatgpt` `Your ChatGPT subscription, through Codex CLI or ChatGPT sign-in`, need `Sign in with your ChatGPT account from either; one sign-in serves both. The app installs Codex CLI first if this machine does not have it. Usage counts against your ChatGPT plan.`, panes Codex CLI, ChatGPT. `cursor` `Cursor`, need `Needs a Cursor API key, or a sign-in from the Cursor pane. Usage counts against your Cursor account.`, pane Cursor. |
| Controls | Pane button click: for a CLI login that is not connected, `onConnect(id)` (App opens the pane and starts the sign-in or install, via `startWith`); otherwise `onSelect(id)` (opens the provider's pane). |
| States | Error: `connectWays` throws `No provider <id> to connect` if the server omits a listed provider (fail fast; tested in `test/settings-panel.test.js`). |
| Props | `providers` (map id → connection), `onSelect(id)`, `onConnect(id)`. |

### E.10 `settings-panel.mjs` (109 lines)

The Settings panel is one value with four phases (lines 1-11): `closed { phase }`; `opening { phase, view }` while saved settings are read; `open { phase, view, draft, error }`; `saving { phase, view, draft }`, during which there is no edit, no second save and no closing; a save that lands closes the panel, one that fails opens it again with its error. Allowed moves (`NEXT`, 15-20): closed → opening; opening → opening (another pane asked for), open, closed (read failed); open → open, saving, closed; saving → open, closed. `panelTo` throws `Settings cannot go from <a> to <b>`, `Open settings, and only they, hold a draft`, `Settings not closed, and only they, are on a pane`. `panelEdit` throws `Settings cannot be edited while <phase>` outside open and clears `error` on every edit. `panelDraft` is the draft in open and saving, else null. `withTitleModel(draft, choice)` sets `titleModel` to `{ provider, model, effort: '' }` or null (null means each chat's own model writes its title). `withShareRepo(draft, text)` trims; empty saves null, which turns sharing off. `usable`, `cliAccount` (`connected` / `install` / `sign-in`), `CONNECT` = `connect`, `firstPane(providers)` = `general` once any provider is usable, else `connect`. Tests: `test/settings-panel.test.js`.
## Part H. Composer placeholder ideas (`web/app.js:1587-1702`)

`IDEAS` (lines 1587-1670) is the list of 82 placeholder invitations shown in an empty chat's composer (comment 1585-1586: only in a chat with no prompt yet). The native app needs the list verbatim; it is in `web/app.js:1588-1669` and is not repeated here. The first is `Add a cat`, the last `A vending machine that dispenses snacks`. `EGGS` (1673-1684), ten rarer ones, for example `Herobrine's summer house`, `A to-scale replica of this text box`, `Whatever you were going to build anyway`. `SEASONAL` (1687-1694), by month and day: `1-1` `Fireworks over a city at midnight`; `2-14` `A heart made of roses in a garden`; `3-17` `A pot of gold at the end of a rainbow`; `4-1` `A perfectly normal house, upside down`; `10-31` `A haunted manor with a pumpkin patch`; `12-25` `A gingerbread village under snow`.

`pickIdea()` (1696-1702): on a seasonal day, with probability 1/2 the day's idea; otherwise with probability 1/40 a random egg, else a random idea (uniform). The local date is used. When the pick is made (per chat, per page load, per render) is App's.

## Part I. Prompts library

### I.1 `TAG_COLORS` and `tagColor` (`web/app.js:1706-1712`)

Eight colours `#7da869`, `#c9a25e`, `#6f9aa8`, `#9a7fae`, `#a8785f`, `#a86f7e`, `#8fa86f`, `#6f7fa8`. A tag's colour is `TAG_COLORS[h % 8]` where `h` starts at 0 and for each code point `h = (h * 31 + codePoint) >>> 0` (unsigned 32-bit). Stable across sessions and devices; nothing is stored.

### I.2 `promptSections(prompts, query)` (`web/app.js:1716-1730`)

Filter: the trimmed, lowercased query as a substring of the title, the text or any tag (case-insensitive). Sections, in order: `Favorites` (matching favourites, library order); `Recent` (matching non-favourites with a `usedAt`, newest first, at most 5); then the rest in library order, labelled `All` when there is a query or either earlier section is non-empty, else unlabelled. Empty sections are dropped.

### I.3 `AddedByMark` (`web/app.js:1735-1740`)

The mark on a prompt that a model added through its `prompt_library` tool. Label `Added by <model id after its last slash> in a chat` as both title and aria-label. Full form: `sparkles` icon 13 px and the label text. Compact form (list rows): the `sparkles` icon 12 px alone. Props `addedBy` (`{ model, … }`; the chat is stored by the server too but not shown), `compact`.

### I.4 `PromptText` (`web/app.js:1742-1751`)

A paragraph (`p.pp-text`) of a prompt's text where each `{{fill-in}}` (`composer.FILL_IN`, `\{\{([^{}\n]*)\}\}`) is replaced by a highlighted `mark` showing the name inside the braces, or `…` when the braces are empty (`{{}}`). Whitespace around the name inside the braces is kept in this display (the composer trims it when it makes a field). Prop `text`.

### I.5 `PromptPalette` (`web/app.js:1758-1828`)

The palette over the composer, opened by typing `/` (the text after it filters) or by the prompts button (which adds its own search field). Keys (comment 1755): arrow keys move, Enter or Tab inserts, Escape closes; these keys are handled by the caller (`onKey` on the search field here, the composer's key handler otherwise), not inside this component.

| Field | What |
|---|---|
| What it shows, left | With `withSearch`: a search field (`search` icon 14 px; input id `prompt-palette-search`, placeholder `Search prompts`) that receives focus at once. A list of the sections from `promptSections`; section labels are shown only when there is more than one section. Each row: the title or `Untitled prompt`; the compact `AddedByMark` when added by a model; when it has attachments a `file` icon 12 px with the count; an edit button (`edit` 13 px, title `Edit prompt`, aria-label `Edit <title>` or `Edit prompt`); a star button (`star` 13 px, class `favorite` when a favourite, title `Favorite` / `Unfavorite`). The highlighted row has class `active`. When `canSaveDraft`: a footer button `Save message as prompt`. |
| What it shows, right | A preview of the highlighted prompt: its title (or `Untitled prompt`), the full `AddedByMark`, its tags as chips coloured by `tagColor` (CSS variable `--tag`), its text through `PromptText`, and its attachments: images as thumbnails (`img` with the data URL, alt and title the file name), other files as the `file` icon 14 px and the name (title the name). Nothing on the right when nothing is listed. |
| Empty and error | When nothing is listed: with `onRetry` set (the library failed to load), `Prompts failed to load.` and a link button `Try again` (`role="alert"`); otherwise `No prompts match.` when the library has prompts, `No saved prompts yet.` when it is empty. |
| Controls | Row hover: `onIndex(position)` (hover moves the highlight). Row click: `onPick(prompt)`. Edit: `onEdit(prompt)` (does not pick). Star: `onToggleFavorite(prompt.id)` (does not pick). Search input: `onQuery(text)`, keydown → `onKey(evt)`. Save message as prompt: `onSaveDraft()`. Retry: `onRetry()`. Mouse down anywhere in the palette except on an input has its default prevented, so the composer (or the search) keeps focus and the caret while clicking. |
| Highlight | `index` is clamped to the last row; the highlighted row is scrolled into view (`block: 'nearest'`) whenever `index` or `query` changes. |
| Props | `prompts`, `query`, `index`, `withSearch`, `onQuery`, `onIndex`, `onPick`, `onEdit`, `onToggleFavorite`, `canSaveDraft`, `onSaveDraft`, `onRetry`, `onKey`. |
| Accessibility | No listbox roles; rows are clickable `div`s (not focusable); the active row is shown only by class. The search is wrapped in a `label` with no text (the icon is hidden), so it has no accessible name beyond its placeholder. |
### I.6 `PromptList` (`web/app.js:1832-1972`)

The prompt library in the app sidebar (shown under `PromptsSection` while it is unfolded). Selecting a prompt opens it in Bench's editor pane (`PromptEditor`).

| Field | What |
|---|---|
| What it shows | One row per visible prompt: the title or `Untitled prompt`; the compact `AddedByMark` when a model added it; when it has tags, one small dot per tag coloured by `tagColor`; then two action buttons: a star (`star` 13 px, class `favorite`, title and aria-label `Favorite prompt` / `Unfavorite prompt`) and a delete button (the `x` icon 13 px, title and aria-label `Delete prompt`; when armed it shows the word `Delete`, class `armed`, title `Click again to delete`, aria-label `Confirm delete`). The active prompt's row has class `active`. When the library has prompts but none is visible: `No prompts match.` (when the library is empty nothing is shown at all). |
| Filter | Text: trimmed lowercased query as a substring of title, text or any tag. Tags: when any tags are chosen, a prompt shows when it has at least one of them (any, not all). Both must hold. |
| Controls | Row click: `onSwitch(prompt.id)`, except the click that ends a drag. Star: `onToggleFavorite(prompt.id)`, does not select. Delete: first click arms it for that prompt (only one prompt armed at a time; arming another moves the arm), a second click on the same button within 3 s calls `onDelete(prompt.id)`; the click does not select. Drag (pointer, primary button, not starting on a button, only while there is no text query and no tag filter): pressing a row and moving 4 px or more starts the drag; the pointer is captured, the page cursor becomes a grab cursor (`body.grabbing`), the row follows the pointer vertically (clamped to the list's top and bottom), other rows slide by one slot (the lifted row's height plus the gap) once the lifted row's leading edge passes their middle; holding the pointer within 28 px of the list's top or bottom edge scrolls the list 8 px per pointer move; release (or cancel) animates the row into its slot (class `dropping`), and after 150 ms calls `onReorder(from, to)` (indexes in the visible list, which is the whole library since dragging needs no filter) when it moved. A press under 4 px is a click. There is no keyboard reorder. |
| States | Normal; filtered (no dragging); armed delete (3 s); dragging (list class `sorting`, row class `lifted`); dropping (150 ms); empty match. |
| Timers | Delete arm: 3,000 ms. Drop settle: 150 ms. Drag threshold 4 px; edge auto-scroll zone 28 px, step 8 px per move event. |
| Props | `prompts`, `query`, `tags` (chosen tags), `activeId`, `onSwitch(id)`, `onReorder(from, to)`, `onToggleFavorite(id)`, `onDelete(id)`. |
| Oddities | Rows are `div`s with click handlers and are not keyboard-focusable; the action buttons have no `type` attribute (they are not in a form, so harmless). If the list unmounts mid-drag the grab cursor is removed (line 1861). Auto-scroll only happens while the pointer moves; holding still at the edge does not keep scrolling. |

### I.7 `PromptsSection` (`web/app.js:1976-2064`)

The library's sidebar section: its header folds it, the search button opens into a bar in the same row, and the filter inside that bar narrows by tag.

| Field | What |
|---|---|
| What it shows | A header button: a `chevron-right` 12 px (rotated by CSS when open), `Prompts`, and when tags are chosen a row of their coloured dots (`aria-label="Filtered by tag"`). In the same row a search area: a search icon button (`search` 14 px, aria-label `Search prompts`) that, when pressed, gives way to an input (aria-label `Search prompts`, no placeholder) and, when the library has any tags, a filter button (`filter` 14 px, aria-label `Filter by tag`, class `active` when tags are chosen) that opens a tag menu. A `plus` 14 px button (title and aria-label `New prompt`). Below, when unfolded, `PromptList`. |
| Tag menu | `role="menu"`; one `role="menuitemcheckbox"` per tag in alphabetical order (every tag used by any prompt): a dot in the tag's colour, the tag name, and a `check` 13 px when chosen. Click toggles the tag; the menu stays open. Closes on Escape or a click outside (`usePopover(110)`). No arrow-key navigation inside it. |
| Controls | Header click: fold or unfold. Search icon: unfolds the section, opens the search bar, focuses the input on the next frame. Input: filters as typed; Escape ends the search (clears the query and closes the tag menu). A mouse press anywhere outside the search area ends the search too. `New prompt`: `onCreate()`. |
| States | Folded / unfolded; searching (class `searching` on the section; the icon button leaves the Tab order and the input and filter button join it); tag menu open/closing; tags chosen. Chosen tags that no prompt has any more are dropped from the filter shown (`shownTags`), though they stay in state and return if a prompt gets the tag back. Ending the search clears the text but keeps the chosen tags. |
| Persistence | `localStorage['mcb-prompts-open']` = `open` or `closed`; anything but `closed` (including missing) reads as unfolded. Written on every fold change and on mount. |
| Accessibility | Header `aria-expanded`. Filter button `aria-expanded`. |
| Props | `prompts`, `onCreate()`, and the rest passed through to `PromptList` (`activeId`, `onSwitch`, `onReorder`, `onToggleFavorite`, `onDelete`). |

## Part J. Sidebar search filters

### J.1 `FILTER_GROUPS` and `filterLabels(offer, providers)` (`web/app.js:2071-2096`)

Group headings: `size` → `Size`, `prompt` → `Prompt`, `model` → `Model` (in the order of `GROUPS`: size, prompt, model). `filterLabels` gives each offered value its menu text, its chip text, an optional note, an optional logo and the full name used for the hover title and the chip's remove label:

| Group | Menu text | Chip | Note | Logo | Full |
|---|---|---|---|---|---|
| size | the stop's label (`Standard`, `High (2×)`, `Very high (5×)`, `Maximum (10×)`) | `Standard` for 1, else the short (`2×`, `5×`, `10×`) | none | none | `Size: <label>` |
| prompt | the title or `Untitled prompt` | same | `Deleted` when deleted from the library | none | `Prompt: <title>`, plus ` (deleted from the library)` |
| model | the model id after its last `/` | same | the provider's label, only when two offered models share that short name | `vendorIcon(provider, id)` | `Model: <id> (<provider label>)`; an unknown provider shows its key |

### J.2 `FindFilter` (`web/app.js:2098-2174`)

| Field | What |
|---|---|
| What it shows | A filter button (`filter` 14 px, title `Filter`, aria-label `Filter` or `Filter, <n> chosen`, class `active` when any is chosen, `aria-haspopup="menu"`, `aria-expanded`). When open, a menu (`role="menu"`, aria-label `Filter`) with three groups (`role="group"`, labelled by their heading `Size`, `Prompt`, `Model`); under `Prompt`, when no row came from a library prompt, the text `emptyPrompts` (supplied by App). Each value is a `role="menuitemcheckbox"` button with `aria-checked`, class `selected`, title the full name, the logo (14 px) for models, the text, the note in `em` when any, and a `check` 13 px (always present; CSS shows it only when selected). |
| Controls | Button click toggles the menu. Value click: `onToggle(group, value)`; the menu stays open. Keys in the menu: ArrowDown / ArrowUp move focus to the next / previous value with wrap-around, Home to the first, End to the last (default prevented); Escape closes (stops propagation) and returns focus to the button. Tab moving focus out of the menu closes it. A click outside closes it (`usePopover(110)`; that path does not move focus). |
| Focus | Opening focuses the first value. |
| What is offered | `offered()` (`filters.mjs:63-77`): every resolution stop; the prompts the listed rows came from, in library order, then the deleted ones by title; the rows' models sorted by model id then by full key. Computed from every row the mode lists, not only those shown, so one choice never takes the others away. |
| Props | `offer`, `labels` (from `filterLabels`), `filters` (`{ size: [], prompt: [], model: [] }`), `emptyPrompts` (text), `buttonRef` (shared with `FilterChips` so focus can return to the button), `onToggle(group, value)`. |

### J.3 `FilterChips` (`web/app.js:2180-2209`)

| Field | What |
|---|---|
| What it shows | A row (`role="group"`, aria-label `Filters`): one chip per value in effect, groups in order size, prompt, model, values in the order chosen. A chip: the logo 12 px for models, the chip text, title the full name, and a remove button (`x` 12 px, aria-label `Remove filter <full>`). Last, a button `Clear all`. |
| Controls | Remove: `onToggle(group, value)` and focus moves to the next chip's remove button, else the previous one, else the filter button. `Clear all`: `onClear()` and focus to the filter button. |
| Props | `labels`, `filters` (the values in effect: `inEffect` drops chosen prompts or models that no listed row has any more; they come back when a row has them, test `a chosen prompt or model no listed row has is not in effect, and is again once a row has it`), `buttonRef`, `onToggle`, `onClear`. When the row is shown at all (only while filtering) is App's. |

### J.4 `filters.mjs` (85 lines)

`GROUPS` = `size`, `prompt`, `model`. `NO_FILTERS` all empty. `modelKey` = `<provider>::<model>`. `filtering` = any group has a choice. `toggled` adds or removes one value. A chat's facets: its current resolution, the library prompt ids any of its messages was sent from, its current model. A bench's facets: its resolution, its rounds' prompts, and its members' models (any matches). `matches`: every group with a choice must match, and within a group any chosen value (tests `each group alone: any chosen value matches`, `groups combine: a row must match every group with a choice, and the search text`, `a bench matches by its resolution, any of its rounds' prompts and any of its models`). `listed(rows, found, filters, facetsOf)`: when the text search returned results (`found`, ranked), those rows in rank order (ids not in the page's list are dropped), else every row; then the filter (test `no filter lists every row, in the order given or the search ranked`). Only the text search goes to the server; filtering takes no request.
### I.8 `PromptEditor` (`web/app.js:2213-2509`)

The prompt editor pane opened from Bench (the list itself is `PromptList` in the sidebar). Uses `ModelPicker`, `EffortPicker`, `useModelInfo`, `effortFor`, `checkAttachmentLimits`, `readUpload`, `readMessageClipboard` and `insertClipboardText` (`clipboard.mjs`), `tagColor`, `AddedByMark`, `AppMark`.

| Field | What |
|---|---|
| No prompt active | A blank section: `AppMark` 44 px, heading `No prompts yet`, text `Save build requests you reuse. Type / in a message to insert one, and write {{style}} for parts you fill in each time.` (the `/` in a `kbd`, `{{style}}` in `code`), and a gold button with the `plus` 14 px icon and `New prompt` → `onCreate()`. |
| Title | A text input, value the title, placeholder `Untitled prompt`; every keystroke → `onUpdate(id, { title })`. |
| Added by | When a model added the prompt: the full `AddedByMark` under the title (the section has class `added-by`). |
| Tags | One chip per tag, coloured by `tagColor` (`--tag`), with a remove button (`x` 10 px, aria-label `Remove tag <tag>`). Then an input (id `prompt-tag-input`, placeholder `Add tag` only while the prompt has no tags) with suggestions from a `datalist` (id `prompt-tag-suggestions`) of every tag used by any prompt and not already on this one. Adding a tag: trimmed and lowercased; empty or duplicate tags are ignored; then `onUpdate(id, { tags: [...tags, tag] })`. A tag is added on Enter (default prevented), on typing a comma (the first comma is removed and the text before and after it is added as one tag), and on blur. Backspace with an empty input removes the last tag. |
| Body | A textarea, value the text, placeholder `Describe the build. Write {{like this}} for parts to fill in when you insert it.`; every keystroke → `onUpdate(id, { text })`. Paste: see below. |
| Footer, attachments | An attach button (`plus` 16 px, title and aria-label `Attach file`) that opens a hidden multi-file input; the files are checked by `checkAttachmentLimits` (8 files, 200 MB) and read by `readUpload`, then appended to the prompt it was opened on (`onUpdate(id, fn)` with a function, so files land on the prompt they were added to even if another is active by then). The input is cleared afterwards so the same file can be picked again. Each attachment: an image thumbnail (data URL, empty alt) or a `file` 16 px tile; the name in bold (title the name); the subtype (`type` after its `/`, or `file`); a remove button (`x` 12 px, title and aria-label `Remove attachment`). Errors (too many, too large, empty file) → `onError`. Drag and drop of files is not handled here. |
| Paste into the body | First, a message copied from the page (`readMessageClipboard`: HTML with a `data-mcbench-message` element): the default is prevented, the limits are checked, its text is inserted at the caret (replacing the selection; the caret is put after it on the next frame) and its attachments appended. A malformed copied message throws `The pasted message holds an attachment that is not a file` → `onError`. Otherwise, pasted files (clipboard items of kind file, for example a screenshot): default prevented, added as attachments. Otherwise the browser's normal text paste. |
| Generation model | A `ModelPicker` with label `Generation model` (so the empty label is `Choose generation model`); choosing a model resets the effort to the model default (`''`). When the model has reasoning efforts: an `EffortPicker` with label `Generation reasoning effort`. |
| AI button (split button) | Main part: `sparkles` 14 px and `Improve with AI` (source `text`) or `Write with AI` (source `note`), or `Improving…` while busy (both modes). Tooltip (`data-tip`) `Rewrites the text above` or `Writes the prompt from a short description`. Disabled while not ready (no model, the model's provider not connected, or busy) and, in text mode, while the body is blank. Text mode click: sends the body. Note mode click: opens or closes the note box. The chevron part (`chevron-down` 12 px, aria-label `Choose what the AI works from`) opens a menu (`role="menu"`) of two `menuitemradio` options with `aria-checked`, a `check` 14 px on the current: `Improve the text` / `Rewrites what is in the editor`, and `Write from a note` / `Opens a box for a short description`. Choosing one sets the source and closes the menu. |
| Note box | A form with a three-row textarea (id `improve-note`, placeholder `e.g. a lighthouse on a cliff, stormy`) that receives focus when it opens, and a gold `Write` button disabled unless ready and the note is not blank. Enter without Shift (and not while composing with an input method) sends; Shift+Enter is a newline. |
| Menu and note closing | While the menu or the note box is open, any Escape key anywhere in the document, or a mouse press outside the split button area, closes both. |
| Request | `POST /api/prompts/generate` body `{ provider, model, effort: effortFor(caps, effort), text }` where text is the body (text mode) or the note (note mode). Sending closes the note box and clears the error. Only one request at a time; a new one aborts the old one. On success, if the active prompt is still the same prompt with the same text as when sent, `onUpdate(id, { text: result.text })` replaces the body and the note is cleared; otherwise the error `The prompt changed while the model was writing. Try again.` is shown and nothing changes. On failure the error message is shown (unless aborted). Switching to another prompt or unmounting aborts the request, clears busy, the error and the tag draft. |
| Error display | `span.pe-ai-error` with the message as text and as title, to the left of the model picker. It stays until the next send or a prompt switch. |
| Persistence | `localStorage['mcb-prompt-ai-model']` = JSON `{ provider, model, effort }`, written whenever the generation model or effort changes and both provider and model are set; read once per page load (G.2), falling back to `mcb-last-model`. `localStorage['mcb-improve-source']` = `text` or `note` (anything else reads as `text`), written on every change and on mount. |
| Props | `prompts`, `activeId`, `providers`, `favorites`, `onToggleFavorite`, `onSetup`, `onCreate()`, `onUpdate(id, patch | fn)` (a patch object, or a function of the current prompt returning a patch), `onError(error)`. |
| Oddities | The title and body save on every keystroke through `onUpdate` (how App batches or debounces the save is App's / `prompt-sync.mjs`). The busy label reads `Improving…` in note mode as well. The menu and note close on Escape but do not stop the Escape from reaching other handlers. |

## Part C, continued. Transcript pieces

### C.8 `Fold` (`web/app.js:2513-2521`)

A disclosure (`details`/`summary`) whose body is built only while it is open, so a long transcript's closed folds (tool code, outputs, reasoning) cost nothing until opened. Starts closed every time it is mounted (state is not persisted). Click on the summary (or Enter/Space on it) toggles. Props `className`, `summaryClass`, `title` (the summary's tooltip), `summary` (content), `body` (a function returning the content).

### C.9 `Keyed` (`web/app.js:2524-2526`)

Renders its children unchanged; it exists to give a cached transcript entry a keyed slot (`entryKeys` in `transcript.mjs`, App's). Nothing visible.

## Part K. Composer pieces

### K.1 `SelectionChip` (`web/app.js:2531-2555`)

A box of blocks picked in the viewer, shown as a chip (in the composer while drafting, `draft` true, and in sent messages).

| Field | What |
|---|---|
| What it shows | The block's icon (`/api/blocks/<id without minecraft:>/icon.png` via `blockIconUrl`, line 3062, empty alt) or an empty icon square when the selection has no block; the label (`selection.label`, a letter-based name given by App/viewer); the size as `<x>×<y>×<z>` (max − min + 1 per axis). Title `<blocks> blocks`. In draft mode a remove button (`x` 12 px, aria-label `Remove <label>`). |
| Controls | Hover in: `onHover({ ...selection, draft })` (the viewer lights the box). Hover out: `onHover(null)`. Right-click in draft mode: default menu prevented, `onRemove(selection.letter)`. Remove button: `onRemove(selection.letter)`. When the chip goes away while hovered (sent, removed, another chat), `onHover(null)` is called so the light goes out. |
| Props | `selection` (`{ min, max, blocks, block, label, letter, … }`), `draft`, `onHover`, `onRemove`. |

### K.2 `ComposerInput` (`web/app.js:2560-2614`; `web/composer.mjs`)

The composer's editable surface: a `contentEditable` `div.composer-input` created once and owned outside Preact, with `role="textbox"`, `aria-multiline="true"`, spellcheck on. Its content is plain text plus two kinds of inline atoms (`composer.mjs`): a prompt token (`/Title`, standing for a saved prompt and sending its text) and a field (a small input box made from a prompt's `{{fill-in}}`).

| Field | What |
|---|---|
| Prompt token | A non-editable span `ce-prompt` showing `/` and the title (or `Untitled prompt`), title (hover) the prompt's text. Inside the `/` there is a small remove button (an x, 10 px) with aria-label `Remove /<title>`, out of the Tab order; clicking it removes the token, refocuses the composer and reports the change. |
| Field | A non-editable span `ce-field` holding an input, placeholder the fill-in's name, no spellcheck, width `max(length of value, length of placeholder, 2) + 1` characters, growing as typed. |
| Controls | Typing (`input`): reads the content (`composer.read`) and calls `onChange({ segments, text, emptyField })`. Keydown → `onKeyDown(evt)` (App handles Enter to send, the `/` palette keys, and so on). Paste → `onPaste(evt)`. The document's `selectionchange`: when the caret is inside the composer, `onCaret(range)` with a copy of the caret range (App keeps it to insert prompts later). |
| Reading (`composer.read`) | Segments in order; text nodes and `<br>` (as newline) become text; a `div` or `p` (which browsers insert on Enter in contentEditable) starts a new line; zero-width spaces are removed; a token contributes its prompt's text to the sent text; a field contributes its typed value; `emptyField` is the first field whose value is blank. |
| Writing | When `value.version` changes, the content is replaced by `value.segments` (`composer.write`), before the next event so a key right after a rewrite reads the new content. The host gets class `empty` while the content is blank (no atom and no non-space text), which shows the placeholder. |
| Placeholder | `placeholder` is put on the editor as `data-placeholder` (drawn by CSS). |
| Inserting (`composer.insert`, used by App) | Replaces the given range with the segments, adds a zero-width space after them so the caret has a place after a trailing atom, puts the caret there, and returns the first field's input so the caller can focus it. `promptSegments(prompt)`: a prompt without fill-ins becomes one token; with fill-ins, its text with a field in place of each fill-in (field name trimmed), each field remembering the prompt it came from. `promptLinks(segments)`: the library prompts a message was inserted from, once each, in order, counted while their token or any of their fields remains (tests in `test/composer.test.js`: `a prompt arrives as a token, or as its text with boxes that name the prompt they came from`; `a message names each prompt it was inserted from once, while its token or any of its boxes is in it`). `slashAtCaret(root)`: `/query` just before a collapsed caret in a text node, at the start or after whitespace, with no space or `/` in the query; returns where it starts and the query. `rangeAtEnd`, `caretRange`, `fields` are helpers. |
| Props | `inputRef` (receives the editor element), `value` (`{ version, segments }`), `placeholder`, `onChange`, `onKeyDown`, `onPaste`, `onCaret`. |
| Oddities | The editor element is created by hand, so it is not re-created on re-render; a token's remove button is an `innerHTML` SVG. A field's input is inside a non-editable span: Tab moves between fields and other controls in DOM order. |

### K.3 Key binds: `MAC`, `BIND_ACTIONS`, `DEFAULT_BINDS`, `comboOf`, `formatBind` (`web/app.js:2619-2646`)

`MAC` is true when `navigator.platform` contains `Mac`, `iPhone` or `iPad`. "Mod" is Cmd on a Mac and Ctrl elsewhere. The rebindable actions (id, label, default):

| Id | Label (Settings row title) | Default |
|---|---|---|
| `build` | `Switch to Build` | `Mod+1` |
| `bench` | `Switch to Bench` | `Mod+2` |
| `search` | `Search everything` | `Mod+K` |
| `new` | `New chat or bench` | `Mod+Shift+O` |
| `sidebar` | `Show or hide the sidebar` | `Mod+B` |
| `select` | `Select blocks in the viewer` | `Alt+S` |

What each does is App's (and the viewer's for `select`, see `BlueMapViewer`). `comboOf(evt)`: null for a bare modifier; the key is the letter from `KeyA`…`KeyZ` codes, the digit from `Digit0`…`Digit9` codes (so the physical key counts, not the layout's character or Shift's symbol), else a one-character `key` uppercased, else the key name (for example `ArrowUp`, `F5`, `Enter`). A combination needs Mod, Alt, or (on a Mac) Ctrl, or is an F key (`F1` to `F99` by the pattern); otherwise null, so typing never triggers one. The string is the parts present in the order `Mod`, `Ctrl` (Mac only), `Alt`, `Shift`, key, joined by `+`. On non-Mac, Ctrl is Mod, so there is no separate `Ctrl` part. `formatBind(combo)`: `Not set` for an empty bind; on a Mac `⌘`, `⌃`, `⌥`, `⇧` and the key with no separators (`⌘⇧O`); elsewhere `Ctrl`, `Alt`, `Shift` joined with `+` (`Ctrl+Shift+O`).

### K.4 `BindsSection` (`web/app.js:2650-2696`)

| Field | What |
|---|---|
| What it shows | A Settings section `Shortcuts`; when any bind differs from its default, a head action button `Reset all`. One row per action (title = the label) with a key button showing `formatBind(bind)` (class `unset` and text `Not set` when empty), or `Press a shortcut` while recording (class `recording`). Title `Click to change`, or `Press a shortcut, or Escape to cancel` while recording. |
| Controls | Key button click: starts recording for that action (or stops when it is already recording). While recording, every key press anywhere in the window is captured first (capture-phase listener on `window`), its default prevented and its propagation stopped, so it is not run as a shortcut: Escape cancels and keeps the old bind; a bare modifier or a key without a modifier is ignored and recording continues; a valid combination calls `onChange(id, combo)` and stops. A mouse press anywhere except on the recording button cancels. `Reset all`: stops recording and calls `onReset()`. |
| States | Idle; recording one action; changed (Reset all shown). No conflict check: two actions can be given the same combination. There is no way to clear a bind to `Not set` from this UI (Escape cancels); `Not set` appears only if the stored binds lack one. |
| Props | `binds` (id → combo), `onChange(id, combo)`, `onReset()`. Where binds are stored and applied is App's. |

### K.5 `ModeSwitcher` (`web/app.js:2700-2714`)

The wordmark is the switch between the two workspaces: it reads `mc` followed by the current mode word (`build` or `bench`, so `mcbuild` or `mcbench`) as a button. Inside the button a swap area with the `swap` icon 12 px and `dot` (an element App supplies: a working or finished indicator for the other mode). Title: `Switch to Bench` / `Switch to Build`, plus ` · <waiting> finished` when `waiting` is set, else ` · working` when `dot` is set, plus ` (<formatted bind>)` when the other mode has a bind. Click: marks itself `swapped` (the CSS flash) and calls `onChange(other)`. The button is keyed by mode, so it is rebuilt (and the swapped flag reset) when the mode changes. Props `mode`, `binds`, `dot`, `waiting` (a count or text of finished items), `onChange(mode)`.

### K.6 `Elapsed` (`web/app.js:2716-2723`)

`span.elapsed` with `formatElapsed(now − since)`, updated every 1,000 ms. Prop `since` (ms epoch).

### K.7 `command.mjs` (15 lines)

The command panel's highlight belongs to an item by its key, not to a position: `highlighted(keys, active)` is `active` while it is listed, else the first key (or null); `stepped(keys, active, step)` moves `step` items on from the highlighted one, wrapping round. Tests `test/command.test.js`: `the highlight stays on its item when groups arrive in front of it`, `the arrows step from the highlighted item and wrap round`. The panel itself is App's.
## Part L. Bench pieces

### L.1 `BenchModelChip` (`web/app.js:2728-2768`)

One model chosen for a new bench, in the new-bench composer.

| Field | What |
|---|---|
| What it shows | A chip with two parts. The model part is a button: the vendor `Logo` 13 px and, in the same slot, an `x` 11 px (CSS swaps the logo for the x on hover), then the short name (model id after its last `/`, title the full id). Button title `Remove`, aria-label `Remove <name>`. When the model has reasoning efforts, an effort part: the current effort word and a `chevron-down` 10 px (`aria-haspopup="menu"`, `aria-expanded`, class `open`). The effort shown is the chip's effort, else the model's default effort. |
| Effort menu | `role="menu"`, aria-label `<name> reasoning effort`. One row per effort in the catalog's order: a `menuitemradio` button (`aria-checked`, a `check` 14 px on the current) that sets the effort and closes; and, for each effort that no chip of this same model already runs (comparing each taken effort, with `''` read as the default), a `plus` 13 px button (title and aria-label `Also run <name> at <effort>`) that adds the same model again at that effort and closes. |
| Controls | Model part click: `onRemove()`. Effort option: `onEffort(level)`. Plus: `onCopy(level, effort)` (the new effort and this chip's current one). Escape or outside click closes (`usePopover(110)`). |
| Props | `entry` (`{ provider, model, effort }`), `taken` (the efforts of every chip with this provider and model, the caller's list), `onEffort(level)`, `onCopy(level, currentEffort)`, `onRemove()`. |

### L.2 `memberName(row, rows)` (`web/app.js:2772-2775`)

A bench chat's name: the model's short name, plus ` · <effort>` (or ` · default`) when the same bench runs the same provider and model more than once.

### L.3 `roundState(member, round)` (`web/app.js:2779-2783`)

Where one model stands on round `round` (1-based); `member.round` is how many rounds it was sent. Earlier rounds: `done`. The current round: `running` while `runningSince` is set, else `member.halted` (the server's `failed` or `stopped`) or `done`. Later rounds: `held` when halted (a failed or stopped turn holds the rest), else `queued`.

### L.4 `hidesNames`, `benchLetter` (`web/app.js:2790-2791`)

A blind bench that is not revealed hides which model is which: `hidesNames(bench)` = `bench.blind && !bench.revealed`. `benchLetter(bench, memberId)` = `A` + the member's index in `bench.order` (the letters run in a random order the server sets). Comment 2785-2789 belongs to `BenchResultRow`, and the comment 2793-2795 belongs to `ConfirmButton`; both sit above other code (misplaced comments).

### L.5 `BenchResultRow` (`web/app.js:3004-3053`)

One model on one round of a bench: where it stands, its effort, time and tokens, and the way into its chat.

| Field | What |
|---|---|
| What it shows | A row (`role="listitem"`, classes the state and `picked`). In order: the status slot; a letter badge (blind) or the vendor `Logo` 14 px; the name (`Build <letter>` when blind, else the short model name, title the full id) followed by the effort word (not when blind; the member's effort or the model's default); a token stat `<n> tok` (input plus output, by `formatTokens`; title `<in> in · <out> out`; empty while running or blind); a time stat (a live `Elapsed` from `runningSince` while running and not blind; when finished, `formatElapsed(stats.ms)`; empty when queued, held, blind or no time); the vote or stop slot; an open button (`open` 14 px, title `Open chat`, aria-label `Open <name>'s chat`). |
| Status slot | `done`: `check` 14 px (`role="img"`, aria-label `Done`). `running`: a pulsing dot (aria-label `Working`). `failed` or `stopped`: a retry button (`refresh` 13 px, title `Failed. Retry this round` or `Stopped. Retry this round`, aria-label `Retry <name>`, disabled while its request is pending). `queued` / `held`: a waiting mark (aria-label `Queued` or `Waiting for a retry`). |
| Vote slot | When `onVote` is given (the round can be voted on): a toggle (`trophy` 13 px, plus `Best` when picked; title `Pick as the best build` or `Your pick for this round. Click to take it back`; aria-label `Pick <name> as the best build` or `Take back the pick of <name>`; `aria-pressed`). Else while running: a stop button (`stop` 18 px, title and aria-label `Stop <name>`, disabled while pending). Else when picked: a static `trophy` and `Best` mark (aria-label `Your blind pick for this round: <name>`). Else an empty placeholder. |
| Controls | Row click: `onFocus()` (App shows that model's build). Vote, open, stop and retry clicks do not reach the row. Stop and retry call the handler, which returns a promise; the button is disabled until it settles (`pending`), errors are the handler's. |
| Blind rule | When `letter` is set the row shows only the letter: no logo, effort, time or tokens (they would give the model away). |
| Props | `member` (`provider`, `model`, `effort`, `runningSince`), `letter`, `picked`, `onVote`, `state` (from `roundState`), `stats` (`{ inputTokens, outputTokens, ms }`), `onFocus`, `onOpen`, `onStop()`, `onRetry()`. |
| Oddities | `tokens` is `0` when stats exist with no tokens, which renders as nothing because of the `tokens && …` test, but a `0` value could render literally in the title condition; ambiguous in practice (the title is an empty string when tokens is falsy). The open button has no `type` (harmless outside a form). |

### L.6 `memberDot(member, activeId)` (`web/app.js:3055-3059`)

The dot class for a bench member: `running` while running, `halted` when halted, `unread` when it has unread output and is not the active chat, else `idle`.

### L.7 `bench-details.mjs` (38 lines)

A bench's details (its rounds, stats and votes) as the page holds them: `loading` (`data` from an earlier read, still shown, or null), `loaded`, `failed` (`error`, earlier data kept). Moves: loading → loading, loaded, failed; loaded → loading; failed → loading; a bad move throws `Bench details cannot go from <a> to <b>`. `detailsRead`/`detailsFailed` throw when no read was begun. `detailsVotes(details, votes)`: a vote cast here (or all votes cleared) replaces the votes in the held data whatever read is on its way; throws when never read. Tests: `bench details are loading, loaded or failed, and a failed read can be made again`; `what was read stays on show through a later read, and one that fails`.

## Part M. Archive

### M.1 `ArchivedRow` (`web/app.js:2800-2823`)

An archived chat or bench in the sidebar's archive list.

| Field | What |
|---|---|
| What it shows | The title (truncated by CSS, full title on hover); a meta text (`meta`, the caller's, normally `timeLeft`), replaced while armed by `Delete for good?` (`role="status"`, class `armed`); a restore button (`restart` 14 px, title `Restore to the list and open it`, aria-label `Restore`); a delete button (`trash` 14 px; title `Delete permanently: this cannot be undone`, aria-label `Delete permanently`; when armed, class `armed`, title `Click again to delete it for good`, aria-label `Delete for good`). |
| Controls | Restore: `onRestore()`. Delete: first click arms for 4 s; a second click within that time disarms and calls `onDelete()`. |
| Timers | Arm 4,000 ms. |
| Props | `title`, `meta`, `onRestore()`, `onDelete()`. |

### M.2 `archive.mjs` (49 lines)

The sidebar's archived items, one mode at a time: `closed`; `loading { mode, data, error }` (earlier data still shown, but only when it is the same mode); `loaded` (`data` = `{ conversations, benches }`); `failed` (`error`). Moves: closed → loading; loading → loading, loaded, failed, closed; loaded → loading, closed; failed → loading, closed (`The archive cannot go from <a> to <b>`). `archivedRows` gives the mode's rows, null before the first read, and throws on a closed archive. `timeLeft(expiresAt, now)`: under a day `less than a day left`; otherwise whole days rounded up, `1 day left` or `<n> days left` (an item archived a moment ago shows `7 days left`). Tests: `the archive opens loading, lands or fails, can be read again, and closes`; `a read again keeps the rows on show in the same mode, not in the other`; `time left counts whole days rounded up, and under one day says so`.

## Part N. Share and published

### N.1 `ConfirmButton` (`web/app.js:2825-2842`)

An action that cannot be undone: the first click asks, the second acts. It is armed for the `subject` it was clicked on, so the same button shown for another subject asks again.

| Field | What |
|---|---|
| What it shows | A quiet header-style button with `Icon icon` 14 px and `label`, or `confirmLabel` while armed (class `armed`). Title `title` (unchanged when armed). |
| Controls | First click arms (for this subject); second click within 4 s disarms and calls `onConfirm()`. |
| States | Idle; armed (4,000 ms); disabled (`disabled` prop). |
| Props | `subject` (string key), `icon`, `label`, `confirmLabel`, `title`, `disabled`, `onConfirm()`. |

### N.2 `useCopy(onError)` (`web/app.js:2924-2931`)

Copies text to the clipboard and remembers the key of the copy that landed for 1,500 ms (so its button can say `Copied`); a later copy of another key takes over; a failure calls `onError(error)`.

### N.3 `PublishedBuilds` (`web/app.js:2847-2920`; `web/published.mjs`)

Settings' list of published builds, under the repository field (`id="published-builds"`).

| View (`publishedView`) | What shows |
|---|---|
| `loading` (before the first read lands) | `Loading published builds…` (`role="status"`) |
| `failed` | the error text and a `Retry` button → `onRetry()` (`role="alert"`) |
| `no-repo` | `Set a repository to publish builds.` |
| `empty` | `No builds are published.` |
| `list` | `<n> build(s) at <owner/name>` with the repository as a link to `https://github.com/<repo>` (new tab, `rel="noopener"`), then one row per build |

A row (class `bench` or `chat`, `removing` while its removal is on the way): a `Bench` tag for a bench; the title (title attribute the title); a meta line: for a bench `<n> model(s)` (hover lists the labels joined by `, `), for a chat its first label (the model), then ` · published <ago>` (hover the local date and time), then ` · <kind> deleted` when the chat or bench it came from is gone, or ` · archived` when it is archived; the row's last removal error, when any (`role="alert"`). Actions: `Open` (`open` 14 px, a link to the page, new tab); `Copy link` (`copy` 14 px), reading `Copied` for 1.5 s after copying; `Go to bench` / `Go to chat` (`forward` 14 px) only when the subject exists and is not archived → `onGoTo(subject)`; and a `ConfirmButton` (subject `published:<shareId>`, `x` icon, label `Remove` or `Removing…`, confirm `Remove from the web?`, title `Take the public page down`, disabled while removing) → `onRemove(build)`.

`publishedAgo(iso, now)`: the largest whole unit among year (365 days), month (30 days), day, hour, minute: `1 hour ago`, `3 days ago`; under a minute `just now` (test `published times read in their largest whole unit`). The ago text is computed when the list renders and does not tick.

`published.mjs` states: `unread` (not read since Settings last opened), `loading { data, removing, errors }` (earlier data still shown), `loaded`, `failed { error }`. The list is read each time Settings open (`GET /api/shares`, App's) and goes back to unread when Settings close. Moves: unread → loading; loading → loading, loaded, failed, unread; loaded → loading, unread; failed → loading, unread. `publishedRemoving` (throws if the build is not shown or already being removed; clears its last error), `publishedRemoved` (the row leaves the list before the next read), `publishedKept` (the row stays, with the error). `publishedSubject(build)`: `{ kind: 'chat', id: conversationId }` or `{ kind: 'bench', id: benchId }`, or null when deleted; throws for an unknown kind. Tests: `the list reads, lands or fails, can be read again, and goes unread when Settings close`; `no repository and no builds are their own views`; `a removal marks its row, then drops it or keeps it with why`; `a published build names the chat or bench it came from, or null once that is deleted`.

Props: `published`, `onRetry()`, `onRemove(build)`, `onGoTo(subject)`, `onError`.

### N.4 `SharePanel` (`web/app.js:2939-2998`; `web/share.mjs`)

The Share panel under its button in the header, for a subject (a chat or a bench). `role="dialog"`, aria-label `Share this build` (chat) or `Share this bench`. A pointer press outside the panel and its parent element (the share button's wrapper) calls `onClose()`. No Escape handling inside the component (that is the layers/App's, if any).

| Share state | What shows |
|---|---|
| `unknown`, `loading`, or `none` while the repository is not read yet (`repo` undefined) | `Checking…` (muted) |
| `none`, no repository set (`repo` null) | `Set the GitHub repository for shared builds in Settings.` and a button `Open Settings` → `onSettings()` |
| `none`, repository set | bench: `Publishes this bench's builds to a public page on <repo>, where anyone can switch between the models' builds. To post on X.`; chat: `Publishes this build's 3D view to a public page on <repo>, to post on X.`; the blind note when names are hidden; a gold `Publish` → `onPublish()` |
| `exporting`, `publishing`, `waiting` | the step text (`role="status"`): `Preparing the build…`, `Uploading…`, `Waiting for GitHub Pages…` |
| `failed` | the server's error (`role="alert"`) and `Try again` → `onPublish()` |
| `shared` or `removing` | a read-only input with the page's address (aria-label `Shared page link`, selects all on focus); `Copy link` (reads `Copied` for 1.5 s; disabled while removing); a link `Post on X` to `https://x.com/intent/post?url=<encoded url>` (new tab); `The page shows the build as it was when published.` (or `bench`); the blind note when names are hidden; `Publish again` → `onPublish()` (disabled while removing); a `ConfirmButton` (subject `unshare:<kind>:<id>`, `x` icon, `Remove` or `Removing…`, confirm `Remove the page`, title `Take the public page down`, disabled while removing) → `onRemove()` |

Blind note text: `Names are hidden: the page shows the models as A, B, C. Publish again after revealing to show their names.`

`share.mjs`: one value per subject, phases `unknown`, `loading`, `none`, `shared`, `exporting`, `publishing`, `waiting`, `failed { error }`, `removing`, each with `url`. Moves: unknown → loading; loading → none, shared, unknown (read failed: read again on the next opening); none → exporting; shared → exporting, removing; exporting → exporting (the click starts on exporting and the server's first line says exporting again), publishing, failed; publishing → waiting, failed; waiting → shared, failed; failed → exporting; removing → none, shared (removal failed: still up). A bad move throws `The share cannot go from <a> to <b>`. `shareKey` = `<kind>:<id>`; `shareUrl` = `/api/conversations/<id>/share` or `/api/benches/<id>/share`; an unknown kind throws `<kind> is not a kind of share (chat, bench)`. `shareStep(share, message)`: the stream's `state` `live` → shared with the message's url; `failed` → failed with its error; otherwise that state. `shareStoppedEarly`: a stream that ended while still publishing fails with `The publish stopped before the page was live`. `readShareStream(body, onMessage)` reads NDJSON lines (same caveat as `readLines`: a last line without newline is dropped). The publish request (POST to `shareUrl`, streamed) and the removal (DELETE) are App's. Server-side tests in `test/share.test.js` (for example `a publish moves only along exporting, publishing, waiting, live, or to failed from the first three`).

Props: `subject` (`{ kind, id }`), `hiddenNames`, `share`, `repo` (string, null, or undefined until read), `onPublish()`, `onRemove()`, `onSettings()`, `onClose()`, `onError`.
## Part O. Viewer and live controls

### O.1 `blockIconUrl`, `viewable` (`web/app.js:3062-3067`)

`blockIconUrl(block)` = `/api/blocks/<block id without minecraft:>/icon.png`, or null for empty space (the texture a selection's main block breaks into). `viewable(live)`: the live status has a viewer revision and a mesher, so the viewer can show a view (the world Minecraft saved or a preview the engine placed; the viewer meshes the blueprint itself).

### O.2 `BlueMapViewer` (`web/app.js:3069-3372`)

The 3D view. Today it is an iframe holding the BlueMap viewer document (`live.viewerUrl`) and talks to it only by `postMessage` with the page's own origin; the iframe side is `bluemap-bridge.js` (the other agent's part). One BlueMap document serves every chat: once loaded it stays, behind a loading cover while another chat's world is prepared, and only its tiles and camera change (rebooting it per chat cost a third of a second, comment 3298-3300). `live` is the status snapshot of the page's `LiveWorld` (`web/live-world.mjs:190-221`); `world` is the `LiveWorld` itself.

What it shows, in order of the code:

| Condition | What shows |
|---|---|
| No viewer address known yet | A placeholder: `build` icon 20 px and the first of `live.viewerError`, `live.syncError`, `live.error`, else `Preparing the live world…` when a chat or bench is active, else `emptyText` (App's); plus `Try again` (restart) when the sync failed with no live world for the active chat |
| Viewer loaded | The iframe (title `Live Minecraft world in BlueMap`) inside `div.bluemap-shell` with `aria-busy` true until the frame confirmed painting exactly this chat's current world |
| Covered (the frame does not show this chat yet, or an error) | Over the iframe a cover (`role="status"`): `build` icon 20 px and the error, else `emptyText` with no active id, else `Placing every build…` for a bench (`activeId` starting with `bench:`), else `Preparing this chat’s world…` (typographic apostrophe). With a boot error a button `Reload the viewer` (reloads the iframe document); with a view error `Try again` (forgets the error and the painted frame and re-sends the view) |
| Sync error or live error | A strip `div.bluemap-mismatch` (`role="status"`) with `live.syncError` or `live.error`, plus the restart `Try again` when the sync failed with no live world |
| Not covered and the world is live | `LiveControls` |

Swapping rule (3329-3336): the cover hides another chat's world, or none yet. A newer version of the world on show (while it syncs or its tiles render) replaces the old one in a single swap: the old one stays in view meanwhile, uncovered.

Messages the page sends to the iframe:

| Type | When | Fields |
|---|---|---|
| `mcbench-lite:ping` | on every iframe load | none |
| `mcbench-lite:ui` | after ready, and whenever any of these change: active id, address, block count, conversation, status, labels, selections, bounds, binds, `typeAhead`, `full`, whether `onSelect` is set | `exportable` and `playable` (shown and live), `download` (`/api/conversations/<id>/schematic` for a chat with blocks, else null), `labels`, `selectable` (a chat and `onSelect` given), `selections` (chat only), `binds` (every non-empty combo), `typeAhead`, `full`, `bounds` (lifted by `yOffset`) |
| `mcbench-lite:refresh` | when the base (`<conversationId>:<viewerRevision>`) is not yet confirmed, re-sent every 1,000 ms until the iframe answers for that key; forced on ready and on Try again | `key`, `blueprint` (`viewerBlueprint`), `mask`, `wasm` and `pack` (from `mesher`), `frame` (the bounds, only when the conversation changed), `fresh` (true when the conversation changed: move the camera and stream tiles fresh instead of one atomic swap) |
| `mcbench-lite:frame` | right after a refresh, what the world did since that base (`world.accumulated()`), then each engine tick for the base last sent | `key`, `patch`, `entities` |
| `mcbench-lite:player` | each tick that carries the player's view (`view`, `tick`); the answer to a spawn (`view` or `error`); `gone: true` when the world dropped the player itself | as listed |
| `mcbench-lite:fly` | when `focus` changes (frames one build, `min`/`max`); and when a running world's server `frame` counter advances after its new version painted (flies to the whole world, bounds lifted by `yOffset`); the first value seen for a world is where it stands and does not fly | `min`, `max` |
| `mcbench-lite:toggle-select` | when `selectToggle` changes to a truthy value (the select shortcut pressed anywhere in the app) | none |

Messages the iframe sends (accepted only from the iframe's window and the page's origin):

| Type | What the page does |
|---|---|
| `mcbench-lite:bluemap-ready` | marks ready, forces a refresh, sends the UI |
| `mcbench-lite:refreshed` `{ key }` | if the key is the active chat's current base: confirms it (no more re-sends), records the painted frame (the cover comes off), clears the view error. The bridge answers after visible tiles had a frame to render, so the cover stays until then, empty maps included |
| `mcbench-lite:refresh-error` `{ key, error }` | for the current base: confirms it (no re-send), stops sending ticks for it, shows the error on the cover with `Try again` |
| `mcbench-lite:boot-failed` `{ error }` | shows the error with `Reload the viewer` until the iframe loads again |
| `mcbench-lite:player-spawn` `{ spawn }` | `world.spawnPlayer(spawn)`; answers `{ view }`, or `{ error: 'Another world came on show before the player stood in this one' }` if the active chat changed meanwhile, or `{ error: <message> }` |
| `mcbench-lite:player-input` `{ keys, taps, yaw, pitch }` | `world.setPlayerInput(...)`; failure → `onError` |
| `mcbench-lite:player-leave` | `world.removePlayer()` (every leave reaches the world, a spawn on its way included); failure → `onError` |
| `mcbench-lite:interact` `{ x, y, z, yaw }` | `world.input(x, y, z, yaw)` (a click on a lever, button and so on in the live world); failure → `onError` |
| `mcbench-lite:export` | `world.exportWorld(exportName)` gives `{ folder, bytes }`; the page downloads `<folder>.zip` (`application/zip`) through a temporary object URL revoked after 10 s; failure → `onError` |
| `mcbench-lite:selection` `{ min, max }` | `onSelect({ min, max })` when given |
| `mcbench-lite:deselect` `{ key }` | `onDeselect(key)` |
| `mcbench-lite:bind` `{ combo }` | `onBind(combo)` (a shortcut pressed while the iframe had focus, passed up to the app) |
| `mcbench-lite:type` `{ key }` | `onType(key)` (type-ahead: a printable key pressed in the viewer goes to the composer, App's) |
| `mcbench-lite:full` `{ on }` | `onFull(on === true)` (the viewer's full-window toggle) |
| `mcbench-lite:escape` | `onEscape()` |

On every iframe load: ready is cleared, the painted frame, view error and boot error are cleared, the last confirmed key and conversation are forgotten (a fresh document lost its camera hash, so the first refresh after ready must resend even for the same conversation), and a ping is sent.

| Field | What |
|---|---|
| Timers | Refresh re-send every 1,000 ms while unconfirmed. Export object URL revoked after 10,000 ms. |
| Persistence | Reads only through `world`/`live`; the schematic download address `/api/conversations/<id>/schematic` is handed to the iframe. |
| Props | `live`, `world`, `exportName` (the zip's base name for the export), `onError`, `activeId` (a chat id, `bench:<id>`, or empty), `labels` (build labels drawn over a bench's builds), `focus` (`{ min, max }` to fly to), `emptyText`, `selections`, `onSelect`, `onDeselect`, `binds`, `onBind`, `selectToggle` (a changing value), `typeAhead`, `onType`, `full`, `onFull`, `onEscape`. |
| Oddities | Every control inside the 3D view (the viewer's own buttons for export, play/player mode, download, select, full window) belongs to the iframe document and is listed by the other agent; this component only relays. A message handler test `event.data?.type` chain runs every `if` (no `else`), harmless. The `refresh` re-send interval runs for the component's whole life, once a second, even when nothing is pending (it returns early). |

### O.3 `LiveControls` (`web/app.js:3377-3393`)

The live world in the page's engine plays at Minecraft's 20 ticks a second; paused it steps one tick at a time; restart places the build afresh, which is all a world the engine stopped takes.

| Field | What |
|---|---|
| What it shows | A toolbar (`role="toolbar"`, aria-label `Live world`): a play/pause button (`pause` icon and title `Pause` while playing, else `play` and `Play`), a step button (`step` icon, title `Step one tick`), a restart button (`restart` icon, title `Restart from the build`), all icons 14 px; then `<ticks> ticks` with the count in the locale's number format (title `Ticks since the build was placed`). |
| Controls | Play/pause → `world.control('pause' | 'play')`. Step → `world.control('step')`. Restart → `world.control('restart')`. Failures → `onError`. |
| States | Playing (step disabled); paused; stopped by the engine (play/pause and step disabled, restart enabled). |
| Accessibility | The buttons have titles but no aria-labels or pressed state; the icon SVGs are hidden, so the accessible name comes from the title. |
| Props | `live` (`{ playing, stopped, ticks }`), `world`, `onError`. |

### O.4 Running-dot clock (`web/app.js:3399-3404`)

A document-level listener: whenever a CSS animation named `conv-pulse` starts (the running dots in lists), every `conv-pulse` animation on that element is set to start at the page's time zero, so all running dots pulse in step whatever time each began.
## Part P. Page modules used by App and listed for completeness

These modules are imported at the top of `web/app.js` and hold state that the components above read through props. Their behaviour is stated by their tests; App drives them (the other agent records how).

### P.1 `shared.mjs` (40 lines)

A cache of server data that several views read (model catalogs, model capabilities): fetched once and shared, so a view that mounts after the data landed shows it in its first frame instead of flashing a default. Each url's entry holds `value`, `error`, the current request and listeners. `load(url, { fresh })`: returns the request in flight, or starts one; `fresh` starts a new request even when one is in flight or a value is held, and only the newest request may settle the entry (an older one still answers whoever asked, and writes nothing). A reload keeps showing the old value until the new one arrives. A failed request is forgotten, so the next load asks again, and its error stays in the entry until a later success clears it. Tests `test/shared.test.js`: `a value is fetched once and shared`; `a failed request is forgotten, so the next load asks again`; `a superseded request that succeeds late does not overwrite the newer value`; `a superseded request that fails late leaves the newer request and its value alone`; `a superseded request that fails after the newer one landed sets no error`.

### P.2 `layers.mjs` (31 lines)

The layers that open over the app, topmost first: `command` (modal), `lightbox` (modal), `settings` (modal), `drawer` (modal: the narrow window's list drawer), `viewer` (not modal: the viewer filling the window), `share` (not modal: the Share panel). `topLayer(open)` is the one Escape closes. `modalOpen(open)`. `bindRuns(action, open)`: the `search` bind always runs (it opens and closes the command panel from anywhere); the `sidebar` bind runs when the drawer is the top layer (it closes the drawer it opened); every other bind waits while a modal layer is open. Tests `test/layers.test.js`: `Escape closes the topmost open layer, in the order they stack`; `binds that act on the app wait while a modal layer covers it`.

### P.3 `favorites.mjs` (74 lines)

The starred models; the list lives on the server (`modelFavorites`). A star shows at once and is saved after, each toggle on its own and in order, as the server's list with that one toggle made; a failed save undoes only its own star and calls `onError`. Phases `unloaded` (nothing can be starred: toggling throws `The favorites have not loaded, so they cannot be changed yet`), `idle`, `saving`. Tests `test/favorites.test.js`: `nothing is starred, and nothing saved, before the server's list is known`; `a star that could not be saved goes back, and says so`; `stars are saved one at a time, in order, and a failed one takes no other with it`. The route used by `write` is App's.

### P.4 `draft.mjs` (119 lines)

A place's unsent message (a chat, a new chat, a bench): `{ segments, attachments, selections, reading, choice, edit }`. `reading` lists files picked and still being read (one `{ name, size }` each, in batches, one batch per pick); a draft that is reading is not sent, and files being read count toward the attachment limits (`draftFiles`). `edit` is null or the sent prompt or round the draft is a new version of, with what the composer held before (Cancel brings it back). `draftEmpty`: an edit is never empty. `beginEdit` (throws `This draft is already an edit`), `endEdit` (throws `This draft is not an edit`), `afterSend`, `sentBack` (a message that did not go out comes back unless something new was written there since; an edit always comes back; throws `A place holds one edit at a time`). `Drafts` parks the drafts of places not on screen (a draft is parked or in the composer, never both; `park` throws `The draft of <key> is parked: it cannot be in the composer too`), `put` keeps the model picked there, `putBack`, `drop`, `endRead`. Tests `test/draft.test.js`: `files being read are part of the draft until their read ends`; `a failed read leaves the draft with what it had`; `batches end on their own, and files still being read count toward the limits`; `a draft is parked or in the composer, never both`; `a prompt put in a parked draft keeps the model picked there, and an unsent message does not replace newer text`; `a read ends in the parked draft that holds it`; `an edit puts the prompt in the composer and its cancel brings back what was there, files read meanwhile and all`; `a sent edit leaves what the composer held before it, and one that did not go out comes back with what is there now`.

### P.5 `prompt-sync.mjs` (132 lines)

The prompt library in the page; it lives on the server and the builders (models, through `prompt_library`) can change it too. Local edits show at once and reach the server after 300 ms without further edits (`delay`). Each edit is a function of the list, kept until a save that carried it succeeds, so a reload (a builder changed the library) lands the server's list with those edits made again on it. Phases `unloaded`, `loading`, `failed { error }`, `ready`, `saving`, `reloading`; one request at a time. Editing before the list is read throws `The prompt library has not loaded, so it cannot be changed yet`. A failed save keeps its edits, which go with the next save; it is not retried by itself. A failed reload leaves the list stale, read again before anything is saved over the server. `promptsLoaded(phase)` is true for ready, saving and reloading. The `PromptPalette` `onRetry` corresponds to the `failed` phase. Tests `test/prompt-sync.test.js`: `no edit is taken, and nothing is saved, before the library has loaded`; `a load cannot start while one is on its way or once it has landed`; `a failed save keeps its edits, which go again with the next one`; `an edit made while a save is on its way stays unsent until its own save`; `edits made close together are saved once`; `a reload waits for the save on its way, and an edit made during it is kept`; `a reload asked for while the first read is out follows it`; `a failed reload leaves the list stale: it is read again before anything is saved over the server`.

### P.6 Modules imported by `web/app.js` but used only by App

`transcript.mjs` (`groupItems`, `entryKeys`, `applyEvent`, turn states), `place.mjs` (draft keys and the new chat and new bench drafts), `address.mjs` (the page address of a place), `live-world.mjs` (`LiveWorld`, whose status snapshot `BlueMapViewer` and `LiveControls` read: see O.2) and `live-host.mjs` (`browserHost`) are used only inside `App` (lines 3406 onwards) and are left to that inventory. Within lines 1 to 3405 they are referenced only by the import lines 2, 6, 7, 23 and 24, and `LiveWorld` methods are called through the `world` prop in O.2 and O.3: `onFrame`, `accumulated`, `spawnPlayer`, `setPlayerInput`, `removePlayer`, `input`, `exportWorld`, `onPlayerGone`, `control('play' | 'pause' | 'step' | 'restart')`.

### P.7 `clipboard.mjs` (70 lines), `upload-image.mjs` (12 lines)

`writeMessageClipboard(message)` (used by App to copy a sent message) writes two clipboard formats in one write started inside the click: `text/plain` with the message text, and `text/html` with the text (HTML-escaped, newlines as `<br>`), one `<p>` per attachment name, and the whole message as JSON in a `data-mcbench-message` attribute, attachments carrying their bytes as data URLs so a copy survives reloads and the removal of its source. An attachment known only by its `url` is fetched; a failed fetch throws `Could not copy "<name>" (HTTP <status>)`. `readMessageClipboard(clipboardData)` rebuilds `{ text, attachments }` from such HTML (null when the HTML has no such element; any attachment without a string name or a base64 data URL throws `The pasted message holds an attachment that is not a file`); each attachment's type is taken from its data URL and its size computed from the base64 length. `insertClipboardText(textarea, text)` returns the textarea's value with `text` replacing the selection and puts the caret after it on the next frame. `readBlobAsDataUrl(blob)` reads a blob as a data URL. `paintUpload(canvas, bitmap, maxEdge)` sizes the canvas to the bitmap scaled down (never up) so its longest edge is at most `maxEdge`, fills it white, and draws the bitmap.

### P.8 `resolution.mjs`, `setup-status.mjs`, `vendor-icon.mjs`, `settings-panel.mjs`, `filters.mjs`, `composer.mjs`, `command.mjs`, `share.mjs`, `published.mjs`, `archive.mjs`, `bench-details.mjs`

Recorded where they are used: resolution in 1.4 and D.1; setup-status in F.1; vendor-icon in 1.2; settings-panel in E.9 and E.10; filters in J.1 to J.4; composer in K.2 and I.4; command in K.7; share in N.4; published in N.3; archive in M.2; bench-details in L.7.

## Checklist

| Item | Kind | Lines | Section | Status |
|---|---|---|---|---|
| imports, `api`, `readLines`, `shared`, `load` | helpers | 1-51 | A.1 | covered |
| `useShared`, `modelsUrl`, `modelInfoUrl`, `useModelInfo`, `effortFor` | hooks, helpers | 54-77 | A.1 | covered |
| `renderInline`, `Markdown`, `renderMarkdown` | helper, component | 81-142 | A.2 | covered |
| `formatElapsed` | helper | 145-150 | A.3 | covered |
| `PlanCard` | component | 154-169 | C.1 | covered |
| `NoticeLine` | component | 173-175 | C.2 | covered |
| `LIMIT_WINDOWS`, `formatTokens` | constants, helper | 177-181 | A.4 | covered |
| `ContextMeter` | component | 184-196 | C.3 | covered |
| `ResolutionPicker` | component | 203-280 | D.1 | covered |
| `ResolutionTag` | component | 283-287 | D.2 | covered |
| `LimitChip` | component | 289-295 | C.4 | covered |
| `Ticker` | component | 297-310 | C.5 | covered |
| `CODE_TOKEN`, `Code`, `highlightCode` | constant, component, helper | 314-333 | A.5 | covered |
| `itemsFromMessages` | helper | 336-401 | A.6 | covered |
| `branchNavigation` | helper | 403-424 | A.7 | covered |
| `DiffStat` | component | 427-435 | C.6 | covered |
| `MAX_IMAGE_EDGE`, `MAX_ATTACHMENT_BYTES`, `MAX_ATTACHMENTS_BYTES`, `checkAttachmentLimits`, `readImageAsDataUrl`, `readUpload` | constants, helpers | 440-470 | A.8 | covered |
| `Icon` | component | 474-532 | B.1, 1.1 | covered |
| `favoriteKey`, `BRAND` | helper, constant | 534-549 | A.9 | covered |
| `Logo` | component | 551-558 | B.2 | covered |
| `orderModels`, `displayName`, `modelLabel`, `CURATED_VENDORS`, `CURATED_LIMIT` | helpers, constants | 562-576 | A.9 | covered |
| `AppMark` | component | 581-599 | B.3 | covered |
| `storedJson`, `storedResolution`, `ago` | helpers | 601-626 | A.10 | covered |
| `Toggle` | component | 628-637 | B.4 | covered |
| `CopyButton` | component | 639-649 | B.5 | covered |
| `SettingsSection` | component | 651-658 | E.1 | covered |
| `SettingsRow` | component | 661-671 | E.2 | covered |
| `DangerZone` | component | 674-710 | E.3 | covered |
| `SettingsNavItem` | component | 712-729 | E.4 | covered |
| `MODEL_ROWS`, `ModelsSection` | constant, component | 731-875 | E.5 | covered |
| `RUNTIME_NOTES`, `SIGN_IN_METHODS`, `SIGN_IN_HELP`, `LOGIN_THROUGH`, `SIGN_IN_NEXT` | constants | 877-913 | E.6 | covered |
| `SignInRows` | component | 934-1079 | E.7, 1.7 | covered |
| `ProviderPane` | component | 1081-1146 | E.8 | covered |
| `ConnectPane` | component | 1153-1177 | E.9 | covered |
| `SetupStatus` | component | 1184-1225 | F.1 | covered |
| `SIDEBAR`, `NARROW`, `useMedia` | constants, hook | 1229-1247 | G.1 | covered |
| `initialModelSelection`, `initialPromptModelSelection` | constants | 1249-1250 | G.2 | covered |
| `usePopover` | hook | 1254-1301 | G.3 | covered |
| `ModelPicker` | component | 1303-1538 | D.3, 1.3 | covered |
| `EffortPicker` | component | 1540-1583 | D.4 | covered |
| `IDEAS`, `EGGS`, `SEASONAL`, `pickIdea` | constants, helper | 1587-1702 | H | covered |
| `TAG_COLORS`, `tagColor` | constant, helper | 1706-1712 | I.1 | covered |
| `promptSections` | helper | 1716-1730 | I.2 | covered |
| `AddedByMark` | component | 1735-1740 | I.3 | covered |
| `PromptText` | component | 1742-1751 | I.4 | covered |
| `PromptPalette` | component | 1758-1828 | I.5 | covered |
| `PromptList` | component | 1832-1972 | I.6 | covered |
| `PromptsSection` | component | 1976-2064 | I.7 | covered |
| `FILTER_GROUPS`, `filterLabels` | constant, helper | 2071-2096 | J.1 | covered |
| `FindFilter` | component | 2098-2174 | J.2 | covered |
| `FilterChips` | component | 2180-2209 | J.3 | covered |
| `PromptEditor` | component | 2213-2509 | I.8 | covered |
| `Fold` | component | 2513-2521 | C.8 | covered |
| `Keyed` | component | 2524-2526 | C.9 | covered |
| `SelectionChip` | component | 2531-2555 | K.1 | covered |
| `ComposerInput` | component | 2560-2614 | K.2 | covered |
| `MAC`, `BIND_ACTIONS`, `DEFAULT_BINDS`, `comboOf`, `formatBind` | constants, helpers | 2619-2646 | K.3, 1.6 | covered |
| `BindsSection` | component | 2650-2696 | K.4 | covered |
| `ModeSwitcher` | component | 2700-2714 | K.5 | covered |
| `Elapsed` | component | 2716-2723 | K.6 | covered |
| `BenchModelChip` | component | 2728-2768 | L.1 | covered |
| `memberName` | helper | 2772-2775 | L.2 | covered |
| `roundState` | helper | 2779-2783 | L.3 | covered |
| `hidesNames`, `benchLetter` | helpers | 2790-2791 | L.4 | covered |
| `ArchivedRow` | component | 2800-2823 | M.1 | covered |
| `ConfirmButton` | component | 2825-2842 | N.1 | covered |
| `PublishedBuilds` | component | 2847-2920 | N.3 | covered |
| `useCopy` | hook | 2924-2931 | N.2 | covered |
| `SharePanel` | component | 2939-2998 | N.4 | covered |
| `BenchResultRow` | component | 3004-3053 | L.5 | covered |
| `memberDot` | helper | 3055-3059 | L.6 | covered |
| `blockIconUrl`, `viewable` | helpers | 3062-3067 | O.1 | covered |
| `BlueMapViewer` | component | 3069-3372 | O.2 | covered |
| `LiveControls` | component | 3377-3393 | O.3 | covered |
| `animationstart` listener for `conv-pulse` | module-level listener | 3399-3404 | O.4 | covered |
| `web/composer.mjs` | module | 1-175 | K.2, I.4 | covered |
| `web/draft.mjs` | module | 1-119 | P.4 | covered |
| `web/filters.mjs` | module | 1-85 | J.4 | covered |
| `web/favorites.mjs` | module | 1-74 | P.3 | covered |
| `web/settings-panel.mjs` | module | 1-109 | E.9, E.10 | covered |
| `web/setup-status.mjs` | module | 1-68 | F.1 | covered |
| `web/resolution.mjs` | module | 1-27 | 1.4, D.1 | covered |
| `web/clipboard.mjs` | module | 1-70 | P.7, I.8 | covered |
| `web/upload-image.mjs` | module | 1-12 | P.7, A.8 | covered |
| `web/vendor-icon.mjs` | module | 1-32 | 1.2 | covered |
| `web/prompt-sync.mjs` | module | 1-132 | P.5 | covered |
| `web/share.mjs` | module | 1-110 | N.4 | covered |
| `web/published.mjs` | module | 1-102 | N.3 | covered |
| `web/shared.mjs` | module | 1-40 | P.1 | covered |
| `web/bench-details.mjs` | module | 1-38 | L.7 | covered |
| `web/command.mjs` | module | 1-15 | K.7 | covered |
| `web/layers.mjs` | module | 1-31 | P.2 | covered |
| `web/archive.mjs` | module | 1-49 | M.2 | covered |
| `web/transcript.mjs`, `web/place.mjs`, `web/address.mjs`, `web/live-world.mjs`, `web/live-host.mjs` | modules used only by App | imports at 2, 6, 7, 23, 24 | P.6 | noted; left to the App inventory |
