# Inventory: style, the viewer's interface, and the server's routes

This file records what today's browser page looks like and does, for the native rebuild in `mcbench-ui`. Part 1 is `web/style.css` (4,195 lines), Part 2 is the 3D viewer pane's own interface, and Part 3 is the server's routes as the page uses them. It covers the worktree at branch `s4-inventory`, which matches `native-stage4` at `dbcb4f4` for `web/`, `server/` and `test/`. `S` stands for `web/style.css`, so `S:762` means line 762. Other files are named in full. Values are the ones that finally apply after the cascade. Where an earlier rule is overridden, the table gives the final value and says which line wins.

## Part 1: `web/style.css`

### 1.1 How the stylesheet is layered

The file has three layers, and later rules override earlier ones.

| Lines | Layer | What it holds |
|---|---|---|
| S:1-664 | The first layer | Structure (display, flex direction, overflow, positioning) and a few colours. Most of its boxes get their final size, colour and spacing from the second layer. The tokens it uses (`--bone`, `--line`, `--raised`) are defined only in the second layer. The first layer's own `:root` (S:8-20) defines just `--sky-high`, the `--tk-*` syntax colours, `--diff-plus` and `--ease`. |
| S:666-2752 | "2026 interface pass", "Dark first, after T3 Code" | All surface, text and border tokens (S:676-741), body type, the buttons, the layout grid, the sidebar, the transcript, the composer, the pickers, settings, the narrow-window layout, the prompt palette and editor. |
| S:2753-4195 | "Patterns taken from T3 Code" and later additions | Raised-control shadows (S:2759-2781), tooltips (S:2783-2809), settings rows at T3's metrics (S:2811-2817), thin scrollbars (S:2825-2827), ghost controls in the composer (S:2828-2834), harness events, status dots, the mode switcher, the bench sidebar, selection chips, setup, the sidebar search, archived rows, shortcuts, the leaderboard, the command panel, notices and the share panel. |

These first-layer rules are fully overridden:

| Rule | Overridden by |
|---|---|
| `.btn:disabled` `box-shadow: none; transform: none; filter: none` (S:45-50) | S:782-786 sets the colours. `box-shadow: none` still applies to `.btn:disabled`, because `.btn:disabled` (0,2,0) beats `.btn` at S:2772 (0,1,0). For `.btn.gold:disabled`, though, S:2774's `.btn.gold` (0,2,0) comes later, so a disabled primary button keeps `--primary-face`. |
| `.btn:active { transform: scale(0.98); box-shadow: none }` (S:775) | `.btn:active:not(:disabled)` at S:2776 sets `scale(0.97)` and `--control-pressed`. |
| `.conv-item.active { border-color: var(--line) }` (S:122) | S:863-868 sets `border: 0`. |
| `.model-provider-rail button.active { border-color }` (S:213-215) | S:1419-1423 sets `border: 0`. |
| `.composer-box` border and transition (S:485-488) | The border colour is restated at S:1388-1393. The transition still applies. |
| `.layout` `--chat-col` fallback `minmax(420px, 38%)` (S:797, S:800) | It never applies. `web/app.js:6526` always sets `--chat-col` inline, to the dragged width in px or to `minmax(360px, 42%)`. |
| `.log-err` font (S:380-383) | It keeps `--font-data` and `line-height: 1.5`. S:1191-1196 adds the margin, colour and `11px`. |
| `.code` (S:397-402) and `.code.term` (S:404-408) | S:1309-1320 sets margin, size and colour. `.code.term` keeps `margin-top: 4px`, `color: var(--term-text)` and `max-height: 220px`. Its 4px top margin replaces the 4px of S:1310. |
| `.side-title` transition (S:3068) | S:3091 sets `color 120ms ease, flex-grow 220ms var(--ease)`, which drops the opacity transition. |
| `.settings-panel .panel-actions` (S:579-582, S:1463-1465) | S:1592-1595 adds `margin-top: -4px; padding-top: 0`. In the sidebar layout S:2536 sets `margin: 0; padding: 12px 22px 18px`. |
| `.toggle > span` with no transition (S:2513-2520) | S:2821 adds `transform 150ms ease, width 100ms ease 100ms`. |
| Every CSS `border` on raised controls | S:2909-2913 sets `border: 0` on `.btn`, `.btn:hover`, `.theme-picker`, `.cmd-chip`, the model, effort and resolution triggers in every state, and `.title-model-controls .effort-picker-trigger`. Their edge is the `--control-face` shadow ring. The comment explains that a border would push the inner top light 1px down and show a second line. Every `border-color` set on these triggers elsewhere (S:931, S:940, S:1626, S:1683) therefore draws nothing. |

### 1.2 Themes and design tokens

The theme is the `data-theme` attribute on `<html>`, either `dark` or `light`. `web/theme.js:3` sets it before first paint from `localStorage['mcb-theme']`, with `'dark'` as the default. `web/app.js:3559` and `:4146-4147` keep it in React state and write it back. Settings, Appearance, Theme offers two buttons, `Light` (with the sun icon) and `Dark`, with the tip `Applies right away on this browser` (`web/app.js:7123-7135`). There is no "system" choice. The stylesheet has no `prefers-color-scheme` rule, so the OS setting is never consulted. `color-scheme` is `dark` on `:root` (S:709) and `light` under `[data-theme="light"]` (S:740). That setting changes the browser's own form controls and scrollbars. `.viewer` and `.bluemap-frame` always use `color-scheme: dark` (S:589, S:607).

#### Colour and surface tokens

| Token | Dark (S:676-710) | Light (S:712-741) | Main uses |
|---|---|---|---|
| `--bedrock` | `#0a0a0a` | `#fafafa` | `body`, `.layout`, `.sidebar`, `.strata`, the provider rail, `.settings-nav`, `.prompt-editor`, settings inputs, `.ce-field-input`; text on primary buttons, `.send`, `.header-action`, `.ce-remove` and `.model-check.on`; the toggle thumb when on; `.conv-dot`'s base |
| `--cave` | `#0d0d0d` | `#ffffff` | `.chat`, `.settings-panel`, `.pp-preview`, the end of the composer's gradient |
| `--well` | `#111111` | `#ffffff` | `.composer-box`, `.model-picker-popover`, the effort, tag and resolution menus, `.md code`, `.prompt-palette`, `.selection-chip`, `.setup-steps`, `.connect-way`, `.plan-card`, `.round-switch`, `.bind-key` |
| `--raised` | `#1a1a1a` | `#f4f4f5` | `.btn`, `.msg.user`, the active `.conv-item`, hover rows, `.live-controls`, `.bluemap-mismatch`, `.s-input`, `.cmd-chip`, `.s-tag`, `.find-chip`, `.side-find`, `#page-error` |
| `--popover` | `#141414` | `#ffffff` | tooltips, `.pe-ai-menu`, `.pe-ai-note`, `.cmdk`, `.share-panel` |
| `--bevel-light` | `#2a2a2a` | `#ffffff` | hover borders: `.msg-file:hover`, `.bench-prompt:hover`, `.selection-chip:hover`, `.bind-key:hover`, `.bench-letter`, `.composer-toggle.on`, `.header-action.quiet.armed` |
| `--bone` | `#f5f5f5` | `#27272a` | primary text; the primary button's fill; the base of many `color-mix` tints |
| `--ash` | `#a3a3a3` | `#52525b` | secondary text |
| `--dim-text` | `#7c7c7c` | `#71717a` | tertiary text, placeholders, icons |
| `--gold` | `#ededed` | `#18181b` | the accent (a near-white or near-black, not gold): the focus outline, `.toggle.on`, the selected effort check, the strata hover line, the `.s-input` focus ring |
| `--star` | `#f0b54a` | `#c9820f` | favourite stars, `.limit-chip`, `.context-meter.high`, warning notice dots, search highlight `mark` in `.pp-text` |
| `--grass` | `#7fbf6a` | `#4f8a35` | feeds `--tk-s` and `--diff-plus` in dark only, because light overrides both |
| `--done` | `#3ecf6b` | `#1f9d4c` | the bench's done status, `.bench-result.picked`, `.bench-vote.picked` |
| `--waiting` | `#6cb4ff` | `#2b7de9` | the unread chat dot and its glow |
| `--redstone` | `#eb6a60` | `#d14a42` | error borders and tints, `.diffstat .m`, `.btn.danger`, the error notice dot, `field-missing` |
| `--hover` | `rgba(255, 255, 255, 0.06)` | `rgba(0, 0, 0, 0.05)` | row and ghost-control hover |
| `--hover-well` | `rgba(255, 255, 255, 0.04)` | `rgba(0, 0, 0, 0.03)` | settings nav hover, `.s-model:hover`, `.s-recheck:hover` |
| `--note-text` | `#a3a3a3` | `#52525b` | `.log-note`, `.reasoning-text .md-h` |
| `--reason-text` | `#8a8a8a` | `#71717a` | `.reasoning-text` |
| `--err-text` | `#f3a7a1` | `#a8322d` | error text everywhere |
| `--err-soft` | `#cc827c` | `#b9544e` | `.tool-failure`, `.log-err`, failed or stopped `.bench-status` |
| `--term-text` | `#a8a8a8` | `#52525b` | `.code.term` (bash output) |
| `--line` | `rgba(255, 255, 255, 0.06)` | `#e4e4e7` | every hairline |
| `--input-line` | `rgba(255, 255, 255, 0.08)` | `#d4d4d8` | `--field-face`, `.ce-field-input`, `.model-check`, `.pe-ai-note textarea` |
| `--sky-high` | `#6f9fdd` (S:9) | not overridden | `.ce-prompt` (a saved prompt in the composer); feeds `--tk-n` in dark |
| `--tk-c` | `#626b78` (S:12) | `#918872` (S:23) | `.tk-c`, comments in highlighted code |
| `--tk-f` | `#8fbde8` | `#23639c` | `.tk-f` |
| `--tk-n` | `var(--sky-high)` | `#2f6bb0` | `.tk-n` |
| `--tk-s` | `var(--grass)` | `#4a7a33` | `.tk-s` |
| `--tk-k` | `var(--gold)` | `#a06a10` | `.tk-k` |
| `--diff-plus` | `var(--grass)` | `#4a7c32` | `.diffstat .p` |

#### Shadow, shape, motion and type tokens

| Token | Dark | Light | Uses |
|---|---|---|---|
| `--highlight` | `inset 0 1px 0 rgba(255, 255, 255, 0.05)` (S:701) | `inset 0 -1px 0 rgba(0, 0, 0, 0.04)` | tooltips, `.pe-ai-menu`, `.pe-ai-note` |
| `--shadow-panel` | `0 0 0 1px rgba(255, 255, 255, 0.06), 0 16px 48px rgba(0, 0, 0, 0.6)` | `0 0 0 1px rgba(0, 0, 0, 0.06), 0 16px 48px rgba(0, 0, 0, 0.12)` | the model picker, `.settings-panel`, `.lightbox img`, the prompt palette, the drawer, `.cmdk`, `.share-panel`, `.pe-ai-menu` |
| `--glow-gold` | `0 0 0 3px rgba(255, 255, 255, 0.08)` | `0 0 0 3px rgba(0, 0, 0, 0.08)` | input focus rings |
| `--control-face` | `inset 0 1px 0 rgba(255, 255, 255, 0.07), 0 0 0 1px rgba(255, 255, 255, 0.09), 0 1px 2px rgba(0, 0, 0, 0.55)` (S:2760) | `inset 0 -1px 0 rgba(0, 0, 0, 0.04), 0 0 0 1px rgba(0, 0, 0, 0.12), 0 1px 2px rgba(0, 0, 0, 0.06)` (S:2767) | `.btn`, the three picker triggers, `.theme-picker`, `.pe-ai-split`, `.cmd-chip` (S:2772); `.theme-picker button.active`, `.round-switch button.active`, `.connect-pane`, the lifted `.pe-item` |
| `--control-pressed` | `inset 0 1px 2px rgba(0, 0, 0, 0.35), 0 0 0 1px rgba(255, 255, 255, 0.08)` | `inset 0 1px 2px rgba(0, 0, 0, 0.1), 0 0 0 1px rgba(0, 0, 0, 0.12)` | the `:active` state of the raised controls (S:2776) |
| `--primary-face` | `inset 0 1px 0 rgba(255, 255, 255, 0.9), inset 0 -1px 0 rgba(0, 0, 0, 0.16), 0 1px 3px rgba(0, 0, 0, 0.5)` | `inset 0 1px 0 rgba(255, 255, 255, 0.16), 0 0 0 1px rgba(0, 0, 0, 0.9), 0 1px 2px rgba(0, 0, 0, 0.2)` | `.btn.gold`, `.header-action` (S:2774) |
| `--primary-pressed` | `inset 0 1px 2px rgba(0, 0, 0, 0.25)` | `inset 0 1px 2px rgba(0, 0, 0, 0.4)` | their `:active` (S:2777) |
| `--field-face` | `0 0 0 1px var(--input-line)` (S:2764) | follows `--input-line` | `.s-input`, `.s-filter` (S:2773), `.share-url` |
| `--radius-sm` | `8px` (S:704) | same | `.log-shots img`, `.settings-error`, `.share-url` |
| `--ease` | `cubic-bezier(0.2, 0.7, 0.3, 1)` (S:19) | same | most transitions; several rules spell the same curve out literally |
| `--font-body` | `-apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif` (S:705) | same | body text |
| `--font-ui` | the same stack (S:706) | same | controls; identical to `--font-body` |
| `--font-data` | `ui-monospace, 'SF Mono', SFMono-Regular, Menlo, Consolas, monospace` (S:707) | same | code, ids, versions, inline `code` |
| root `font-size` | `14px` (S:708) | same | `em` and `rem` base |

#### Custom properties set from script or scoped to a component

| Property | Set at | Value | Read at |
|---|---|---|---|
| `--sidebar-w` | `web/app.js:6526` | the sidebar's width in px: default `236`, clamped `200` to `420`; dragged below `140` it collapses (`SIDEBAR`, `web/app.js:1229`, drag at `:4164-4198`). Persisted as `mcb-sidebar-width`; the open or closed state as `mcb-sidebar` (`:4155`). | S:797, S:1963 |
| `--chat-col` | `web/app.js:6526` | the dragged chat width in px, or `minmax(360px, 42%)` before any drag. The drag clamps to at least `320` and at most `max(room - 360, 320)`, where room is the width from the chat's left edge to the layout's right edge less the divider (`:4200-4215`). Persisted as `mcb-chat-width`. | S:797, S:800 |
| `--icon` | `web/app.js:555` | `url(/icons/<name>.svg)` for `.vlogo` | S:169-170 |
| `background` of `.vlogo` | `web/app.js:540-549, :555` | brand fills that replace `currentColor`: `claude #d97757`, `openrouter #7072f5`, `groq #f55036`, `qwen linear-gradient(135deg, #7b6cf6, #5b4fe0)`, `gemini linear-gradient(135deg, #4285f4 15%, #9b72cb 55%, #d96570 90%)`, `meta linear-gradient(135deg, #0081fb, #0064e1)`, `mistral linear-gradient(180deg, #ffd800, #ff8205 50%, #e10500)`, `deepseek #4d6bfe`. Other vendors paint in the current text colour. | S:165-171 |
| `--tag` | `web/app.js:1814, :2378` | a tag's colour, picked by hash from `#7da869`, `#c9a25e`, `#6f9aa8`, `#9a7fae`, `#a8785f`, `#a86f7e`, `#8fa86f`, `#6f7fa8` (`TAG_COLORS`, `:1706-1712`, hash `h = h * 31 + codePoint` mod 2^32) | `.pp-tags span`, `.pe-tag` |
| `--nudge` | `web/app.js:1360-1366` | how far the model popover moved sideways to stay 8px inside the window | S:2337, S:3503 |
| `--rail`, `--lines`, `--per-line` | `web/app.js:1371-1380` | the popover's height with every provider in one column; the column (or, on a phone, row) count; the providers per column. Starting from one, it adds lines while the rail overflows. | S:2327, S:2347-2348, S:2422-2423, S:3504 |
| `--below` | `web/app.js:6438` | `window.innerHeight - row.bottom - 16` px, for the new-bench lineup popover | S:3504 |
| `--transcript-icon-column` `20px`, `--transcript-action-column` `20px`, `--transcript-column-gap` `8px`, `--transcript-row-height` `24px` | S:960-963, on `.messages` | the tool-row grid | S:1113-1349 |

### 1.3 Type

Every font is the system stack. On macOS `--font-body` and `--font-ui` are both SF Pro; `--font-data` is SF Mono. `body` sets `-webkit-font-smoothing: antialiased; text-rendering: optimizeLegibility` (S:743-748). Weights such as `430`, `450`, `470`, `520`, `550`, `570`, `620` and `650` are variable-font weights that SF renders exactly. Letter spacing is `0` unless listed.

| Text | Selector (line) | Family, size / line height, weight, letter spacing, colour |
|---|---|---|
| Root | `:root` S:708 | `14px` |
| Page loading | `.page-loading` S:4117 | ui `13px`, `--dim-text`, padding `24px`. Text `Loading mcbench-lite…` (`web/index.html:16`) |
| Wordmark | `.brand .wordmark` S:834-843 | body `14px`/`1`, `650`, `-0.2px`; under 560px it is `13px` (S:1533) |
| Wordmark `em` | S:844-849 | `11px`, `500`, `--ash`, normal style; hidden under 980px (S:1500) |
| Mode word | `.mode-word` S:2994-3006 | inherits the wordmark; colour `color-mix(in srgb, var(--bone) 58%, transparent)`, `--bone` on hover |
| Sidebar rows | `.conv-item` S:852-861 | `13px`, `430`, `--ash`; `--bone` when hovered or active |
| Row snippet | `.conv-snippet` S:3691-3697 | `12px`, `--dim-text`; `mark` is `--bone` with no background |
| Bench member rows | `.bench-member` S:3045 | `12.5px` |
| Sidebar section title | `.side-title` S:3054-3069 | ui `500 13px`, `--dim-text` |
| Sidebar search | `.side-find input` S:3632-3641 | body `13px`, `--bone`; `kbd` ui `11px`, `0.5px` |
| Sidebar warning | `.sidebar .meta.warn` S:903-908, S:139-141 | body `12px`/`1.5`, `--err-text` |
| Archived rows | `.archived-row` S:3707-3716 | `13px`, `--ash`; meta `11.5px` `--dim-text`, armed `--err-text` |
| Resolution tag, published tag | `.resolution-tag` S:3894-3902 | ui `500 10.5px/16px`, `--dim-text` |
| Pane header title | `.chat-header .title` S:921-927 | body `13.5px`, `570`, `-0.1px`, `--bone`; `13px` under 560px |
| Breadcrumb | `.chat-header .crumb` S:2733-2734 | `500`, `--ash`; separator `400` `--dim-text`, margin `0 7px` |
| Header action | `.header-action` S:2735-2748 | ui `500 12.5px` |
| Transcript prose | `.msg` S:968-972 | body `14px`/`1.62` |
| Headings in prose | `.md-h` S:1052 | the same size as the prose, weight `620`, margin `10px 0 4px`. Lines starting with `#` to `####` all render as one `.md-h` style; there is no per-level size (`web/app.js:124, :139`). |
| Bold, italic | `strong`, `em` from `web/app.js:91-92` | the browser's defaults (`bolder`, italic); the stylesheet sets nothing |
| Inline code | `.md code` S:332-335, S:1055-1060 | data, `0.88em` |
| Code blocks in prose | none | the markdown renderer has no fenced blocks; a ```` ``` ```` line renders as an ordinary `.md-line` (`web/app.js:104-141`) |
| Reasoning | `.reasoning-text` S:1205-1213 | `12.5px`/`1.6`, `--reason-text`; its `.md-h` is `--note-text` |
| Narration between passes | `.log-note` S:1197-1202 | `13.5px`/`1.62`, `--note-text` |
| Tool row name | `.log-step .s` S:1143-1151 | body `12px`, `550`, `color-mix(in srgb, var(--bone) 82%, transparent)` |
| Tool row detail | `.log-step .d` S:1153-1163 | body `12px`, `color-mix(in srgb, var(--dim-text) 82%, transparent)`; `.mono` is the same, body face (S:1163) |
| Tool meta, diffstat | S:1164-1170 | data `10px`, `--dim-text`; `+n` `--diff-plus`, `-n` `--redstone` |
| Tool error line | `.log-err` S:380-383, S:1191-1196 | data `11px`/`1.5`, `--err-soft` |
| Program source, bash output | `.code` S:1309-1320, `.code.term` S:404-408 | data `11px`/`1.65`, `--dim-text` (`--term-text` for bash), `tab-size: 2`, `white-space: pre` |
| Code still arriving | `.code-awaiting` S:1321-1325 | body, italic, `color-mix(in srgb, var(--dim-text) 72%, transparent)` |
| Ticker | `.ticker` S:1326-1337 | body `11px`/`20px`, tabular numbers, `color-mix(in srgb, var(--dim-text) 70%, transparent)` |
| Fold summary | `.activity .steps` S:1085-1091 | body `12px`, `450` |
| Error message | `.msg.error` S:366-371 | `13px`, `--err-text` |
| Empty state heading | `.empty-hint .big` S:1374-1380 | body `20px`/`1.25`, `620`, `-0.4px`, `--bone`; `19px` under 560px |
| Empty state sub | `.empty-sub` S:3931 | `13px`, `--dim-text` |
| Composer text | `.composer-input` S:1398-1401, S:1737-1742 | `14px`/`20px`, `--bone` |
| Buttons | `.btn` S:762-773 | body `13px`, `500`, no text transform |
| Picker triggers | `.model-picker-trigger` S:928-937 | ui `12.5px`, `450`, `--ash`; effort and resolution triggers are body `12px`, capitalized (S:1618-1619) |
| Model rows | `.model-name` S:275-281, S:1429, S:2375 | `12.5px`, `470`, `0.1px`, `--bone`; `500` when selected |
| Settings nav title | `.settings-nav-title` S:2468 | `15px`, `600` |
| Settings nav group label | `.settings-nav-label` S:2469 | `11.5px`, `500`, `--dim-text` |
| Settings nav item | `.settings-nav-item .name` S:2495 | `13.5px`/`1.3`, `500`; `.ver` data `11px` `400` `--dim-text`; `.sub` `12px`/`1.3` `--ash`, `.sub.muted` `--dim-text` |
| Settings pane title | `.pane-head h3` S:2538 | `17px`, `600`, `-0.01em`; `.pane-version` data `12px` |
| Settings section head | `.s-section-head h4` S:2817 | `13px`, `400`, `color-mix(in srgb, var(--bone) 70%, transparent)` |
| Settings row title | `.s-row-title` S:2814 | `13.5px`, `500`, `--bone` |
| Settings row help | `.s-row-help` S:2815, S:2570 | `12px`/`1.45`, `color-mix(in srgb, var(--dim-text) 85%, transparent)`, max width `62ch` |
| Settings value | `.s-value` S:2585 | `13px`, `--ash`, right-aligned; `.mono` data `12.5px` |
| Sign-in code | `.s-signin-code code` S:2642 | `15px`, `0.08em` |
| Chips and tags | `.s-tag` S:2685-2696 | `11px`, `500`, `0.01em` |
| Prompt editor title | `.pe-title` S:2172-2181 | body `620 24px/1.2`, `-0.6px` |
| Prompt editor body | `.pe-body` S:2199-2209 | body `14px/1.65` |
| Command panel input | `.cmdk-input input` S:4035-4043 | body `15px` |
| Setup heading | `.setup .big` S:3526 | body `620 20px/1.25`, `-0.4px`; `17px` under 980px |
| Crash screen | `.crash` S:4118-4120 | ui; `h1` `18px` `600`; `pre` `12.5px` `--err-text` |

### 1.4 Window, layout grid and the split

| Selector | Property | Value | Lines |
|---|---|---|---|
| `*` | `box-sizing` | `border-box` | S:6 |
| `html, body, #app` | height, margin | `100%`, `0` | S:31 |
| `body` | colour, overflow, background, font | `--bone`, `hidden` (the page never scrolls; panes scroll), `--bedrock`, `--font-body` | S:33-36, S:743-748 |
| `.layout` | grid | rows `100%`; columns `var(--sidebar-w) var(--chat-col) 5px minmax(0, 1fr)`, that is sidebar, chat, divider, world; background `--bedrock` | S:64-68, S:796-799 |
| `.layout.sidebar-closed` | columns | `var(--chat-col) 5px minmax(0, 1fr)`; `.sidebar` `display: none` | S:70, S:800 |
| `.layout > *` | | `min-height: 0; overflow: hidden`, so tall content cannot push the composer off screen; `[hidden]` children are `display: none !important` | S:71-74 |
| `.layout.prompts-mode` (a prompt open in Bench) | columns | `var(--sidebar-w) minmax(0, 1fr)`; closed `minmax(0, 1fr)`; there is no viewer and no divider; `.chat` has no right border | S:1962-1966 |
| `.strata` (the divider) | box | 5px column, `position: relative`, background `--bedrock`, `cursor: col-resize`, transition `background 120ms ease` | S:79-81, S:803-807 |
| `.strata::after` | the line | absolute, `top: 0; bottom: 0; left: 2px; width: 1px`, `--line`, transition `background 120ms ease, box-shadow 120ms ease` | S:808-817 |
| `.strata:hover` | | background `--hover`; the line turns `--gold` with `box-shadow: 0 0 0 1px color-mix(in srgb, var(--gold) 25%, transparent)` | S:818-819 |
| `body.dragging` | while dragging the divider or the sidebar edge | `cursor: col-resize; user-select: none`; `.viewer iframe` gets `pointer-events: none` so the frame cannot swallow the moves | S:82-83 |
| `.sidebar-edge` | the sidebar's resize handle | absolute on the sidebar's right edge (`inset: 0 0 0 auto`), `7px` wide, `z-index: 5`, `cursor: col-resize` | S:883-888 |
| `.sidebar-reveal` | shown when the sidebar is closed and the window is wide (`web/app.js:6528`, title `Show sidebar`) | fixed on the window's left edge (`inset: 0 auto 0 0`), `7px`, `z-index: 5`, `cursor: e-resize`. A click opens the sidebar; a drag opens it at the dragged width. | S:883-889 |
| `::after` of both | the hover line | `2px` wide, transparent, at the right edge (edge) or the left (reveal), transition `background 140ms ease`; on hover, and on `body.sizing-sidebar` during a drag, `color-mix(in srgb, var(--bone) 35%, transparent)` | S:890-902 |

### 1.5 Sidebar

| Selector | Box | Text | States | Lines |
|---|---|---|---|---|
| `.sidebar` | flex column, `gap: 8px`, padding `10px 9px 9px`, background `--bedrock`, `border-right: 1px solid var(--line)`, `position: relative` | | | S:87-91, S:822-828 |
| `.brand` | flex, centred, `min-height: 40px`, `gap: 9px`, padding `2px 4px 4px`; its svg is `image-rendering: pixelated` | the wordmark | | S:93-97, S:829-833 |
| `.brand-actions` | `margin-left: auto`, flex, `gap: 0` | | | S:101-104, S:850 |
| `.icon-btn` (new chat, sidebar toggle) | `32x32`, radius `8px`, no background or border, a grid that centres the svg | `--ash` | hover: `--bone` on `--raised`; transition `background 140ms var(--ease), color 140ms var(--ease)` | S:53-62, S:788-794 |
| `.mode-word` (Build or Bench; switches the mode) | inline-flex, `gap: 5px` | inherits | hover `--bone`; its svg is at opacity `0.75`, stroke `2.1`, and on hover mirrors (`scaleX(-1)`, `200ms var(--ease)`); `.swapped` runs `mode-swap 700ms ease-out` (pure `--bone` for the first 25%, then back to 58%) | S:2993-3017 |
| `.switch-dot` | absolute `top: -3px; right: -5px`, `6x6` | | a `.conv-dot` showing the other mode's state: blue if something there finished, grey pulse if working | S:3008-3010 |
| `.side-find` (search) | `32px` high, margin `0 2px`, padding `0 3px 0 10px`, radius `8px`, `--raised`, `box-shadow: inset 0 0 0 1px var(--line)` | `--dim-text` icon | while its input has focus, ring `color-mix(in srgb, var(--bone) 22%, transparent)`, `140ms ease` | S:3607-3643 |
| `.find-filter-button` | `26x26`, radius `6px` (S:3122-3134) | `--dim-text` | hover or expanded: `--bone` on `--hover`; `.active` (a filter is on): `--bone` on `--hover` | S:3135-3137, S:3645-3647 |
| `.tag-menu.find-menu` | spans the search box (`left: 0; right: 0`), `max-height: min(440px, 70vh)`, padding `3px 5px 5px`; groups divided by `1px solid var(--line)` with `3px` margin | items are flex `gap: 8px`; a logo `--ash`; `em` ui `11.5px` `--dim-text`; the check svg shows only on `.selected` | focus-visible: outline offset `-2px` (drawn inside, because the menu scrolls), `--raised` and `--bone` | S:3648-3658 |
| `.find-chips`, `.find-chip` | chips row `gap: 4px`, margin `-2px 2px 0`; chip `22px` high, padding `0 2px 0 7px`, radius `6px`, `--raised`, inset 1px `--line` | ui `500 12px` `--bone`; text trimmed to cap height | its remove button is `16x16` radius `4px` `--dim-text`, hover `--bone` on `--hover`; `.find-clear` is `22px` high, padding `0 7px`, ui `12px` | S:3660-3688, S:3248-3267 |
| `.conv-list` | scrolls on y, `flex: 1`, column, `gap: 1px`, padding `3px 0` | | | S:110, S:851 |
| `.conv-item` (a chat row) | flex, centred, `min-height: 34px`, `gap: 8px`, radius `7px`, padding `7px 9px`, no border; `user-select: none` (a click would leave a caret in the title, and Chrome draws no ellipsis on a line with a caret) | `13px` `430` `--ash`; the title ellipsizes on one line | hover: `--hover` and `--bone`; `.active`: `--raised` and `--bone`, no shadow; transition `background, color, border-color 140ms var(--ease)` | S:112-125, S:852-868 |
| `.conv-text` | column, `gap: 2px`; the title ellipsizes; then `.conv-snippet` | | | S:3689-3698 |
| `.conv-item .archive` | `24x24`, margin `-3px -5px -3px 0`, radius `6px`, `--dim-text` | | opacity `0`, shown on row hover or its own focus-visible (`140ms`); hover `--bone` on `color-mix(in srgb, var(--bone) 7%, transparent)`; `:disabled` (a running chat or bench) is `visibility: hidden` but keeps its space, so the resolution tag does not move | S:126-136, S:869-879 |
| `.conv-dot` (status dot) | `7x7` circle, `flex: none`, base fill `--bedrock` (the dot keeps its place even when idle) | | `.unread`: `--waiting` with `box-shadow: 0 0 6px color-mix(in srgb, var(--waiting) 45%, transparent)`; `.unread.arrived`: `conv-arrive 700ms ease-out` from `color-mix(in srgb, var(--bone) 50%, var(--bedrock))` with no glow; `.running`: `conv-pulse 0.9s ease-in-out infinite alternate`, between `color-mix(in srgb, var(--bone) 78%, var(--bedrock))` and `color-mix(in srgb, var(--bone) 22%, var(--bedrock))`; under reduced motion the pulse lasts `3s`; `.idle` and `.halted` have no fill and no shadow | S:2939-2968, S:3046 |
| Pulse clock | `web/app.js:3396-3404` | | every `conv-pulse` animation has its `startTime` set to `0` on `animationstart`, so every running dot on the page fades in step | |
| `.arrived` logic | `web/app.js:6167-6172` | | `arrived` holds while a dot that was `running` turns `unread`, and stays while it remains unread | |
| `.side-section` (Prompts or Benches heading row) | flex, `gap: 2px`, `min-height: 34px`, padding `6px 4px 0` | | | S:3047-3053 |
| `.side-title` | inline-flex, `flex: 1 0 auto`, `gap: 6px`, `28px` high, radius `7px`, padding `0 6px` | ui `500 13px` `--dim-text` | hover `--bone`; the chevron rotates `90deg` when `aria-expanded="true"`, `160ms var(--ease)`; transition `color 120ms ease, flex-grow 220ms var(--ease)` | S:3054-3072, S:3091 |
| `.side-tags i` | `6x6` dots, `gap: 3px` | | | S:3073-3074 |
| `.side-add` | `28x28`, radius `7px` | | | S:3075 |
| `.side-search` (search in a section) | starts as a 28px button (`flex: 0 0 28px`), `28px` high, radius `7px` | | `.searching`: the title's `flex-grow` goes to `0` and the search's to `1`, with `--raised` and an inset 1px `--line` ring; `220ms var(--ease)` on flex-grow and `160ms` on background and shadow; the input fades in from opacity 0 (`160ms`); `.side-filter` shows only while searching; the icon goes `--dim-text` with `cursor: default` | S:3076-3121 |
| `.side-search-icon` | `28x28`, radius `7px`, `--ash` | | hover: `--bone` on `--raised` | S:3092-3106 |
| `.tag-menu` | absolute, `z-index: 90`, `top: calc(100% + 6px); right: 0`, `190px` wide, `max-height: 260px`, scrolls; `1px solid var(--line)`, radius `10px`, `--well`, `box-shadow: 0 16px 42px rgba(0, 0, 0, 0.42)`, padding `5px`, origin top right | items: grid `8px minmax(0, 1fr) auto`, `gap: 9px`, `min-height: 30px`, radius `7px`, padding `5px 8px`, ui `12.5px`, `--ash`; `i` is an `8x8` colour dot | item hover `--raised` and `--bone`; `.selected` `--bone`; opens with `effort-menu-in 150ms`, closes (`.closing`, no pointer events) with `effort-menu-out 110ms ease-in` | S:3138-3174 |
| `.bench-list` | `flex: 0 1 auto; max-height: 58%` | | | S:3020 |
| `.bench-row` | `gap: 6px; padding-left: 4px` | | | S:3021 |
| `.bench-caret` | `20x20`, radius `5px`, `--dim-text` | | hover `--bone` on `color-mix(in srgb, var(--bone) 7%, transparent)`; `.open` turns its svg `90deg`, `160ms var(--ease)` | S:3022-3038 |
| `.bench-members` | column, `gap: 1px`, margin `1px 0 3px 17px` | members: `min-height: 30px`, padding `5px 9px`, `12.5px` | | S:3039-3045 |
| `.prompt-side`, `.pe-list` (the Bench mode's prompt list) | list: grid, `gap: 2px`, padding `7px`, scrolls | | | S:2079-2092 |
| `.pe-item` | flex, `gap: 7px`, `min-height: 34px`, radius `7px`, padding `8px 10px`, body `13px` `--ash`, `user-select: none` | the title ellipsizes | hover `--hover` and `--bone`; `.active` `--raised` and `--bone` | S:2093-2109, S:2139-2145, S:2972 |
| `.pe-item-actions button` | `23x23`, radius `5px`, `--dim-text`, opacity `0` (`100ms ease`) | | shown on row hover, on the active row, on focus-visible, on a favourite star, or on an armed delete; hover and focus `--hover` and `--bone`; the star `--star` on hover or when favourite; delete text `--err-text` on hover (`!important`); `.pe-item-delete.armed` widens to auto with padding `0 7px`, ui `500 11.5px`, `--err-text` on `color-mix(in srgb, var(--redstone) 14%, transparent)` | S:2110-2138 |
| `.pe-item-dots i` | `6x6` circles, `gap: 3px` | | | S:2146, S:2154-2158 |
| `.prompt-added-by` | inline-flex, `gap: 5px`, ui `11.5px` `--dim-text`; `.compact` is `--ash` | | | S:2149-2151 |
| `.pe-empty` | padding `10px`, data `11px` `--dim-text` | | | S:2159-2163 |
| Prompt reordering | `.pe-list.sorting` | | the other rows slide, `transform 150ms cubic-bezier(0.2, 0.7, 0.2, 1)`, with no pointer events; inactive rows lose their background; `.lifted` follows the pointer with no transition, `z-index: 2`, `--raised`, `--bone`, `box-shadow: var(--control-face), 0 10px 24px rgba(0, 0, 0, 0.32)`; `.lifted.dropping` glides into place with `transform 150ms` and the shadow fading out over `150ms`; action buttons hide except a favourite star; `body.grabbing` forces `cursor: grabbing !important` everywhere | S:2970-2990 |
| `.archived` | column, padding `4px 4px 0`; list `max-height: 40vh`, scrolls, `gap: 2px` | rows: flex, `gap: 2px`, `min-height: 34px`, padding `5px 4px 5px 9px`, radius `7px` | row hover or focus-within: `--hover` and `--bone`; its buttons always show | S:3701-3720 |
| `.row-icon` | `24x24`, radius `6px`, `--dim-text` | | hover `--bone` on `color-mix(in srgb, var(--bone) 7%, transparent)`; `.armed` `--err-text` on `color-mix(in srgb, var(--err-text) 14%, transparent)`; `140ms var(--ease)` | S:3721-3736 |
| `.side-empty` | padding `8px 10px`, `12.5px` `--dim-text` | | | S:3699 |
| `.sidebar-foot` (Settings) | flex start, `margin-top: auto`, padding `6px 0 0 2px`; the 2px lines its icon up with the rows' text | | | S:106-108, S:880 |

### 1.6 Chat shell, pane header and transcript

| Selector | Box | Text | States | Lines |
|---|---|---|---|---|
| `.chat` | flex column, `min-width: 0`, background `--cave`, `border-right: 1px solid var(--line)` | | | S:145-149, S:911-914 |
| `.chat-header` | flex, centred, `min-height: 52px`, `gap: 6px`, padding `9px 12px`, `border-bottom: 1px solid var(--line)` | the title flexes and ellipsizes | | S:151-159, S:915-927 |
| `button.crumb` | no border or background | `--ash` | hover `--bone`, `120ms ease` | S:3175-3184 |
| `.header-action` | inline-flex, `gap: 7px`, `30px` high, padding `0 12px`, radius `8px`, `--bone` fill, `--primary-face` | ui `500 12.5px`, `--bedrock` | hover opacity `0.88`; active `scale(0.97)` and `--primary-pressed`; disabled `--raised` and `--dim-text`, opacity `0.64`, default cursor; focus-visible `2px solid var(--gold)`, offset `2px`; transition `transform 80ms ease, box-shadow 80ms ease, background 120ms ease` | S:2735-2751, S:2774-2780 |
| `.header-action.quiet` | transparent, `box-shadow: inset 0 0 0 1px var(--line)` | `--ash` | hover opacity `1`, `--bone` on `--hover`; disabled stays transparent; `.armed` `--bone` with ring `--bevel-light`; `.share-button.active` `--bone` on `--hover` | S:3210-3212, S:3787, S:4134 |
| `.round-switch` | flex, `gap: 2px`, padding `2px`, radius `8px`, `--well`, inset 1px `--line` | buttons: `min-width: 26px`, `24px` high, radius `6px`, ui `500 12px`, tabular numbers, `--dim-text` | hover `--bone`; `.active` `--raised`, `--bone`, `--control-face`; `110ms ease` | S:3187-3209 |
| `.cap-warn` | radius `6px`, `color-mix(in srgb, var(--redstone) 12%, transparent)`, padding `3px 7px` | body `11px` `500`, `--err-text` | | S:306-308, S:945-956 |
| `.app-notices-anchor`, `.app-notices` | the anchor takes no height. The notices are absolute, `top: 10px`, centred (`left: 50%; translateX(-50%)`), `width: min(520px, calc(100% - 32px))`, a grid with `gap: 8px`, `z-index: 400`, `pointer-events: none` | each is `.msg.error.app-notice` (`web/app.js:6762-6766`) with `pointer-events: auto`, border `color-mix(in srgb, var(--redstone) 28%, transparent)`, `box-shadow: 0 12px 32px rgba(0, 0, 0, 0.36)`, `overflow-wrap: anywhere` | | S:4093-4113 |
| `.messages` | scrolls on y, `flex: 1`, column, `gap: 18px`, padding `22px 20px 16px`, `scroll-padding-bottom: 24px` | | new children rise in: `rise 200ms var(--ease) both`, from opacity 0 and 5px down; nodes already in the DOM do not animate again; only under `prefers-reduced-motion: no-preference` | S:310-326, S:959-967 |
| Tight register | `.messages > .reasoning-row + .reasoning-row`, and every pair of `.reasoning-row`, `.live-tool-stack` and `.pending` | | `margin-top: -14px`, so adjacent live work rows sit `4px` apart instead of `18px` | S:1264-1271 |
| `.msg` | `max-width: 94%`, `white-space: pre-wrap`, `word-wrap: break-word` | prose | | S:317, S:968-972 |
| `.user-block` | `align-self: flex-end`, column, items to the end, `max-width: 92%`, `gap: 3px` | | | S:338-343, S:980 |
| `.msg.user` | `max-width: 100%`, `1px solid var(--line)`, radius `13px`, `--raised`, padding `9px 13px` | | `.user-block.editing .msg.user` (the prompt the composer is editing): border `color-mix(in srgb, var(--bone) 28%, transparent)` | S:344-346, S:973-979, S:994 |
| `.msg-tools`, `.mtool` | tools row end-aligned, `gap: 2px`; `27x27`, radius `6px`, `line-height: 0`, `--dim-text` | | opacity `0`, `1` while the block is hovered (`140ms`); hover `--bone` on `--well`; disabled (waiting for a branch switch, or for an open edit) `--dim-text`, no background, default cursor | S:347-360, S:981-992 |
| `.branch-picker` (`‹ 2/3 ›`) | inline-flex, `gap: 1px`, `margin-right: 3px` | data `10px`/`1`, `--dim-text` | opacity `0`, shown on block hover or focus-within (`140ms`); its own buttons are always opacity 1; a disabled one is at `0.28` | S:995-1015 |
| `.msg.assistant` | `align-self: flex-start`, padding `0` | `--bone` | | S:362-364, S:1016 |
| `.msg.error` | `align-self: stretch`; flex, `justify-content: space-between`, `align-items: start`, `gap: 10px`; border `color-mix(in srgb, var(--redstone) 28%, transparent)`, radius `9px`, background `color-mix(in srgb, var(--redstone) 8%, var(--well))`, padding `10px 12px` | `13px`, `--err-text` | | S:366-371, S:1017-1035 |
| `.error-dismiss` (on every error surface) | `22x22`, margin `-2px -4px 0 0`, radius `6px`, transparent, inherits the colour, opacity `0.65` | | hover: opacity `1` on `color-mix(in srgb, var(--redstone) 16%, transparent)` | S:1036-1050 |
| `.md` | `white-space: normal`; `.md-line` and `li` are `pre-wrap`; `.md-line` `min-height: 1.45em`; lists margin `7px 0`, `padding-left: 20px`, `li` margin `3px 0` | | | S:328-331, S:1051-1054 |
| `.md code` | `1px solid var(--line)`, radius `5px`, `--well`, padding `1px 5px` | data `0.88em` | inside `.reasoning-text` its background is `--raised` (S:336 is more specific) | S:332-336, S:1055-1060 |
| `.empty-hint` | `margin: auto`, centred column, `max-width: 340px`, `gap: 13px`, padding `30px` (`24px 18px` under 560px) | `.big` heading | | S:468-477, S:1369-1380 |
| `.missing-actions` | flex, centred, `gap: 8px`, `margin-top: 6px` | | | S:3933 |

### 1.7 Tool rows, folds, plans and notices

Every work row shares one grid of `20px` icon, `minmax(0, 1fr)` text and `20px` action, with an `8px` gap and a `24px` minimum height (S:960-963). Carets and icons sit in the icon column. Status is shown by text colour, with no badges.

| Selector | Box | Text | States | Lines |
|---|---|---|---|---|
| `.activity` (a turn's work in one fold) | `align-self: stretch`, `border-bottom: 1px solid color-mix(in srgb, var(--line) 60%, transparent)`, padding `4px 0 8px` | | | S:1062-1067 |
| `.activity > summary` | inline-flex, `width: fit-content`, `min-height: 20px`, `gap: 4px`, radius `6px`, padding `0 4px`; no list marker | `--dim-text`; `.steps` body `12px` `450` | hover `--bone` (`140ms`); focus-visible draws no outline but `box-shadow: inset 0 0 0 2px color-mix(in srgb, var(--dim-text) 28%, transparent)`, as do the reasoning, history and code summaries | S:446-453, S:1068-1091 |
| `.activity-chevron` | opacity `0.7`, `transform 200ms ease` | | open: `rotate(90deg)` | S:1092-1097 |
| `.activity-body` | column, items at the start, `gap: 1px`, `margin-top: 5px`, padding `0 2px`, no left border | | opening animates `fold-in 180ms var(--ease)` (opacity 0 and `-3px` up to normal); so does an opened `.log-code`'s `.code`; no-preference only | S:454-466, S:1098-1106 |
| `.log-step` | grid on the shared columns, full width, radius `6px`, `font-size: 12px`, `line-height: 20px`, `transition: background 150ms ease` | `.s` name, `.d` detail | `.log-code > .log-step:hover` `color-mix(in srgb, var(--bone) 3.5%, transparent)` | S:376-379, S:1111-1124 |
| `.tool-kind-icon` | `20x24` cell, centred | `--dim-text` at opacity `0.8` | | S:1125-1135 |
| `.tool-copy` | block, `line-height: 20px`, siblings spaced `6px` apart | | | S:1136-1142 |
| `.tool-actions` | `20px` wide, `24px` minimum, centred, `gap: 1px` | `--dim-text` at `0.72` | | S:1171-1180 |
| `.tool-failure` | `16x16` | `--err-soft` | | S:1181-1190 |
| `.log-err` | margin `1px 28px 4px 28px` (that is the action column plus the gap on the right, and the icon column plus the gap on the left) | data `11px` `--err-soft` | | S:1191-1196 |
| `.log-code > summary` | pointer, no marker, `user-select: none` | | hover or open: `.s` becomes `--bone` | S:393-396 |
| `.code` (program source) | margin `4px 0 3px 28px`, `max-width: calc(100% - 28px)`, `max-height: 256px`, scrolls both ways, no border or background, padding `2px 0` | data `11px`/`1.65` `--dim-text` | while it streams, it keeps scrolled to the bottom when it was within 40px of it (`web/app.js:6031-6038`) | S:397-402, S:1309-1320 |
| `.code.term` (bash output) | `margin-top: 4px`, `max-height: 220px` | `--term-text` | | S:404-408 |
| `.tk-k`, `.tk-s`, `.tk-n`, `.tk-c`, `.tk-f` | keywords, strings, numbers, comments, functions | the `--tk-*` colours | | S:410-414 |
| `.log-note` | `max-width: 96%`, margin `7px 0` | `13.5px`/`1.62` `--note-text` | | S:417-419, S:1197-1202 |
| `.reasoning-text` | `max-width: 96%`, margin `3px 0`, `border-left: 2px solid var(--line)` (the rail), `padding-left: 12px`; one following another is `8px` lower | `12.5px`/`1.6` `--reason-text`, pre-wrap | | S:505-509, S:1203-1214 |
| `.reasoning-row > summary` | the shared grid, radius `6px`, no marker, `user-select: none` | `12px` `500` `--dim-text` | hover `--bone` on `color-mix(in srgb, var(--bone) 3.5%, transparent)`, `150ms ease`; the icon slot (opacity `0.7`) turns its svg `90deg` when open, `200ms ease` | S:1215-1251 |
| `.reasoning-body` | margin `4px 0 2px 28px`, padding `1px 0` | | | S:1252-1256 |
| `.live-tool-stack`, `.live-tool-call` | columns, `gap: 1px` | | | S:1257-1277 |
| `.tool-history > summary` (earlier calls folded) | the shared grid, radius `6px` | `12px` `550`, `color-mix(in srgb, var(--bone) 82%, transparent)` | hover background `3.5%` bone; when open, the icon turns `180deg`, `200ms ease` | S:1278-1299 |
| `.tool-history-body` | column, `gap: 1px`, margin `2px 0 3px`, padding `1px 0` | | | S:1300-1307 |
| `.log-shots` | flex wrap, `gap: 6px`; inside a live call, margin `2px 0 5px 28px` | | | S:422-426, S:1308 |
| `.log-shots img` | `88px` high, auto width, `1px solid var(--line)`, radius `8px`, `cursor: zoom-in` | | hover: border `--gold`, `translateY(-1px)`, `140ms var(--ease)` | S:427-436 |
| `.ticker` | grid `20px minmax(0, 1fr)`, `gap: 8px`, `24px` minimum, no text transform | body `11px`/`20px`, tabular numbers, `color-mix(in srgb, var(--dim-text) 70%, transparent)` | | S:438-442, S:1326-1337 |
| `.pending` | flex, `24px` minimum, `align-self: flex-start` | | | S:443, S:1338-1343 |
| `.working-dots` (the working indicator) | three `4x4` circles in `currentColor`, `gap: 3px` | | `working-dot 2s steps(6) infinite`, delays `0`, `200ms` and `400ms`: opacity `1` over 0-40% and at 100%, `0.5` over 50-90%; the base opacity is `0.3`; under reduced motion there is no animation and opacity is `0.45` | S:1350-1367 |
| `.diffstat` | inline-flex, `gap: 7px` | data `10px`; `.p` `--diff-plus`, `.m` `--redstone` | | S:384-390, S:1164-1169 |
| `.plan-card` | grid, `gap: 8px`, margin `6px 0`, padding `12px 14px`, radius `10px`, `--well`, ring 1px `--line`, `max-width: 560px` | head `12px` `500` `--ash`; count `--dim-text` tabular; items `13px`/`1.45` `--bone`, grid `16px 1fr` with a `9px` gap | `li.done`: text `--dim-text`, struck through in `color-mix(in srgb, var(--dim-text) 60%, transparent)`; `.plan-mark` is a `14x14` circle with a `1.5px` inset `--dim-text` ring; done fills `--ash`; active has a `--bone` ring plus a centre dot `radial-gradient(circle, var(--bone) 0 2.5px, transparent 3px)` | S:2916-2924 |
| `.notice-line` | flex, baseline, `gap: 8px`, margin `4px 0` | `12.5px` `--ash` | the dot is `6x6`, `--dim-text`, `translateY(-1px)`; `.warning` `--star`; `.error` `--redstone` | S:2925-2928 |
| `.context-meter` | `26x26`, radius `7px`, default cursor; an svg circle with stroke `2` | track `color-mix(in srgb, var(--bone) 14%, transparent)`; fill `--ash`, round cap | hover `--hover`; `.high` (over 85%, `web/app.js:189`) fill `--star`; tip `N% of context · used of window tokens` | S:2929-2934 |
| `.limit-chip` | `22px` high, padding `0 8px`, radius `6px`, `color-mix(in srgb, var(--star) 14%, transparent)` | `11.5px` `500` `--star` | `.rejected`: `color-mix(in srgb, var(--redstone) 16%, transparent)` and `--err-text`; tip `Resets at <time>` (`web/app.js:294`) | S:2935-2936 |

### 1.8 Composer

| Selector | Box | Text | States | Lines |
|---|---|---|---|---|
| `.composer` | column, `gap: 8px`, padding `8px 14px 14px` (`8px 12px 12px` under 980px), background `linear-gradient(to bottom, transparent, var(--cave) 12px)` so the transcript fades out under its top edge | | | S:481-484, S:1383-1387, S:1527 |
| `.composer-box` | `position: relative`, column, `gap: 6px`, `min-height: 104px`, padding `8px`, `1px solid var(--line)`, radius `12px`, `--well`, `box-shadow: 0 8px 30px rgba(0, 0, 0, 0.14)` | | `:focus-within`: border `color-mix(in srgb, var(--bone) 24%, transparent)`, shadow `0 8px 30px rgba(0, 0, 0, 0.16), 0 0 0 3px color-mix(in srgb, var(--bone) 5%, transparent)`; transition `border-color 160ms var(--ease), box-shadow 160ms var(--ease)` | S:485-488, S:1388-1397, S:1730-1736, S:1980 |
| `.composer-input` (contenteditable) | `min-height: 46px`, `max-height: 220px`, then it scrolls; padding `5px 7px`; `white-space: pre-wrap`, `overflow-wrap: anywhere`; no outline; text cursor. It grows with content from 46px to 220px; the box grows with it from its 104px minimum. | `14px`/`20px` `--bone` | | S:490-498, S:1398-1401, S:1737-1742 |
| Placeholder | `.composer-input-host.empty .composer-input::before` shows `attr(data-placeholder)`, absolute, no pointer events | `--dim-text` | | S:499-503, S:1402 |
| `.ce-prompt` (a saved prompt token) | inline-flex, margin `0 1px`, padding `0 6px 0 2px`, radius `5px`, `color-mix(in srgb, var(--sky-high) 16%, transparent)`, `user-select: none` | `500`, `--sky-high`, `line-height: 20px` | the slash slot (`.ce-slash`, `14px` wide) holds `.ce-remove`, absolute `inset: 3px 0`, radius `4px`, `--sky-high` fill, `--bedrock` icon, opacity `0`, which shows on token hover (`110ms ease`) | S:1744-1780 |
| `.ce-field` (a fill-in box) | inline-block, margin `0 1px`; the input is `20px` high, `min-width: 3ch`, radius `5px`, `--bedrock`, `box-shadow: inset 0 0 0 1px var(--input-line)`, padding `0 6px` | inherits; placeholder `color-mix(in srgb, var(--dim-text) 70%, transparent)` | focus ring `color-mix(in srgb, var(--bone) 40%, transparent)` (`120ms`); `.missing` flashes `field-missing 700ms ease` (a `--redstone` ring over 0-60%) | S:1781-1802 |
| `.composer-footer` | flex, centred, `gap: 6px`, `min-height: 34px`; `position: relative` when it holds a resolution picker | | | S:1803-1808, S:3809 |
| `.composer-controls` | flex, `gap: 3px`, `margin-left: auto` | | | S:1809-1815 |
| `.attach`, `.send`, `.stop` | `32x32` circles (`999px`), transition `color 130ms ease, background 130ms ease, border-color 130ms ease, transform 80ms ease` | | | S:1816-1832 |
| `.attach` | transparent | `--ash` | hover, and `.active` (the prompt palette is open): `--raised` and `--bone` | S:1833-1838, S:2045 |
| `.send` | `--bone` fill | `--bedrock` | hover `color-mix(in srgb, var(--bone) 88%, var(--ash))`; active `scale(0.94)`; disabled `--raised` and `--dim-text`, default cursor | S:1853-1868 |
| `.stop` | `1px solid color-mix(in srgb, var(--redstone) 25%, var(--line))`, background `color-mix(in srgb, var(--redstone) 10%, var(--well))` | `--err-text` | hover: border `45%`, background `17%`, text `--bone`; active `scale(0.94)` | S:1867-1878 |
| Triggers in the composer | model, effort and resolution triggers are ghosts: no shadow, transparent, `min-height: 32px`; effort and resolution padding `4px 7px` and `min-width: 0` (S:1847-1851: the base `76px` does not hold here); the model trigger `gap: 5px`, padding `4px 6px` | | hover `--hover` | S:1839-1852, S:2378-2381, S:2828-2834 |
| `.composer-model` | `flex: 0 1 auto`, `min-width: 0`, no max width; the trigger is `width: 100%` and its label is not capped, so the whole model id shows unless the composer is too narrow | | its popover opens upward: `bottom: calc(100% + 8px); right: 0`, origin bottom right | S:2382-2388, S:2898-2904 |
| `.composer-toggle` (for example blind bench) | inline-flex, `gap: 6px`, `28px` high, padding `0 10px`, pill (`999px`) | ui `12.5px` `--dim-text` | hover `--bone` on `--hover`; `.on` `--bone` with ring `--bevel-light` | S:3788-3803 |
| `.composer-attachments` | flex wrap, `gap: 6px`, padding `1px` | | | S:1879-1884 |
| `.composer-attachment` | grid `30px minmax(0, 1fr) 22px`, `gap: 7px`, `width: min(220px, 100%)`, `min-height: 42px`, `1px solid var(--line)`, radius `8px`, `--raised`, padding `5px` | name `11px` `520` `--bone`, ellipsized; `small` data `9px` uppercase `--dim-text` | `.reading` (a file still being read): dashed border, transparent; remove button `22x22` radius `5px` `--dim-text`, hover `--bone` on `--well` | S:1885-1935 |
| `img`, `.file-tile` | `30x30`, radius `6px`, `--well`, `--ash`, `object-fit: cover` | | | S:1897-1907 |
| `.msg-attachments`, `.msg-file` (in a sent message) | wrap, `gap: 6px`, `margin-top: 8px`; a file is inline-flex, `gap: 7px`, `max-width: 220px`, `min-height: 38px`, `1px solid var(--line)`, radius `8px`, `--well`, padding `4px 8px 4px 4px`; image `30x30` radius `5px` | body `11px` `--ash` | hover border `--bevel-light`, text `--bone` | S:1936-1959 |
| `.composer-selections` | flex wrap, `gap: 5px`, padding `1px 1px 4px`; in a message `margin-top: 8px` | | | S:3402-3406 |
| `.selection-chip` (blocks picked in the viewer) | inline-flex, `gap: 7px`, `28px` high, padding `0 9px 0 4px`, radius `8px`, `--well`, inset 1px `--line` | ui `12px` `--ash`; label `--bone` ellipsized; size `--dim-text` tabular; both trimmed to cap height | hover `--bone` with ring `--bevel-light`, `120ms`; icon `20x20` radius `4px` pixelated, `.empty` a `1.5px` `--dim-text` ring; remove `20x20` radius `5px`, `margin-right: -5px`, hover `--bone` on `--hover` | S:3407-3446, S:3248-3267 |
| `.composer-held`, `.composer-editing` (a line over the composer) | flex, `gap: 8px`, padding `0 4px` | `12.5px` `--ash`; the `.text-link` sits at the right | | S:3560-3565 |
| `.bench-models`, `.bench-chip` (a new bench's models) | row wraps, `gap: 5px`; chip `30px` high, `padding-right: 3px`, radius `8px`, `--raised`, inset 1px `--line` | model button ui `500 12.5px` `--bone`, padding `0 4px 0 9px`, `gap: 7px`; name `max-width: 160px`, ellipsized | the `13x13` icon slot stacks the logo and an x; on hover of the model button the logo fades out and the x (stroke `2.4`, `--ash`) fades in (`110ms`), and the name dims to `--ash`; the effort button is `24px` high, padding `0 6px`, ui `12px` capitalized `--dim-text`, hover or open `--bone` on `--hover` | S:3214-3295 |
| Optical alignment | `text-box: trim-both cap alphabetic` on chip texts; small marks (`x`, chevrons) offset by `translate: 0 calc((1cap - 1ex) / 2)` so they sit on the x-height band | | | S:3244-3267 |
| `.bench-effort-menu` | upward: `bottom: calc(100% + 8px); left: 0`, `168px`, origin bottom left, `effort-menu-up-in`; in `.bench-lineup` it opens down (`top: calc(100% + 6px)`) with `effort-menu-in` | rows flex `gap: 2px`; `.bench-effort-add` `28x28` `--dim-text`, hover `--bone` | | S:3296-3317, S:3596-3602 |

### 1.9 Pickers and popovers

| Selector | Box and position | Content | States and motion | Lines |
|---|---|---|---|---|
| `.model-picker-trigger` | flex, end-aligned, `gap: 7px`, `min-height: 32px`, radius `8px`, padding `5px 8px`, `max-width: 250px`, `--control-face`, no border | ui `12.5px` `450` `--ash`; the label ellipsizes; `.empty` `--dim-text`; the chevron (`.picker-chevron`) has opacity `0.5`, `margin-right: -3px`, stroke `2.25` | hover or focus: `--well` and `--bone`; no focus outline (S:187-189), so focus shows only as the hover look; active `scale(0.97)` with `--control-pressed`; transition `transform 80ms ease, box-shadow 80ms ease, background 120ms ease` | S:172-191, S:928-944, S:2772-2776 |
| `.model-picker-popover` | absolute, `z-index: 100`, `top: calc(100% + 7px); right: 0`, `width: min(360px, calc(100vw - 24px))`, `height: min(max(400px, var(--rail, 0px)), calc(100vh - 96px))`, grid columns `auto minmax(0, 1fr)`, rows `minmax(0, 1fr)`, `translate: var(--nudge, 0px) 0`, origin top right, radius `10px`, `1px solid var(--line)`, `--well`, `--shadow-panel`, clips | rail on the left, then search, list and footer | opens `surface-in 170ms cubic-bezier(0.2, 0.7, 0.3, 1) both` (opacity 0 and scale 0.985 to 1); `.closing` `surface-out 120ms ease-in` (to opacity 0 and scale 0.99), no pointer events | S:192-197, S:1405-1409, S:1688-1694, S:2316-2339 |
| Where it opens | chat header: down and right-aligned; composer and prompt editor (`.pe-ai-model`): up (`bottom: calc(100% + 8px)`), origin bottom right; settings (`.title-model-controls`): right-aligned; new-bench lineup: centred under the row (`left: 50%`, `translate: calc(-50% + var(--nudge, 0px)) 0`), `height: clamp(180px, var(--below), max(400px, var(--rail, 0px)))`, origin top centre | | | S:2279-2285, S:2382-2388, S:2397-2400, S:3496-3506 |
| `.model-provider-rail` | grid, column flow, columns `repeat(var(--lines), 31px)`, rows `repeat(var(--per-line), auto) 1fr`, `gap: 3px 6px`, padding `6px 6px 0`, `--bedrock`, `border-right: 1px solid var(--line)` | buttons: `min-height: 34px`, radius `7px`, centred, `--dim-text`; the `.rail-divider` is `1px`, `--line`, margin `3px 6px`, vertically centred; a harness button carries `.harness-badge`, absolute at `right: 3px; bottom: 2px`, `700 8px/1` data, `-0.5px`, opacity `0.9`; `.rail-setup` (Settings) spans every column in the last row, `min-height: 42px`, margin `auto -6px 0`, `border-top: 1px solid var(--line)`, `--bedrock`, so it lines up with the footer's hairline | hover (not disabled) `--raised` and `--bone`; `.active` `--raised` and `--bone`; `.disabled` opacity `0.28`, `cursor: not-allowed`; `120ms var(--ease)` | S:198-228, S:1410-1423, S:2340-2366 |
| `.model-picker-search` | flex, `gap: 8px`, `min-height: 42px`, padding `2px 11px`, `border-bottom: 1px solid var(--line)` | `--dim-text` icon at `14px`; input transparent, no border or shadow, ui `13px`, padding `9px 0`, `--bone`; placeholder `--dim-text` | no focus ring | S:235-254, S:1424-1426, S:2367-2369 |
| `.model-picker-list` | `flex: 1`, scrolls, padding `4px` | | | S:255-258, S:2372 |
| `.model-picker-row` | flex, centred, `min-height: 34px`, radius `7px` | `.model-choice`: flex `1`, `gap: 8px`, padding `7px 6px 7px 8px`, `--ash`, left-aligned; `.model-name` ellipsizes; `.model-provider` (in a search across providers) `11.5px` `--dim-text` at the right | hover `--raised`; `.selected` `color-mix(in srgb, var(--gold) 10%, var(--raised))` with the name in `500` `--bone`; transitions `140ms ease` on background, border, colour and shadow | S:259-274, S:1427-1429, S:1695-1701, S:2373-2375, S:3519 |
| `.model-star` | padding `7px 8px`, `--dim-text`, opacity `0` (`120ms`) | | shown on row hover, when favourite, or on focus-visible; hover `--bone`; `.favorite` `--star` with a filled svg | S:282-296, S:2376 |
| `.model-check` (choosing several models) | `16x16`, radius `4px`, `margin-left: auto`, ring `--input-line`, transparent | | `.on`: `--bone` fill, `--bedrock` check (stroke `2.6`), no ring; `110ms` | S:3448-3462 |
| `.model-picker-empty`, `.model-picker-error` | padding `24px 18px`, centred | `12.5px`; `--ash`, or `--err-text` for the error, with a dismiss button | | S:297-304 |
| `.model-picker-no-ai` | centred, `gap: 12px`, padding `24px 18px` | text `13px` `--ash`; `.model-picker-connect` `min-height: 32px`, padding `0 14px`, ui `500 12.5px` | | S:3514-3517 |
| `.model-picker-footer` | flex, `gap: 9px`, `min-height: 42px`, padding `0 14px 0 12px`, `border-top: 1px solid var(--line)` | ui `12.5px` `--ash`; the last svg is pushed right | hover `--raised` and `--bone`, `140ms` | S:3947-3965 |
| Phone popover, 560px and below | `width: min(340px, calc(100vw - 16px))`, `height: min(320px, calc(100vh - 80px))`, one column; the rail becomes a strip above the list in `--lines` rows of `--per-line` buttons (`34px` wide, `32px` minimum), `gap: 3px`, padding `5px 0 5px 6px`, a bottom border instead of a right one; the divider is vertical (`1px` wide, margin `6px 3px`); `.rail-setup` sits at the right end spanning the rows, with a left border | | | S:2410-2438 |
| `.effort-picker-trigger`, `.resolution-picker-trigger` | inline-flex, `space-between`, `gap: 7px`, `min-width: 76px`, `min-height: 32px`, radius `8px`, transparent, padding `5px 8px`, `--control-face`, no border | body `12px`, capitalized, `--ash` | hover (resolution: not disabled) or `.open`: `--well` and `--bone`; the chevron turns `180deg` when open, `transform 160ms cubic-bezier(0.2, 0.7, 0.3, 1)`; active `scale(0.97)` with `--control-pressed`; `.resolution-picker-trigger.raised` (above Standard) is `--bone`; disabled has a default cursor | S:1606-1635, S:3811-3812 |
| `.effort-picker-menu` | absolute, `z-index: 90`, `top: calc(100% + 6px); right: 0`, `174px`, clips, `1px solid var(--line)`, radius `10px`, `--well`, `box-shadow: 0 16px 42px rgba(0, 0, 0, 0.42)`, padding `5px`, origin top right | buttons: full width, `min-height: 34px`, grid `minmax(0, 1fr) auto auto`, `gap: 8px`, radius `7px`, padding `7px 8px`, `--ash`; label `12.5px` `500` capitalized | opens `effort-menu-in 150ms cubic-bezier(0.2, 0.7, 0.3, 1) both` (from `translateY(-4px) scale(0.98)`, opacity 0); `.closing` `effort-menu-out 110ms ease-in both` (to `translateY(-3px) scale(0.985)`); in the composer and prompt editor it opens upward with `effort-menu-up-in` and `-up-out` (from `+4px`, to `+3px`); item hover `--raised` and `--bone`; `.selected` `--bone` with a `--gold` check | S:1636-1678, S:1710-1717, S:2286-2293, S:2389-2396, S:2401-2408 |
| In Settings | `.title-model-controls .effort-picker` is `112px`; its trigger is full width, `min-height: 38px`, `--well` | | | S:1679-1685 |
| `.resolution-picker-menu` | absolute, `z-index: 90`, `right: 0; bottom: calc(100% + 8px)` of the composer footer (not of the trigger, so it stays inside the composer), `240px`, `max-width: calc(100vw - 32px)`, `1px solid var(--line)`, radius `10px`, `--well`, the same shadow, padding `10px 12px 11px`, origin bottom right | body `12px` `--ash`; head flex, baseline, `12.5px` `500`, value `--bone`; slider full width, `margin-top: 10px`, `accent-color: var(--bone)`; four tick buttons under the stops: the first at `left: -12px`, the second at `33.333%` and the third at `66.667%` (each `translateX(-50%)`), the fourth at `right: -12px`; ticks are ui `11px` `--dim-text`, padding `2px 4px`, radius `5px`, in a box with margin `2px 8px 0` (the track is inset about 8px, half a thumb); help is one grid cell holding every stop's paragraph, the others `visibility: hidden`, so the height never changes; `11.5px`/`1.45` `--dim-text` | `effort-menu-up-in 150ms` and `-up-out 110ms`; the slider takes focus on open, with a thin ring `1px solid color-mix(in srgb, var(--bone) 26%, transparent)`, offset `5px`, radius `6px`; tick hover `--raised` and `--bone`; `.selected` `--bone` | S:3804-3891 |
| `.prompt-palette` (opened with `/` or the prompts button) | absolute above the composer box (`left: 0; right: 0; bottom: calc(100% + 8px)`), `z-index: 40`, grid `minmax(0, 1fr) minmax(0, 1.15fr)`, `320px` high, radius `12px`, `--well`, `--shadow-panel`, clips; `surface-in 140ms`; at 720px and below one column, `300px`, no preview | left: search `42px` high, padding `0 12px`, `gap: 8px`, bottom hairline (`inset 0 -1px 0 var(--line)`), input ui `13px`; list padding `6px`; group labels `11.5px` `500` `--dim-text`, padding `8px 8px 4px`; rows `32px`, padding `0 4px 0 9px`, `gap: 4px`, radius `7px`; title `13px` `--ash`; meta `11.5px` `--dim-text`; empty `12.5px` `--dim-text`, padding `14px 10px`; foot `11.5px` `--dim-text`, padding `8px 12px`, top hairline, buttons ui `500 12px` `--ash`, hover `--bone`; right preview `--cave`, padding `16px 18px`, `gap: 10px`; title `13.5px` `600` `--bone`; tags `11px` in `--tag` on `color-mix(in srgb, var(--tag) 14%, transparent)`, padding `2px 7px`, radius `5px`; text `12.5px`/`1.55` `--ash` pre-wrap, with `mark` in `--star` on a `16%` tint, padding `0 4px`, radius `4px`; images `54x54` radius `7px`; file chips padding `4px 8px`, radius `6px`, `--raised`, `11.5px` | `.active` row `--raised`, title `--bone`; the star and the edit button (`24x24`, radius `6px`) show on the active row; a favourite star always shows; edit hover `--bone` on `--well` | S:1978-2049 |
| `.pe-ai-menu`, `.pe-ai-note` (Improve with AI) | absolute, `right: 0; bottom: calc(100% + 8px)`, `z-index: 60`, radius `10px`, `--popover`, `box-shadow: var(--highlight), var(--shadow-panel)`, `surface-in 140ms`; the menu is `260px` wide, padding `4px`, gap `1px`; the note is `320px`, padding `10px`, gap `8px` | menu items flex `space-between`, padding `7px 9px`, radius `7px`, `--bone`; `b` `13px` `500`; `small` `11.5px` `--dim-text`; the note's textarea has padding `8px 10px`, radius `8px`, `--well`, ring `--input-line`, ui `13px/1.45` | item hover `--hover`; textarea focus ring `color-mix(in srgb, var(--bone) 30%, transparent)` | S:2852-2896 |
| `.tag-menu` | see the sidebar (1.5) | | | S:3138-3174 |
| `.share-panel` | see 1.14 | | | S:4131-4172 |

### 1.10 Settings

| Selector | Box | Text | States | Lines |
|---|---|---|---|---|
| `.overlay` | fixed, `inset: 0`, `z-index: 20`, centred grid, `rgba(0, 0, 0, 0.62)`, `backdrop-filter: blur(10px) saturate(0.8)` | | `fade-in 150ms var(--ease)` (no-preference); at 560px and below its content sits at the bottom (`align-items: end`) | S:540-546, S:1432-1435, S:529, S:1547 |
| `.settings-panel` (the simple form) | scrolls on y, column, `width: min(620px, calc(100vw - 32px))`, `max-height: min(780px, calc(100vh - 32px))`, `1px solid var(--line)`, radius `14px`, `--cave`, `--shadow-panel`, padding `20px`, `gap: 20px` | | `pop-in 180ms var(--ease)` (from opacity 0, `scale(0.97) translateY(6px)`); at 560px and below a bottom sheet: `max-height: 92vh`, `margin-top: auto`, radius `14px 14px 0 0`, padding `18px 16px` | S:547-552, S:1436-1443, S:1541-1546, S:1552 |
| `.settings-panel.sidebar` (the real Settings) | `width: min(1100px, calc(100vw - 32px))`, `height: min(780px, calc(100vh - 48px))`, block, padding `0`, clips; `height: calc(100vh - 32px)` at 980px and below; at 560px and below it is still a bottom sheet with no padding (`.settings-panel.sidebar` outranks the 560px `.settings-panel` padding) | | | S:2450-2456, S:2717 |
| `.settings-body` | full height, grid `300px minmax(0, 1fr)`; at 980px and below rows `auto minmax(0, 1fr)`, one column, the nav above the pane | | | S:2457, S:2718 |
| `.settings-nav` | scrolls with no scrollbar (`scrollbar-width: none`), padding `20px 12px 16px`, column, `gap: 2px`, `--bedrock`; `max-height: 38vh` at 980px and below | title, group labels | | S:2458-2469, S:2719 |
| `.settings-nav-item` | grid `18px minmax(0, 1fr) auto` (`.single` and `.connect`: no third column), `column-gap: 12px`, full width, `min-height: 40px` (`.provider` `54px`), padding `8px 10px`, radius `9px`, transparent, left-aligned | `--bone`; the icon `--ash`, `--bone` when active; `.nav-text` grid `gap: 2px`, name flex baseline `gap: 7px` | hover `--hover-well`; `.active` `--raised`; focus-visible `2px solid var(--gold)`, offset `-2px`; `.off` dims the logo and text to `0.45`; `120ms var(--ease)` | S:2470-2500, S:3569 |
| `.toggle` (switch) | `34x20`, padding `2px`, pill, track `color-mix(in srgb, var(--bone) 14%, transparent)`; thumb `16x16` circle, `--bone`, `box-shadow: 0 1px 2px rgba(0, 0, 0, 0.35)` | | `.on`: track `--gold`; thumb `translateX(14px)`, `--bedrock`, no shadow; while pressed the thumb widens to `19px` (on: `translateX(11px)`, so it stretches left); thumb transition `transform 150ms ease, width 100ms ease 100ms`; track `background 140ms var(--ease)`; focus-visible `2px solid var(--gold)`, offset `2px` | S:2502-2523, S:2819-2823 |
| `.settings-main` | `position: relative`, grid rows `minmax(0, 1fr) auto auto` (pane, error, actions) | | | S:2525 |
| `.settings-close` (`.panel-close`) | absolute `top: 14px; right: 14px`, `z-index: 1`, `32x32`, radius `8px` | data `16px` `--ash` | hover `--bone` on `--raised`, `140ms` | S:553-562, S:1444-1452, S:2526 |
| `.settings-pane` | scrolls on y, padding `22px 32px 28px` (`18px 16px 24px` at 980px and below), grid, `gap: 26px` | | | S:2527-2534, S:2720 |
| `.settings-error` | flex with a dismiss button, `1px solid color-mix(in srgb, var(--redstone) 35%, transparent)`, radius `8px`, `rgba(224, 93, 79, 0.08)`, padding `8px 11px`; in the sidebar layout, margin `0 32px 6px` | `12px` `--err-text` | | S:571-578, S:1025-1035, S:2535 |
| `.pane-head` | flex, `gap: 11px`, `min-height: 32px`, `padding-right: 40px` (room for the close button) | `h3`, version | | S:2537-2539 |
| `.s-recheck` | `margin-left: auto`, `28px` high, padding `0 10px`, radius `7px`, `gap: 6px` | ui `12px` `--dim-text` | hover `--hover-well` and `--ash` | S:2540-2554 |
| `.s-section`, `.s-section-head` | grid `gap: 10px`; head flex `space-between`, padding `0 2px` | `h4` | | S:2556-2558, S:2817 |
| `.s-section-action` | no border or background | ui `500 12px` `--dim-text` | hover `--bone` | S:3764-3772 |
| `.s-card` | radius `12px`, `color-mix(in srgb, var(--well) 70%, transparent)`, `box-shadow: 0 0 0 1px var(--line)`; a card holding an open model or effort picker gets `position: relative; z-index: 5`, and `.s-row-control` is `z-index: 2`, so the picker paints over the cards below | | | S:2559-2561, S:2816 |
| `.s-row` | grid `minmax(0, 1fr) minmax(0, auto)`, centred, `min-height: 56px`, padding `12px 16px`, `gap: 32px`; rows after the first carry `box-shadow: inset 0 1px 0 color-mix(in srgb, var(--bone) 5%, transparent)`; at 980px and below one column, controls left-aligned, values left-aligned | title and help; the control is flex end-aligned. A row whose control is an `.s-value` becomes a wrapping flex: text `flex: 1 1 260px`, the value beside it when it fits, else under it, `row-gap: 6px` | `.flash` (reached from search): `setting-flash 1.4s ease-out`, `color-mix(in srgb, var(--bone) 8%, transparent)` over 0-35% fading to transparent | S:2562-2592, S:2721-2723, S:2813, S:4087-4091 |
| `.settings-panel input` (any) | `min-height: 38px`, `1px solid var(--line)`, radius `8px`, `--bedrock`, padding `8px 10px`; `140ms` transitions | data `12.5px` `--bone` | focus: border `color-mix(in srgb, var(--bone) 24%, transparent)` and `--glow-gold`, no outline | S:563-570, S:1453-1462 |
| `.s-input` | `300px` wide (`max-width: 100%`), `34px` high, padding `0 11px`, no border, radius `8px`, `--raised`, `--field-face`; in a sign-in form `240px` | `13px` `--bone`; `.mono` data `12.5px` | focus: `inset 0 0 0 1px color-mix(in srgb, var(--gold) 70%, transparent), var(--glow-gold)` | S:2572-2584, S:2644, S:2773 |
| `.s-filter` | `260px`, `32px` high, padding `0 11px`, radius `8px`, `--raised`, `--field-face` | ui `13px` | focus: the same inset ring, without the glow | S:2650-2661 |
| `.cmd-chip` (a command to copy) | inline-flex, `gap: 2px`, `32px` high, padding `0 4px 0 11px`, radius `8px`, `--raised`, `--control-face` | `code` data `12px` `--bone`, no wrap; button `26x26` radius `6px` `--dim-text` | button hover `--bone` on `--hover` | S:2593-2614 |
| `.copy-btn` | `26x26`, radius `6px`, `--dim-text` | | hover `--bone` on `--hover` | S:2630-2641 |
| Sign-in | `.s-signin-link` flex `gap: 4px`, `margin-top: 6px`; link data `12px` `--bone`, ellipsized; `.s-signin-form` and `.s-signin-start` flex wrap end, `gap: 8px`; `.s-signin-error` block, `margin-top: 6px`, `--err-text`, pre-wrap | | | S:2617-2645 |
| `.s-empty` | padding `18px` | `12.5px` `--dim-text`; `.error` `--err-text` | | S:2615-2616 |
| `a.btn` | inline-flex, centred, no underline | | | S:2646 |
| Models list | `.s-models-intro` padding `16px 18px 0`, `12.5px`/`1.45` `--dim-text`; toolbar flex, `gap: 14px`, padding `12px 18px 8px`; `.s-link` ui `500 12.5px` `--ash`, hover `--bone`, disabled `0.4`; `.s-count` `margin-left: auto`, `12px` `--dim-text`, tabular; the list has padding `2px 8px 10px` | | | S:2648-2666 |
| `.s-model` | grid `18px 26px auto minmax(0, 1fr) auto auto` (grip, star, name, id, capabilities, toggle), `column-gap: 10px`, `min-height: 40px`, padding `0 10px 0 4px`, radius `8px`; at 980px and below `18px 26px minmax(0, 1fr) auto`, with the id and capabilities hidden | name `13px` `--bone`, no wrap; id data `11.5px` `--dim-text`, ellipsized; `.s-tag` `20px` high, padding `0 7px`, radius `6px`, `--raised` `--ash` | hover `--hover-well`; `.hidden` dims the name, id and capabilities to `0.45`; `.s-star` `26x26` `--dim-text`, hover `--ash`, `.on` `--star` filled | S:2667-2696, S:2724-2725 |
| `.s-grip` (drag handle) | `18x28`, radius `5px`, `cursor: grab`, `touch-action: none`, opacity `0.5` | | opacity `1` on row hover or focus-visible; hover `--bone`; `120ms` | S:2697-2699 |
| Dragging a model | `.s-models.dragging` | | every element shows `cursor: grabbing` with no text selection; the other rows slide (`transform 170ms cubic-bezier(0.2, 0, 0, 1)`) and lose their hover background; `.dragged` follows the pointer: `--raised`, `box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4)`, `z-index: 1`, its grip at full opacity and `--bone`, shadow transition `120ms`; `.settling` (the drop) turns transitions off so rows move without animating | S:2700-2708 |
| `.theme-picker` | inline-flex, `gap: 3px`, radius `9px`, `--well`, padding `3px`, `--control-face` | buttons: `min-height: 30px`, `gap: 6px`, radius `6px`, transparent, padding `5px 10px`, `12px` `--ash` | hover `--bone`; `.active` `--raised`, `--bone`, `--control-face`; `130ms ease` | S:1553-1580, S:2772, S:2781 |
| `.title-model-controls` | flex end, `gap: 8px`; the picker `flex: 1` (`0 1 auto` inside a row control); `.text-link` `12.5px`; at 560px and below the items stretch | | | S:1581-1599, S:2710-2712 |
| `.bind-key` (a shortcut) | `min-width: 64px`, `28px` high, radius `7px`, `--well`, `box-shadow: inset 0 0 0 1px var(--line), 0 1px 0 rgba(0, 0, 0, 0.35)`, padding `0 10px` | ui `500 12.5px`, `0.5px`, `--bone`; `.unset` `--dim-text`, no spacing | hover ring `--bevel-light`; `.recording` `--ash`, `bind-listen 1.2s ease-in-out infinite alternate` (the ring breathes between `20%` and `55%` bone) | S:3738-3763 |
| Connect an AI | `.connect-intro` margin `-12px 0 0`, `13px`/`1.5` `--ash`; ways grid `gap: 12px`; each way padding `14px 16px`, radius `10px`, `--well`, inset 1px `--line`, `gap: 6px`; title `13.5px` `600`; need `12.5px`/`1.5` `--ash`, `code` data `12px`; `.connect-pane` button `min-height: 32px`, padding `0 10px`, `gap: 7px`, radius `8px`, `--raised`, `--control-face`, ui `500 12.5px` `--bone`; state `400` `--dim-text`, `.connected` `--ash`; svg `--dim-text` | | pane hover `--hover`; focus-visible `2px solid var(--gold)`, offset `2px` | S:3567-3595 |
| `.leaderboard-table` | grid `20px minmax(0, 1fr) 56px 64px 60px`, `column-gap: 14px`, `row-gap: 1px`, body `13px`; rows use subgrid, `min-height: 36px`, padding `0 10px`, radius `8px`, `--ash`, tabular; the head is `28px`, `12px` `--dim-text`; columns from the third on are right-aligned; `.lb-model` flex `gap: 9px` `--bone` | | row hover `--hover` | S:3903-3926 |
| `.btn.danger` | `--redstone` fill, `#fff` text | | hover `color-mix(in srgb, var(--redstone) 86%, #000)`; disabled `--raised` and `--dim-text`; `.danger-confirm` flex `gap: 8px`, `--err-text`, `13px` | S:3927-3930 |
| Published builds | see 1.14 | | | S:4174-4195 |

### 1.11 Setup (first start)

| Selector | Box | Text | States | Lines |
|---|---|---|---|---|
| `.setup` | `margin: auto`, `width: min(100%, 460px)`, grid, `gap: 14px`, padding `24px 0`; at 980px and below `gap: 10px`, padding `0 0 12px` | | | S:3524, S:3553-3559 |
| `.setup-head` | grid, `gap: 6px`, centred | `.big`; `.setup-sub` `13px`/`1.5` `--ash`, balanced wrapping (`12px` at 980px and below) | | S:3525-3527 |
| `.setup-overall`, `.setup-bar` | flex `gap: 10px`; the bar is `4px` high, radius `2px`, track `color-mix(in srgb, var(--bone) 10%, transparent)`, fill `--ash` | `.setup-count` `12px` `--dim-text` tabular | the fill's width transitions over `400ms var(--ease)` | S:3528-3531 |
| `.setup-steps` | no list style, padding `6px 0` (`3px 0` narrow), radius `10px`, `--well`, inset 1px `--line` | | | S:3532 |
| `.setup-step` | grid `16px minmax(0, 1fr) auto`, `column-gap: 10px`, padding `6px 14px` (`3px 14px` narrow), `13px` | mark `--dim-text`, `18px` high; label `--ash`; value `12px` `--dim-text` tabular; `.setup-optional` `11.5px` `--dim-text`, `margin-left: 8px`; `.setup-note` in columns 2 and 3, `margin-top: 3px`, `12px`/`1.45` `--dim-text`, pre-wrap, underlined links | `.running`: label `--bone`, value `--ash`; `.done`: label `--dim-text`; `.failed`: mark, label, value and note `--err-text` | S:3533-3545 |
| Marks | `.setup-dot` `6x6` circle with a 1px `--dim-text` ring (pending); `.setup-dash` `8x1.5` `--dim-text` | | | S:3546-3547 |
| `.setup-failed` | flex centred, `gap: 10px`, `--err-text`, `12.5px`; its `.btn` is `min-height: 30px`, padding `0 12px`, ui `500 12.5px` | | | S:3548-3549 |
| `.connect-link` | padding `2px 4px`, transparent, ui `13px` `--ash`, underlined with a `3px` offset | | hover `--bone` | S:3511-3513 |

### 1.12 Prompt library and editor (Bench mode)

| Selector | Box | Text | States | Lines |
|---|---|---|---|---|
| `.prompt-editor` | grid, one column, `flex: 1`, `--bedrock`, clips | | | S:2054-2061 |
| `.pe-main` | grid rows `auto auto minmax(0, 1fr) auto` (title, tags, body, foot); with `.added-by` a mark row is added after the title (`auto auto auto minmax(0, 1fr) auto`); `width: min(100%, 820px)`, centred, `padding-top: 28px` | | | S:2153, S:2164-2171 |
| `.pe-main.pe-blank` | centred, `padding-bottom: 8vh` | `.pe-empty-state`: grid, `gap: 10px`, `max-width: 360px`, centred; `h2` `16px` `600` `--bone`; `p` `13px`/`1.55` `--ash`; `kbd` and `code` data `12px` `--bone` on `--raised`, padding `1px 5px`, radius `5px`; the image has `margin-bottom: 6px` | | S:2062-2078 |
| `.pe-title` | full width, no border or outline, transparent, padding `6px 26px 2px` | body `620 24px/1.2`, `-0.6px`, `--bone`; placeholder `--dim-text` | | S:2172-2182 |
| `.pe-tags` | flex wrap, `gap: 5px`, `min-height: 26px`, padding `4px 26px 0`; the input `flex: 1`, `min-width: 80px`, `22px`, ui `12px` | | | S:2183, S:2197-2198 |
| `.pe-tag` | inline-flex, `gap: 3px`, `22px`, padding `0 4px 0 8px`, radius `6px`, `color-mix(in srgb, var(--tag) 14%, transparent)` | ui `500 12px` in `--tag` | its x is `16x16`, radius `4px`, opacity `0.6`; hover opacity `1` on a `20%` tint | S:2184-2196 |
| `.pe-added-by` | padding `2px 26px 0` | | | S:2151 |
| `.pe-body` (textarea) | full width, no resize, border or outline, transparent, padding `10px 26px 16px` | body `14px/1.65` `--bone`; placeholder `--dim-text` | | S:2199-2210 |
| `.pe-foot` | flex wrap, `gap: 9px`, padding `10px 12px`; `.attach` `32x32` radius `8px` `--ash`, hover `--raised` and `--bone`; attachments flex wrap `gap: 7px` | | | S:2211-2237 |
| `.pe-ai-controls` | flex wrap end, `gap: 6px`, `margin-left: auto`; at 560px and below the full row | | | S:2238-2247, S:2439 |
| `.pe-ai-error` | full row, `max-width: 420px` (none at 560px and below), right-aligned, ellipsized | data `11px` `--err-text` | | S:2248-2257 |
| `.pe-ai-model` | `max-width: 170px`; trigger `max-width: 170px`, `min-height: 32px`, `gap: 5px`, padding `4px 7px`, label `max-width: 112px`; at 560px and below `138px`, `138px` and `82px` | | its popover opens upward | S:2258-2285, S:2440-2442 |
| `.pe-ai-split` | inline-flex, radius `8px`, `--control-face`; pressing the main half scales the whole split to `0.97` | | | S:2836-2839, S:2779 |
| `.pe-ai-generate` | inline-flex, `min-height: 32px`, `gap: 6px`, radius `8px 0 0 8px` inside the split, `--raised`, padding `6px 11px`, no shadow inside the split | body `500 12.5px` `--bone` | hover `--hover`; active (no scale) `inset 0 1px 2px rgba(0, 0, 0, 0.35)`; disabled `--raised` `--dim-text` at `0.64` | S:2294-2314, S:2778-2780 |
| `.pe-ai-mode` (the chevron half) | `26px` wide, `border-left: 1px solid var(--line)`, radius `0 8px 8px 0`, `--raised` | `--ash` | hover `--hover` and `--bone` | S:2840-2851 |
| Tips in the split | anchored left (`right: auto; left: 0`, origin bottom left); hidden while the menu or note is open | | | S:2895-2896 |

### 1.13 Bench

| Selector | Box | Text | States | Lines |
|---|---|---|---|---|
| `.bench-compare` | `gap: 22px` | | | S:3320 |
| `.bench-round` | column, `gap: 10px` | | | S:3321 |
| `.bench-prompt` | (a `.msg.user` bubble, clickable) | | hover border `--bevel-light`, `120ms` | S:3322-3323 |
| `.bench-results` | grid with fixed columns `18px 14px minmax(0, 1fr) 58px 50px 58px auto` (status, letter, model, three stats, actions), so arriving numbers move nothing; `column-gap: 10px`, `row-gap: 1px`, padding `4px`, radius `11px` | | `160ms var(--ease)` on background and shadow | S:3324-3333 |
| `.bench-result` | spans every column as a subgrid, `min-height: 36px`, padding `5px 6px 5px 9px`, radius `8px` | body `13px` `--ash`; `.bench-model` `--bone`, ellipsized; `.queued` and `.held` model `--ash`; `.picked` model `--done` | hover `--hover`; `.running` stats `--ash` | S:3334-3351, S:3786 |
| `.bench-stat` | right-aligned | `12px` `--dim-text`, tabular | | S:3352-3358 |
| `.bench-effort` | `margin-left: 8px` | `12px` `--dim-text`, capitalized | | S:3359-3365 |
| `.bench-status` | `18x18`, radius `5px`, `--dim-text` | | as a button: hover `--bone` on `--well`; `.done` `--done` with stroke `2.6`; `.failed` and `.stopped` `--err-soft`; working shows a running `.conv-dot` (`web/app.js:3021`, label `Working`); waiting shows `.bench-waiting`, a `7x7` circle with a `1.25px` ring of `color-mix(in srgb, var(--bone) 30%, transparent)` | S:3367-3385 |
| `.bench-letter` (blind) | `16x16`, radius `4px`, `--raised`, ring `--bevel-light` | ui `600 10.5px` `--bone` | | S:3775-3785 |
| `.bench-open` | `26x26`, radius `6px`, `--dim-text` | | row hover `--ash`; own hover `--bone` on `--well`; `120ms` | S:3386-3400 |
| `.bench-vote` | `justify-self: end`, inline-flex, `gap: 5px`, `24px`, padding `0 8px`, radius `7px` | ui `500 12px` `--dim-text` | row hover `--ash`; hover `--bone` on `--hover`; `.picked` `--done` on `color-mix(in srgb, var(--done) 10%, transparent)` with ring `45%`; `.bench-mark` (a vote that can no longer change) has no pointer events | S:3966-3989 |
| `.bench-stop` | `29px` wide (a trophy button's width), centred, no padding | | hover `--err-text` | S:3990-3993 |
| `.bench-tie-row` | the model `--ash`; `.bench-tie-note` `margin-left: 8px`, `12px` `--dim-text` | | | S:3994-3997 |
| New bench | `.messages.bench-new` `position: relative`; while its popover is open `z-index: 3; overflow: visible`, so it paints over the composer; `.bench-hint` full width, no inline padding; `.bench-lineup` centred, `760px` wide (`max-width: 100%`), `margin-top: 18px`, padding `0 16px`; the trigger is a pill: `30px`, padding `0 9px 0 11px`, radius `8px`, inset 1px `--line`, ui `500 12.5px`, transparent `--ash`, hover or focus `--hover` `--bone`; `.bench-need` `margin-top: -4px`, `12px` `--dim-text` | | | S:3464-3507 |

### 1.14 Share and published

| Selector | Box | Text | States | Lines |
|---|---|---|---|---|
| `.share-anchor` | `position: relative` around the header button | | | S:4133 |
| `.share-panel` | absolute, `z-index: 10`, `top: calc(100% + 7px); right: 0`, `300px` (`max-width: calc(100vw - 24px)`), column, `gap: 10px`, padding `12px`, radius `10px`, `--popover`, `--shadow-panel` | ui `12.5px/1.45` `--bone`; `.share-text` wraps anywhere, `.muted` `--dim-text`; `.share-step` `--ash`; `.share-error` `--err-text` | the viewer filling the window (z 15) covers it | S:4135-4156 |
| `.share-actions` | flex wrap, `gap: 8px`; buttons `min-height: 30px`, padding `5px 11px`, `12.5px` | | | S:4157-4159 |
| `.share-url` | full width, `32px`, padding `0 9px`, no border, radius `8px`, `--raised`, `--field-face` | data `12px` `--bone`, ellipsized | focus `inset 0 0 0 1px color-mix(in srgb, var(--gold) 70%, transparent), var(--glow-gold)` | S:4160-4172 |
| `.published` (Settings, Sharing card) | grid, `gap: 4px`, padding `4px 16px 14px` | `.published-note` flex wrap `gap: 10px`, padding `4px 0`, `12.5px` `--dim-text`; count links `--ash`, hover `--bone`; `.published-error` `--err-text`, its `.btn` `28px` padding `4px 10px` | | S:4174-4184 |
| `.published-row` | grid, `gap: 7px`, padding `10px 0`; rows after the first have a `5%` bone top hairline | title `13px` `500` `--bone`, ellipsized; meta `12px` `--dim-text`; row error `12px` `--err-text`; `.published-tag` as `.resolution-tag` | `.removing` dims the text to `0.55` | S:4185-4192, S:3894-3902 |
| `.published-actions` | flex wrap, `gap: 6px`; its `.header-action`s are `26px`, padding `0 9px`, `gap: 5px`, `12px` | | `.quiet.armed` `--err-text` | S:4193-4195 |

### 1.15 Viewer chrome and live controls

| Selector | Box | Text | States | Lines |
|---|---|---|---|---|
| `.viewer` | `position: relative`, `min-width: 0`, no border, `#000` in both themes, `color-scheme: dark`; at 980px and below row 2, `min-height: 38vh` | title attribute `Double-click to expand`, or `Double-click or press Esc to exit fullscreen` when full (`web/app.js:7012`) | `.full`: fixed, `inset: 0`, `z-index: 15` (double-click toggles it, Esc leaves it) | S:584-596, S:1473-1476, S:1521 |
| `.bluemap-shell`, `.bluemap-frame` | `100%` both ways; shell relative, `#000`; frame (the iframe) block, no border, `#000`, `color-scheme: dark` | | | S:597-608 |
| `.live-controls` (play, pause, step, reset, ticks) | absolute, `left: 12px; bottom: 12px`, flex, `gap: 2px`, padding `4px`, `1px solid var(--line)`, radius `10px`, `--raised` | `--ash` | | S:609-621 |
| `.live-controls button` | `28x28`, radius `7px`, transparent, centred | inherits | hover (not disabled) `--line` as fill; disabled opacity `0.4`, default cursor | S:622-634 |
| `.live-ticks` | padding `0 8px 0 4px` | `12px`, tabular numbers | | S:635 |
| `.bluemap-mismatch` | absolute, `inset: auto 16px 16px` (along the bottom), flex centred, `gap: 12px`, padding `12px 16px`, `1px solid var(--line)`, radius `10px`, `--raised` | `13px` `600` `--ash` | | S:637-651 |
| `.bluemap-placeholder` | full size, flex centred, `gap: 9px`, `#000` | `13px` `550` `--ash` | | S:652-663 |
| `.bluemap-loading` | absolute, `inset: 0` | | | S:664 |
| `.block-icon` | block, no flex, `image-rendering: auto` | | | S:2728 |
| `.app-mark` | `--bone`, `overflow: visible` | | | S:2729 |

The texts inside these, and everything drawn inside the frame, are in Part 2.

### 1.16 Lightbox, command panel, banners and errors

| Selector | Box | Text | States | Lines |
|---|---|---|---|---|
| `.lightbox` | fixed, `inset: 0`, `z-index: 30`, centred grid, `cursor: zoom-out`, `rgba(0, 0, 0, 0.78)`, `backdrop-filter: blur(12px)` | | `fade-in 150ms` (no-preference) | S:513-520, S:1466-1469 |
| `.lightbox img` | `max-width: min(1100px, 88vw)`, `max-height: 88vh`, `1px solid var(--line)`, radius `12px`, `--shadow-panel`, `image-rendering: pixelated` | | `pop-in 180ms` | S:521-527, S:1470 |
| `.cmdk-overlay` (search everything) | fixed, `inset: 0`, `z-index: 35`, flex, centred horizontally and at the top, padding `14vh 16px 16px`, `rgba(0, 0, 0, 0.5)`, `backdrop-filter: blur(6px) saturate(0.85)` | | `cmdk-fade 120ms ease-out` (not gated by reduced motion) | S:3999-4014 |
| `.cmdk` | column, `width: min(640px, 100%)`, `max-height: min(540px, 72vh)`, clips, radius `14px`, `--popover`, `--shadow-panel` | | `surface-in 140ms var(--ease)` | S:4015-4025 |
| `.cmdk-input` | flex, `gap: 11px`, `54px`, padding `0 16px 0 18px`, `--dim-text` | input body `15px` `--bone`; `kbd` and `.cmdk-hint` padding `2px 6px`, radius `5px`, inset 1px `--line`, ui `11px`, `0.4px`, `--dim-text` | | S:4026-4054 |
| `.cmdk-list` | scrolls, `overscroll-behavior: contain`, padding `0 8px 8px`; headings padding `12px 10px 5px`, ui `500 11.5px` `--dim-text` | | | S:4055-4056 |
| `.cmdk-item` | flex, `gap: 11px`, `min-height: 42px`, padding `7px 10px`, radius `9px`, `--ash` | icon `26x26` radius `7px`, `--well`, inset 1px `--line`; title `13.5px` `--bone`; detail `12px` `--dim-text`; both ellipsized; `mark` `--bone` `600`, no background; empty padding `30px 10px 26px`, `13px` centred | `.active` `--raised`, icon `--bone` | S:4057-4086 |
| `#page-error` (a failure nothing else caught, `web/page-errors.js`) | fixed, `left: 16px; right: 16px; bottom: 16px`, `z-index: 1000`, flex, `gap: 12px`, padding `10px 12px`, radius `8px`, `--raised`, inset 1px `--line` | ui `13px` `--err-text`; the text flexes and wraps anywhere; its button reads `Reload` (`web/index.html:17`) | | S:4121-4129 |
| `.crash` (ErrorBoundary) | flex column, `gap: 12px`, `max-width: 720px`, padding `32px 24px` | see 1.3 | | S:4115-4120 |
| `.app-notices` | see 1.6 | | | S:4093-4113 |
| `.msg.error`, `.settings-error`, `.model-picker-error` | each carries a dismiss button (1.6) | | | S:1024-1050 |

### 1.17 Tooltips

| Property | Value | Lines |
|---|---|---|
| Source | `data-tip` on the element. The app sets it in four places: the context meter, the limit chip, every Settings row control (`SettingsRow tip`), and the two Improve with AI tips, `Rewrites the text above` and `Writes the prompt from a short description` (`web/app.js:189, :294, :668, :2469`) | |
| Drawing | `[data-tip]::after { content: attr(data-tip) }`, absolute, `right: 0; bottom: calc(100% + 8px)`, so it sits above the element and is right-aligned with it; `z-index: 300`; `width: max-content`; `max-width: 260px`; padding `5px 9px`; radius `7px`; `--popover`; `--bone`; ui `12px/1.45`; left-aligned; `text-wrap: balance`; `box-shadow: var(--highlight), 0 0 0 1px var(--line), 0 8px 24px rgba(0, 0, 0, 0.35)` | S:2784-2806 |
| Hidden state | opacity `0`, `scale(0.98) translateY(2px)`, origin bottom right, no pointer events | S:2801-2804 |
| Show | on hover: opacity `1` and no transform, after a delay of `500ms`; transition `opacity 120ms ease, transform 120ms ease`. Hiding has no delay. | S:2805-2807 |
| Suppressed | when the element contains an open model, effort or resolution menu; when `data-tip=""`; inside `.pe-ai-split` while its menu or note is open | S:2808-2809, S:2896 |
| Variants | in `.composer-controls` the gap is `10px`; in `.pe-ai-split` it is left-anchored with origin bottom left | S:2937, S:2895 |
| Keyboard | there is no `:focus` rule, so tooltips appear only on pointer hover | |
| Native titles | many controls use the HTML `title` attribute instead (for example the viewer's `Double-click to expand`, and `Show sidebar`). The browser draws those with its own delay and style. | |

### 1.18 Shared controls

| Selector | Final box | Text | States | Lines |
|---|---|---|---|---|
| `.btn` | `min-height: 32px`, radius `8px`, `--raised`, padding `6px 12px`, no border, `--control-face`, pointer | body `13px` `500` `--bone` | hover `color-mix(in srgb, var(--raised) 84%, var(--bone))`; active (not disabled) `scale(0.97)` with `--control-pressed`; disabled `--raised`, `--dim-text`, no shadow, no transform, default cursor, opacity `0.64`; transition `transform 80ms ease, box-shadow 80ms ease, background 120ms ease` | S:42-50, S:762-786, S:2772-2780, S:2909 |
| `.btn.gold` (primary) | `--bone` fill, `--primary-face` | `--bedrock` | hover `color-mix(in srgb, var(--bone) 88%, var(--ash))`; active `scale(0.97)` with `--primary-pressed`; disabled `--raised` and `--dim-text` at `0.64`, and it keeps `--primary-face` (see 1.1) | S:776-781, S:2774, S:2777 |
| `.btn.danger` | `--redstone` fill | `#fff` | see 1.10 | S:3927-3930 |
| `.icon-btn` | see 1.5 | | | S:53-62, S:788-794 |
| `.text-link` | no padding, border or background | inherits the font, `--ash`, underlined in `color-mix(in srgb, var(--ash) 40%, transparent)`, offset `3px` | hover `--bone`, underline `--bone`; `120ms` | S:3934-3946 |
| `:focus-visible` (global) | `outline: 2px solid color-mix(in srgb, var(--gold) 85%, white); outline-offset: 2px`. In dark mode this is nearly white; in light mode a dark grey. | | overridden per component where noted (summaries, nav items, the composer input, the model trigger, the slider) | S:757-760 |
| `.toggle` | see 1.10 | | | S:2502-2523 |
| `.vlogo` | inline-block, `background: currentColor` masked by `--icon` (`center / contain no-repeat`). A native build must draw these SVGs as a tint mask in the text colour, or in the brand fills listed in 1.2. | | | S:164-171 |

### 1.19 Motion

| Name | Duration and easing | What it shows | Reduced motion | Lines |
|---|---|---|---|---|
| `rise` | `200ms var(--ease) both` | a new transcript item rising 5px into place | not run | S:319-326 |
| `fold-in` | `180ms var(--ease)` | a fold opening (`-3px` and fading in) | not run | S:459-466 |
| `fade-in` | `150ms var(--ease)` | the lightbox and settings backdrops | not run | S:528-536 |
| `pop-in` | `180ms var(--ease)` | the lightbox image and the settings panel (`scale(0.97) translateY(6px)`) | not run | S:528-536 |
| `working-dot` | `2s steps(6) infinite`, delays `0`, `200ms`, `400ms` | the working indicator | none; opacity `0.45` | S:1350-1367 |
| `surface-in` / `surface-out` | `170ms` (model popover) or `140ms` (palette, Improve with AI, command panel) `cubic-bezier(0.2, 0.7, 0.3, 1)`; out `120ms ease-in` | popovers opening and closing (opacity and scale `0.985`/`0.99`) | the model popover only: none | S:1688-1709, S:1994, S:2860, S:4024 |
| `effort-menu-in` / `-out`, `-up-in` / `-up-out` | `150ms cubic-bezier(0.2, 0.7, 0.3, 1) both`; out `110ms ease-in both` | the effort, tag, resolution and bench effort menus (`4px` slide and `0.98` scale) | effort and resolution menus: none; the tag and bench effort menus still animate | S:1710-1727, S:2401-2408 |
| `field-missing` | `700ms ease` | a fill-in left empty on send | still runs | S:1799-1802 |
| `conv-arrive` | `700ms ease-out` | a chat finished: grey to blue | still runs | S:2955-2959 |
| `conv-pulse` | `0.9s ease-in-out infinite alternate`, every dot on one clock | a chat running | `3s` | S:2960-2968 |
| `mode-swap` | `700ms ease-out` | the new mode word arriving pure white | still runs | S:3013-3017 |
| `bind-listen` | `1.2s ease-in-out infinite alternate` | a shortcut being recorded | still runs | S:3755-3763 |
| `cmdk-fade` | `120ms ease-out` | the command panel's backdrop | still runs | S:4012-4014 |
| `setting-flash` | `1.4s ease-out` | the settings row a search result opened | still runs | S:4087-4091 |
| Transitions | | every hover colour and background (`110ms` to `160ms`); chevrons (`160ms` or `200ms`); the sidebar search growing (`flex-grow 220ms var(--ease)`); prompt reordering (`150ms cubic-bezier(0.2, 0.7, 0.2, 1)`); model reordering (`170ms cubic-bezier(0.2, 0, 0, 1)`); the setup bar (`400ms`); the toggle thumb (`150ms`, width `100ms` after `100ms`); pressed controls (`80ms`) | only the picker triggers, their chevrons and model rows lose their transitions (S:1723-1726) | various |

### 1.20 Media queries and the narrow window

There are no container queries, no `@supports` rules and no `prefers-color-scheme` rules. The width breakpoints are `980px`, `720px` and `560px`, all `max-width`. Besides those there is one `hover: none` query and six `prefers-reduced-motion` blocks.

| Query | Lines | What changes |
|---|---|---|
| `(prefers-reduced-motion: no-preference)` | S:320, S:459, S:528 | `rise`, `fold-in`, `fade-in` and `pop-in` run only here |
| `(prefers-reduced-motion: reduce)` | S:1365, S:1719, S:2966 | working dots stop; model, effort and resolution menus do not animate; trigger and row transitions stop; the running dot slows to `3s` |
| `(max-width: 980px)` | S:1478-1528 | the narrow layout, below |
| `(max-width: 560px)` | S:1530-1548 | rows `52px 36dvh 1px minmax(0, 1fr)`; wordmark `13px`; the chat header becomes a grid `minmax(0, 1fr) auto auto` with a `13px` title; the empty hint shrinks; settings becomes a bottom sheet |
| `(max-width: 560px)` | S:1597-1599 | `.title-model-controls` stretch |
| `(hover: none)` | S:1969-1971 | controls that hover reveals are always shown at opacity `1`: `.conv-item .archive`, `.user-block .mtool`, `.branch-picker`, `.model-star`, `.pp-edit`, `.ce-remove`, `.pe-item-actions button` and `.s-grip` |
| `(max-width: 980px)` | S:1973-1976 | Prompts mode: one column, rows `52px minmax(0, 1fr)`, the editor in row 2 |
| `(max-width: 720px)` | S:2046-2049 | the prompt palette loses its preview and is `300px` high |
| `(max-width: 560px)` | S:2410-2444 | the model popover's phone layout (1.9) and the prompt editor's narrower AI controls |
| `(max-width: 980px)` | S:2716-2726 | Settings stacks its nav above the pane; rows go single-column; model rows drop their id and capabilities |
| `(max-width: 980px)` | S:3553-3559 | setup takes less height |

The narrow window, at 980px and below (S:1478-1528), with `web/app.js:1230-1233` (`NARROW = '(max-width: 980px)'`, which the app also tracks through `matchMedia`, `web/app.js:1236-1246, :3563`):

| Part | Wide | 980px and below |
|---|---|---|
| Grid | four columns, one row | one column; rows `52px` (bar), `38dvh` (world), `1px` (divider), `minmax(0, 1fr)` (chat); `height: 100dvh`. At 560px and below the world row is `36dvh`. |
| Sidebar | a column on the left | row 1, a horizontal bar: `flex-direction: row`, `gap: 6px`, padding `6px 10px`, a bottom border instead of a right one. Only `.brand` and `.sidebar-foot` stay visible; every other child is `display: none` unless the drawer is open (S:1498). Brand: `min-height: 38px`, no padding, `gap: 6px`, wordmark `em` hidden; `.brand-actions` is `display: contents`, so its buttons join the bar; `.sidebar-toggle` moves first (`order: -1`); `.sidebar-foot` (Settings) is pushed right. The resize edge and the reveal strip are hidden. |
| Drawer | none | `.layout.drawer-open > .sidebar` is fixed on the left (`inset: 0 auto 0 0`), `z-index: 12`, `width: min(340px, 88vw)`, a column again (`gap: 8px`, padding `10px 9px 9px`), scrolls, right border, `--shadow-panel`. `.drawer-scrim` is fixed full-screen at `z-index: 11`, `rgba(0, 0, 0, 0.55)`; clicking it closes the drawer (`web/app.js:6529`). The drawer is a modal layer (`web/layers.mjs`). |
| World | right column | row 2, `min-height: 38vh` |
| Divider | 5px vertical | row 3, 1px high; the `::after` line fills it (`inset: 0; width: auto; height: 1px`). Its `cursor: col-resize` and its drag handler are unchanged, so a drag still writes `--chat-col` and `mcb-chat-width` (`web/app.js:4200-4215`), which the one-column grid ignores. A drag here therefore has no visible effect. That reads as an oversight. |
| Chat | second column | row 4, no right border; header `min-height: 49px`, padding `8px 12px`; transcript padding `20px 16px 14px`; composer padding `8px 12px 12px` |

### 1.21 Layers, cursors, scrollbars, selection, pseudo-elements

#### Z-order, from the top

| z-index | Layer | Lines |
|---|---|---|
| `1000` | `#page-error` | S:4123 |
| `400` | `.app-notices` | S:4099 |
| `300` | tooltips | S:2790 |
| `100` | `.model-picker-popover` | S:2319 |
| `90` | `.effort-picker-menu`, `.tag-menu`, `.resolution-picker-menu` | S:1638, S:3140, S:3815 |
| `60` | `.pe-ai-menu`, `.pe-ai-note` | S:2856 |
| `40` | `.prompt-palette` | S:1986 |
| `35` | `.cmdk-overlay` | S:4005 |
| `30` | `.lightbox` | S:516 |
| `20` | `.overlay` (Settings) | S:543 |
| `15` | `.viewer.full` | S:595 |
| `12` | the narrow drawer | S:1508 |
| `11` | `.drawer-scrim` | S:1520 |
| `10` | `.share-panel` | S:4137 |
| `5` | `.sidebar-edge`, `.sidebar-reveal`; `.s-card` holding an open picker | S:884, S:2561 |
| `3` | `.messages.bench-new` with an open popover | S:3471 |
| `2` | `.s-row-control`, `.pe-item.lifted` | S:2560, S:2977 |
| `1` | `.settings-close`, `.s-model.dragged` | S:2526, S:2707 |

The comment at S:4000-4001 and `web/layers.mjs:1-13` give the Escape order: command panel, lightbox, Settings, drawer, the viewer filling the window, the share panel. The first four are modal and take the keyboard from the app (`web/layers.mjs:25-31`). Popovers at `z-index` 90 to 300 sit inside their stacking contexts: the model popover in Settings is lifted with the card at z 5 inside the overlay at z 20.

#### Cursors

| Cursor | Where | Lines |
|---|---|---|
| `col-resize` | `.strata`, `.sidebar-edge`, `body.dragging` | S:80, S:82, S:886 |
| `e-resize` | `.sidebar-reveal` | S:889 |
| `zoom-in` | `.log-shots img` | S:431 |
| `zoom-out` | `.lightbox` | S:519 |
| `grab` | `.s-grip` | S:2697 |
| `grabbing` | `.s-models.dragging *`; `body.grabbing *` with `!important` (prompt reordering) | S:2702, S:2990 |
| `text` | `.composer-input`, `.side-find-field` | S:497, S:3630 |
| `not-allowed` | a disabled provider in the rail | S:216 |
| `default` | disabled buttons, `.ce-prompt`, `.selection-chip`, `.context-meter`, `.bench-mark`, a disabled `.live-controls` button | various |
| `pointer` | every other button and clickable row | various |

#### Scrollbars

`::-webkit-scrollbar` is `6px` both ways. Its thumb is fully rounded, `color-mix(in srgb, var(--bone) 14%, transparent)`, and `24%` on hover; the track is transparent (S:38, S:750-755). Later, S:2826-2827 sets `* { scrollbar-width: thin; scrollbar-color: rgba(255, 255, 255, 0.08) transparent }`, with `rgba(0, 0, 0, 0.15) transparent` in light mode. `.settings-nav` uses `scrollbar-width: none` (S:2461); `.pp-list` uses `thin` (S:2000). In Chromium 121 and later, an element with `scrollbar-width` or `scrollbar-color` set ignores `::-webkit-scrollbar` styling. So in today's Chrome the standard thin scrollbar with the `0.08` or `0.15` thumb is what shows, and the 6px webkit rules apply only in Safari. The file sets both, so this is ambiguous; the comment at S:2825 ("Thin, quiet scrollbars") suggests the later rule is the intended look.

#### Text selection

The stylesheet has no `::selection` rule and sets no `caret-color`, so selection and the caret use the browser's defaults. `user-select: none` is set on `.conv-item`, fold and summary rows, `.ce-prompt`, `.pe-item`, and everything during a drag (S:118, S:394, S:450, S:1234, S:1294, S:1757, S:2702, S:2972, S:82).

#### Things drawn by pseudo-elements or masks

| What | How | Lines |
|---|---|---|
| The divider's line and its hover glow | `.strata::after` | S:808-819 |
| The sidebar edge's hover line, and the reveal strip's | `::after`, 2px | S:890-902 |
| The composer's placeholder | `::before` with `attr(data-placeholder)` | S:499-503 |
| Tooltips | `::after` with `attr(data-tip)` | S:2785-2809 |
| Vendor logos | `mask: var(--icon)` over `currentColor` or a brand fill | S:165-171 |
| Hidden disclosure triangles | `::-webkit-details-marker { display: none }` on every summary | S:395, S:452, S:1236, S:1296 |
| Hairlines drawn as inset shadows, not borders | `.pp-search` bottom, `.pp-foot` top, `.s-row + .s-row` top, `.published-row + .published-row` top, and every `box-shadow: inset 0 0 0 1px var(--line)` ring | S:1998, S:2033, S:2567, S:4186 |
| The reasoning rail | `border-left: 2px solid var(--line)` | S:1208 |
| The active plan item's centre dot | `radial-gradient` | S:2924 |
| The composer's fade | `linear-gradient(to bottom, transparent, var(--cave) 12px)` | S:1386 |
| Backdrop blur | `.overlay` `blur(10px) saturate(0.8)`, `.lightbox` `blur(12px)`, `.cmdk-overlay` `blur(6px) saturate(0.85)` | S:1434, S:1468, S:4011 |

### 1.22 Notes on workarounds and defects in the stylesheet

| Note | Lines |
|---|---|
| `min-height: 0` on grid items and `.model-picker-main`, with comments that the list would not scroll without it (a CSS grid quirk) | S:72-74, S:231 |
| A transparent border on raised controls would draw a second top line, so all borders are zeroed and the edge is a shadow ring | S:2906-2913 |
| `user-select: none` on chat rows exists because Chrome draws no ellipsis on a line that holds the caret | S:112-113 |
| Chip text uses `text-box: trim-both cap alphabetic` and a `1cap - 1ex` offset for optical centring. Browsers without `text-box` (Firefox at the time of writing) draw it untrimmed. | S:3244-3267 |
| The `--chat-col` fallback in the stylesheet is dead (1.1) | S:797 |
| The divider still resizes the chat in the one-column layout, with no visible effect (1.20) | S:1522-1523 |
| `.log-step .d.mono` uses the body face, not the monospace, despite its name | S:1163 |
| A disabled `.btn.gold` keeps its primary shadow (1.1) | S:45, S:2774 |
| Both scrollbar systems are set (1.21) | S:750, S:2826 |
| `.settings-error` uses a literal `rgba(224, 93, 79, 0.08)`, not a token, so its tint does not change with the theme | S:575 |
| Reduced motion leaves several animations running (1.19) | |

### 1.23 Class index

Every class the stylesheet styles, with each line where a rule for it starts (comments excluded). The sections above give the values; this index is for finding every rule that touches a class, including the ones the region tables describe by role rather than by name.

402 classes.

| Class | Rule lines in `web/style.css` |
|---|---|
| `.active` | 2974 |
| `.activity` | 446, 452, 453, 1062, 1068, 1077, 1078, 1085, 1097 |
| `.activity-body` | 454, 460, 1098, 1104, 1105, 1106 |
| `.activity-chevron` | 1092, 1097 |
| `.app-mark` | 2729 |
| `.app-notice` | 4108 |
| `.app-notices` | 4097 |
| `.app-notices-anchor` | 4096 |
| `.archive` | 126, 131, 136, 869, 879, 1970 |
| `.archived` | 3703, 3704 |
| `.archived-list` | 3705 |
| `.archived-meta` | 3719, 3720 |
| `.archived-row` | 3707, 3717, 3718 |
| `.attach` | 1816, 1833, 1838, 2045, 2218, 2230 |
| `.attachment-copy` | 1908, 1909, 1917 |
| `.bench-caret` | 3022, 3036, 3037, 3038 |
| `.bench-chip` | 3222 |
| `.bench-chip-effort` | 3249, 3261, 3278, 3293, 3294, 3295 |
| `.bench-chip-icon` | 3260, 3272, 3273, 3274, 3275, 3276 |
| `.bench-chip-model` | 3232, 3275, 3276, 3277 |
| `.bench-chip-name` | 3248, 3268, 3277 |
| `.bench-compare` | 3320 |
| `.bench-effort` | 3359 |
| `.bench-effort-add` | 3308, 3317 |
| `.bench-effort-menu` | 3296, 3305, 3308, 3317, 3596, 3602 |
| `.bench-effort-row` | 3306, 3307 |
| `.bench-letter` | 3775 |
| `.bench-lineup` | 3473, 3481, 3482, 3483, 3484, 3493, 3494, 3495, 3498, 3596, 3602 |
| `.bench-list` | 3020 |
| `.bench-mark` | 3989 |
| `.bench-member` | 3045 |
| `.bench-members` | 3039 |
| `.bench-model` | 3349, 3350, 3351, 3786, 3996 |
| `.bench-models` | 3215 |
| `.bench-need` | 3507 |
| `.bench-open` | 3386, 3399, 3400 |
| `.bench-prompt` | 3322, 3323 |
| `.bench-result` | 3334, 3348, 3350, 3351, 3366, 3381, 3382, 3383, 3384, 3399, 3786, 3981 |
| `.bench-results` | 3324 |
| `.bench-round` | 3321 |
| `.bench-row` | 3021 |
| `.bench-stat` | 3352, 3366 |
| `.bench-status` | 3367, 3381, 3382, 3383, 3384 |
| `.bench-stop` | 3992, 3993 |
| `.bench-tie-note` | 3997 |
| `.bench-tie-row` | 3996 |
| `.bench-vote` | 3966, 3981, 3982, 3983 |
| `.bench-waiting` | 3385 |
| `.big` | 475, 1374, 1540, 3526, 3555 |
| `.bind-key` | 3739, 3753, 3754, 3755 |
| `.block-icon` | 2728 |
| `.bluemap-frame` | 598, 603 |
| `.bluemap-loading` | 664 |
| `.bluemap-mismatch` | 637 |
| `.bluemap-placeholder` | 652 |
| `.bluemap-shell` | 597, 602 |
| `.branch-picker` | 995, 1007, 1008, 1009, 1010, 1970 |
| `.brand` | 93, 97, 98, 829, 834, 844, 1498, 1499, 1500, 1533, 2993 |
| `.brand-actions` | 101, 850, 1501 |
| `.btn` | 42, 45, 762, 774, 775, 776, 781, 782, 2772, 2774, 2775, 2776, 2777, 2780, 2894, 2909, 3549, 3927, 3928, 3930, 4158, 4183 |
| `.cap-warn` | 306, 945 |
| `.ce-field` | 1782, 1799 |
| `.ce-field-input` | 1783, 1797, 1798, 1799 |
| `.ce-prompt` | 1746, 1780 |
| `.ce-remove` | 1766, 1780, 1970 |
| `.ce-slash` | 1760 |
| `.chat` | 145, 911, 1524, 1966, 1975 |
| `.chat-header` | 151, 155, 915, 921, 1525, 1534, 1538, 2733, 2734, 3175, 3184 |
| `.cmd-chip` | 2593, 2602, 2603, 2614, 2629, 2772, 2909 |
| `.cmdk` | 4015, 4085 |
| `.cmdk-detail` | 4082, 4084 |
| `.cmdk-empty` | 4086 |
| `.cmdk-heading` | 4056 |
| `.cmdk-hint` | 4046 |
| `.cmdk-icon` | 4068, 4079 |
| `.cmdk-input` | 4026, 4035, 4044, 4045 |
| `.cmdk-item` | 4057, 4067, 4079 |
| `.cmdk-list` | 4055 |
| `.cmdk-overlay` | 4002 |
| `.cmdk-text` | 4080 |
| `.cmdk-title` | 4081, 4083 |
| `.code` | 397, 404, 461, 1309 |
| `.code-awaiting` | 1321 |
| `.composer` | 481, 1383, 1527 |
| `.composer-attachment` | 1885, 1897, 1922, 1933, 1935 |
| `.composer-attachments` | 1879 |
| `.composer-box` | 485, 1388, 1394, 1730, 1980 |
| `.composer-controls` | 1809, 1817, 1818, 1847, 1848, 1853, 1858, 1861, 1867, 1868, 1869, 1874, 2389, 2396, 2829, 2830, 2831, 2832, 2833, 2834, 2937 |
| `.composer-editing` | 3564, 3565 |
| `.composer-footer` | 1803, 1816, 1833, 1838, 2045, 3809 |
| `.composer-held` | 3561, 3562 |
| `.composer-input` | 491, 492, 499, 1398, 1402, 1737 |
| `.composer-input-host` | 490, 499, 1402 |
| `.composer-model` | 1839, 1842, 2378, 2382, 2900, 2901, 2902, 2903, 2904 |
| `.composer-selections` | 3403, 3405 |
| `.composer-toggle` | 3788, 3802, 3803 |
| `.connect-intro` | 3570 |
| `.connect-link` | 3511, 3512, 3513 |
| `.connect-pane` | 3577, 3591, 3592, 3594, 3595 |
| `.connect-pane-state` | 3593, 3594 |
| `.connect-way` | 3572 |
| `.connect-way-need` | 3574, 3575 |
| `.connect-way-panes` | 3576 |
| `.connect-way-title` | 3573 |
| `.connect-ways` | 3571 |
| `.context-meter` | 2929, 2930, 2931, 2932, 2933, 2934 |
| `.conv-dot` | 2944, 2951, 2956, 2960, 2967, 3046 |
| `.conv-item` | 114, 122, 125, 126, 131, 136, 852, 862, 863, 869, 879, 1970 |
| `.conv-list` | 110, 851 |
| `.conv-snippet` | 3691, 3698 |
| `.conv-text` | 3689, 3690 |
| `.copy-btn` | 2629, 2630, 2641 |
| `.crash` | 4118, 4119, 4120 |
| `.crumb` | 2733 |
| `.crumb-sep` | 2734 |
| `.d` | 1153, 1163 |
| `.danger-confirm` | 3929 |
| `.diffstat` | 384, 388, 389, 390, 1165 |
| `.drawer-open` | 1498 |
| `.drawer-scrim` | 1520 |
| `.effort-option-label` | 1673 |
| `.effort-picker` | 1602, 1679 |
| `.effort-picker-menu` | 1636, 1651, 1655, 1671, 1672, 1678, 1721, 2286, 2293, 2389, 2396, 2561, 2808 |
| `.effort-picker-trigger` | 1606, 1622, 1623, 1630, 1632, 1680, 1723, 1724, 1847, 2274, 2772, 2775, 2776, 2830, 2833, 2911, 2913 |
| `.empty-hint` | 468, 475, 1369, 1374, 1539, 1540, 3472 |
| `.empty-sub` | 3550, 3931 |
| `.error-dismiss` | 1036, 1050 |
| `.file-tile` | 1898, 1959 |
| `.fill` | 2933, 2934 |
| `.find-chip` | 3262, 3265, 3661, 3675, 3677, 3685, 3686 |
| `.find-chip-text` | 3250, 3676 |
| `.find-chips` | 3660 |
| `.find-clear` | 3678, 3687, 3688 |
| `.find-filter` | 3645 |
| `.find-filter-button` | 3646, 3647 |
| `.find-menu-empty` | 3658 |
| `.find-menu-group` | 3649 |
| `.harness-badge` | 221 |
| `.header-action` | 2735, 2749, 2750, 2751, 2774, 2775, 2777, 2780, 3210, 3211, 3212, 3787, 4134, 4194, 4195 |
| `.history-icon-slot` | 1239, 1298, 1299 |
| `.icon-btn` | 53, 62, 788, 794 |
| `.layout` | 64, 70, 71, 74, 796, 800, 1480, 1481, 1489, 1498, 1505, 1519, 1531, 1532, 1962, 1965, 1966, 1974, 1975 |
| `.lb-head` | 3923, 3924 |
| `.lb-model` | 3926 |
| `.lb-row` | 3912, 3923, 3925 |
| `.leaderboard-row` | 3903, 3904 |
| `.leaderboard-table` | 3905 |
| `.lightbox` | 513, 521, 529, 530, 1466, 1470 |
| `.limit-chip` | 2935, 2936 |
| `.live-controls` | 609, 622, 633, 634 |
| `.live-ticks` | 635 |
| `.live-tool-call` | 1109, 1272, 1308 |
| `.live-tool-stack` | 1257, 1267, 1268, 1269, 1271 |
| `.log-code` | 393, 394, 395, 396, 461, 1081, 1108, 1124 |
| `.log-entry` | 375, 1107, 1110 |
| `.log-err` | 380, 1191 |
| `.log-note` | 417, 1104, 1197 |
| `.log-shots` | 422, 427, 436, 1106, 1308 |
| `.log-step` | 376, 1111, 1124, 1143, 1152, 1153, 1163 |
| `.m` | 390 |
| `.md` | 329, 331, 332, 336, 1053, 1054, 1055 |
| `.md-h` | 509, 1052 |
| `.md-line` | 330, 1051 |
| `.messages` | 310, 321, 959, 1266, 1267, 1268, 1269, 1270, 1271, 1526, 3468, 3471 |
| `.meta` | 139, 903 |
| `.missing-actions` | 3933 |
| `.mode-swap` | 3008 |
| `.mode-switch` | 2993 |
| `.mode-word` | 2994, 3007, 3011, 3012, 3014 |
| `.model-check` | 3449, 3461, 3462 |
| `.model-choice` | 264, 2374 |
| `.model-name` | 263, 275, 1429, 2375 |
| `.model-picker` | 160, 1587, 2711, 3481 |
| `.model-picker-connect` | 3517 |
| `.model-picker-empty` | 297 |
| `.model-picker-error` | 298, 304, 1027, 1035 |
| `.model-picker-footer` | 3947, 3964, 3965 |
| `.model-picker-list` | 255, 2372 |
| `.model-picker-main` | 229 |
| `.model-picker-no-ai` | 3515 |
| `.model-picker-no-ai-text` | 3516 |
| `.model-picker-popover` | 192, 1405, 1688, 1691, 1720, 2279, 2317, 2382, 2397, 2411, 2561, 2808, 3471, 3498 |
| `.model-picker-row` | 259, 263, 291, 1427, 1428, 1695, 1726, 2373 |
| `.model-picker-search` | 235, 243, 244, 252, 253, 254, 1424, 2367, 2368, 2369 |
| `.model-picker-trigger` | 172, 181, 186, 187, 928, 938, 939, 1839, 1842, 2262, 2268, 2378, 2441, 2442, 2772, 2775, 2776, 2829, 2832, 2901, 2903, 2904, 2910, 3482, 3483, 3484, 3493, 3494, 3495 |
| `.model-provider` | 3519 |
| `.model-provider-rail` | 198, 202, 210, 213, 216, 219, 220, 221, 1410, 1414, 1418, 1419, 2345, 2353, 2354, 2358, 2420, 2429, 2430, 2431 |
| `.model-star` | 282, 291, 292, 293, 294, 295, 296, 1970, 2015, 2016, 2131, 2135, 2136, 2376, 2989 |
| `.msg` | 317, 344, 362, 366, 968, 973, 994, 1016, 1017, 1025, 1033 |
| `.msg-attachments` | 1936 |
| `.msg-file` | 1942, 1957, 1958, 1959 |
| `.msg-selections` | 3404, 3406 |
| `.msg-tools` | 347 |
| `.mtool` | 352, 360, 981, 990, 992, 1009, 1010, 1970 |
| `.name` | 2495, 2497 |
| `.nav-text` | 2494, 2496, 2497, 2498, 2499, 2500 |
| `.notice-dot` | 2926, 2927, 2928 |
| `.notice-line` | 2925, 2927, 2928 |
| `.overlay` | 529, 540, 1432, 1547 |
| `.p` | 389 |
| `.page-error-text` | 4129 |
| `.page-loading` | 4117 |
| `.pane-head` | 2537, 2538 |
| `.pane-version` | 2539 |
| `.panel-actions` | 579, 1463, 1592, 2536 |
| `.panel-close` | 553, 562, 1444, 1452 |
| `.pe-added-by` | 2151 |
| `.pe-ai-controls` | 2238, 2274, 2286, 2293, 2439 |
| `.pe-ai-error` | 2248, 2443 |
| `.pe-ai-generate` | 2294, 2309, 2310, 2775, 2778, 2779, 2780, 2839 |
| `.pe-ai-menu` | 2852, 2862, 2863, 2876, 2877, 2878, 2879, 2896 |
| `.pe-ai-mode` | 2840, 2851 |
| `.pe-ai-model` | 2258, 2262, 2268, 2279, 2440, 2441, 2442 |
| `.pe-ai-note` | 2852, 2880, 2881, 2893, 2894, 2896 |
| `.pe-ai-split` | 2772, 2779, 2838, 2839, 2895, 2896 |
| `.pe-attachments` | 2231 |
| `.pe-body` | 2199, 2210 |
| `.pe-empty` | 2159 |
| `.pe-empty-state` | 2068, 2069, 2070, 2071, 2072 |
| `.pe-foot` | 2211, 2218, 2230 |
| `.pe-item` | 2093, 2108, 2109, 2130, 2972, 2973, 2974, 2975, 2984 |
| `.pe-item-actions` | 1970, 2110, 2116, 2129, 2130, 2131, 2132, 2133, 2134, 2135, 2136, 2988, 2989 |
| `.pe-item-delete` | 2131, 2132, 2137, 2138 |
| `.pe-item-dots` | 2146, 2154 |
| `.pe-item-title` | 2139 |
| `.pe-list` | 2085, 2971, 2973, 2974, 2975, 2984, 2988, 2989 |
| `.pe-main` | 2062, 2153, 2164 |
| `.pe-tag` | 2184, 2195, 2196, 3263, 3266 |
| `.pe-tag-text` | 3251 |
| `.pe-tags` | 2183, 2197, 2198 |
| `.pe-title` | 2172, 2182 |
| `.pending` | 443, 1270, 1271, 1338 |
| `.picker-chevron` | 181, 190, 191, 1630, 1631, 1632, 1633, 1724, 1725, 1842, 2268, 2442, 2904 |
| `.plan-card` | 2916, 2919, 2920, 2921, 2923, 2924 |
| `.plan-count` | 2918 |
| `.plan-head` | 2917 |
| `.plan-mark` | 2922, 2923, 2924 |
| `.pp-edit` | 1970, 2017, 2030, 2031 |
| `.pp-empty` | 2032 |
| `.pp-files` | 2042, 2043, 2044 |
| `.pp-foot` | 2033, 2034, 2035 |
| `.pp-label` | 2001 |
| `.pp-left` | 1996, 1997 |
| `.pp-list` | 1997, 2000 |
| `.pp-meta` | 2014 |
| `.pp-preview` | 2036, 2048 |
| `.pp-preview-title` | 2037 |
| `.pp-row` | 2002, 2011, 2013, 2015, 2016, 2030 |
| `.pp-search` | 1998, 1999 |
| `.pp-tags` | 2038, 2039 |
| `.pp-text` | 2040, 2041 |
| `.pp-title` | 2012, 2013 |
| `.prompt-added-by` | 2149, 2150 |
| `.prompt-editor` | 2054 |
| `.prompt-palette` | 1981, 2047 |
| `.prompt-side` | 2079 |
| `.published` | 4177 |
| `.published-actions` | 4193, 4194, 4195 |
| `.published-count` | 4179, 4180 |
| `.published-error` | 4181, 4182, 4183 |
| `.published-heading` | 4189 |
| `.published-list` | 4184 |
| `.published-meta` | 4191 |
| `.published-note` | 4178 |
| `.published-row` | 4185, 4186, 4187 |
| `.published-row-error` | 4192 |
| `.published-tag` | 3895 |
| `.published-text` | 4187, 4188 |
| `.published-title` | 4190 |
| `.rail-divider` | 219, 2354, 2430 |
| `.rail-setup` | 2358, 2431 |
| `.reasoning-body` | 1252 |
| `.reasoning-icon-slot` | 1238, 1247, 1251 |
| `.reasoning-row` | 1079, 1215, 1219, 1236, 1237, 1251, 1266, 1267, 1268, 1270 |
| `.reasoning-text` | 336, 505, 509, 1105, 1205, 1214 |
| `.resolution-picker` | 3809, 3810 |
| `.resolution-picker-head` | 3835, 3843 |
| `.resolution-picker-help` | 3883, 3890, 3891 |
| `.resolution-picker-menu` | 1722, 2808, 3813, 3831, 3844, 3853 |
| `.resolution-picker-ticks` | 3861, 3862, 3875, 3876, 3877, 3878, 3879, 3880 |
| `.resolution-picker-trigger` | 1607, 1624, 1625, 1631, 1633, 1725, 1848, 2772, 2775, 2776, 2831, 2834, 2912, 3811, 3812 |
| `.resolution-tag` | 3894 |
| `.round-switch` | 3187, 3196, 3208, 3209 |
| `.row-icon` | 3721, 3735, 3736 |
| `.s` | 396, 1143, 1152 |
| `.s-card` | 2561, 2816 |
| `.s-count` | 2665 |
| `.s-empty` | 2615, 2616 |
| `.s-filter` | 2650, 2661, 2773 |
| `.s-grip` | 1970, 2697, 2698, 2699, 2708 |
| `.s-input` | 2572, 2583, 2584, 2644, 2773 |
| `.s-link` | 2662, 2663, 2664 |
| `.s-model` | 2667, 2676, 2677, 2698, 2703, 2704, 2705, 2706, 2707, 2708, 2724 |
| `.s-model-caps` | 2677, 2684, 2725 |
| `.s-model-id` | 2677, 2683, 2725 |
| `.s-model-name` | 2677, 2682 |
| `.s-models` | 2666, 2702, 2703, 2704, 2705, 2706, 2707 |
| `.s-models-intro` | 2648 |
| `.s-models-toolbar` | 2649 |
| `.s-recheck` | 2540, 2554 |
| `.s-row` | 2562, 2567, 2589, 2590, 2591, 2721, 2813, 4087 |
| `.s-row-control` | 2560, 2571, 2589, 2590, 2591, 2711, 2712, 2722 |
| `.s-row-help` | 2570, 2815 |
| `.s-row-text` | 2568, 2590 |
| `.s-row-title` | 2569, 2814 |
| `.s-section` | 2556 |
| `.s-section-action` | 3764, 3772 |
| `.s-section-head` | 2557, 2558, 2817 |
| `.s-signin-code` | 2642 |
| `.s-signin-error` | 2645 |
| `.s-signin-form` | 2643, 2644 |
| `.s-signin-link` | 2619, 2620, 2629 |
| `.s-signin-start` | 2643 |
| `.s-star` | 2678, 2679, 2680, 2681 |
| `.s-tag` | 2685 |
| `.s-value` | 2585, 2589, 2590, 2591, 2592, 2723 |
| `.selection-chip` | 3264, 3267, 3407, 3422, 3433, 3446 |
| `.selection-icon` | 3423, 3430 |
| `.selection-label` | 3252, 3431 |
| `.selection-size` | 3253, 3432 |
| `.send` | 1817, 1853, 1858, 1861, 1867 |
| `.settings-body` | 2457, 2718 |
| `.settings-close` | 2526 |
| `.settings-error` | 571, 1026, 1034, 2535 |
| `.settings-main` | 2525 |
| `.settings-nav` | 2458, 2719 |
| `.settings-nav-item` | 2470, 2487, 2488, 2489, 2490, 2491, 2492, 2493, 2494, 2495, 3569 |
| `.settings-nav-label` | 2469 |
| `.settings-nav-title` | 2468 |
| `.settings-pane` | 2527, 2720 |
| `.settings-panel` | 244, 254, 530, 547, 553, 562, 563, 568, 579, 1436, 1444, 1452, 1453, 1462, 1463, 1541, 1552, 1592, 2369, 2450, 2526, 2535, 2536, 2572, 2583, 2584, 2644, 2650, 2661, 2717, 2773 |
| `.setup` | 3513, 3524, 3526, 3550, 3554, 3555 |
| `.setup-bar` | 3529, 3530 |
| `.setup-count` | 3531 |
| `.setup-dash` | 3547 |
| `.setup-dot` | 3546 |
| `.setup-failed` | 3548, 3549 |
| `.setup-head` | 3525 |
| `.setup-label` | 3535, 3537, 3539, 3540 |
| `.setup-mark` | 3534, 3540 |
| `.setup-note` | 3542, 3543, 3544, 3545 |
| `.setup-optional` | 3541 |
| `.setup-overall` | 3528 |
| `.setup-step` | 3533, 3537, 3538, 3539, 3540, 3543, 3558 |
| `.setup-steps` | 3532, 3557 |
| `.setup-sub` | 3527, 3556 |
| `.setup-value` | 3536, 3538, 3540 |
| `.share-actions` | 4157, 4158, 4159 |
| `.share-anchor` | 4133 |
| `.share-error` | 4156 |
| `.share-panel` | 4135 |
| `.share-step` | 4155 |
| `.share-text` | 4153, 4154 |
| `.share-url` | 4160, 4172 |
| `.side-add` | 3075 |
| `.side-empty` | 3699 |
| `.side-filter` | 3120, 3121 |
| `.side-filter-button` | 3122, 3135, 3136, 3137 |
| `.side-find` | 3607, 3622, 3632, 3642, 3643 |
| `.side-find-field` | 3623 |
| `.side-search` | 3079, 3090, 3107, 3119 |
| `.side-search-icon` | 3092, 3105, 3106 |
| `.side-section` | 3047, 3089, 3090, 3106, 3119, 3121 |
| `.side-tags` | 3073, 3074 |
| `.side-title` | 3054, 3070, 3071, 3072, 3089, 3091, 3704 |
| `.sidebar` | 70, 87, 139, 822, 903, 1489, 1498, 1505 |
| `.sidebar-edge` | 883, 888, 890, 899, 901, 902, 1504 |
| `.sidebar-foot` | 108, 880, 1498, 1503, 1519 |
| `.sidebar-reveal` | 883, 889, 890, 900, 901, 1504 |
| `.sidebar-toggle` | 1502 |
| `.steps` | 1085 |
| `.stop` | 1818, 1868, 1869, 1874 |
| `.strata` | 79, 803, 808, 818, 819, 1522, 1523 |
| `.sub` | 2499, 2500 |
| `.switch-dot` | 3010 |
| `.tag-menu` | 3138, 3154, 3155, 3171, 3172, 3173, 3174, 3648, 3650, 3651, 3652, 3653, 3654, 3655, 3657 |
| `.text-link` | 1591, 3562, 3565, 3934, 3946 |
| `.theme-picker` | 1553, 1562, 1576, 1577, 2712, 2772, 2781, 2909 |
| `.ticker` | 438, 1326 |
| `.ticker-icon-slot` | 1344 |
| `.title` | 125, 155, 921, 1538, 3690, 3718 |
| `.title-model-controls` | 1581, 1587, 1591, 1598, 1679, 1680, 2397, 2710, 2711, 2913 |
| `.tk-c` | 413 |
| `.tk-f` | 414 |
| `.tk-k` | 410 |
| `.tk-n` | 412 |
| `.tk-s` | 411 |
| `.toggle` | 2502, 2513, 2521, 2522, 2523, 2821, 2822, 2823 |
| `.tool-actions` | 1171 |
| `.tool-copy` | 1136, 1142 |
| `.tool-failure` | 1181 |
| `.tool-history` | 1080, 1278, 1279, 1296, 1297, 1299 |
| `.tool-history-body` | 1300 |
| `.tool-kind-icon` | 1125, 1135 |
| `.tool-meta` | 1164, 1170 |
| `.track` | 2932 |
| `.user-block` | 338, 347, 352, 360, 980, 981, 990, 992, 994, 1007, 1008, 1970 |
| `.ver` | 2498 |
| `.viewer` | 83, 586, 592, 1473, 1521 |
| `.vlogo` | 165, 2494, 3275, 3652, 3675 |
| `.wordmark` | 98, 834, 844, 1500, 1533 |
| `.working-dots` | 1350, 1351, 1359, 1360, 1366 |

## Part 2: the viewer's own interface

### 2.1 What the pane is, and what is not part of it

The 3D pane is an `<iframe class="bluemap-frame">` (web/app.js:3339-3358, `title="Live Minecraft world in BlueMap"`) showing BlueMap 5.22's own web app (server/bluemap.js:39-40, `VERSION = '5.22'`), baked with `scripts: ["/bluemap-bridge.js"]` (server/bluemap.js:130). Everything mcbench adds inside the frame is in web/bluemap-bridge.js (the "bridge"), web/bluemap-view.mjs and web/entities.mjs. Everything around the frame (cover, error banner, live controls, full-window layout) is the `BlueMapViewer` and `LiveControls` components in web/app.js:3069-3393. The world it shows is driven by web/live-world.mjs (`LiveWorld`) through the page.

web/viewer.js is not the pane. It is the headless renderer behind `see_build` (web/viewer.js:400-403, driven by server/shot.js through web/shot.html: `reset -> applyDelta -> flush -> setCamera -> renderNow`). It takes no user input; its camera is set only by `setCamera({ yaw, pitch, focus, focusDist, fov | ortho })` (web/viewer.js:1524-1530), defaults `yaw 2.4`, `pitch 0.5`, `focus [0.5, 0.5, 0.5]`, `focusDist 24` (web/viewer.js:1107-1110), lens `fov 70`, far plane `MAX_CAMERA_DISTANCE = 12000` (web/viewer.js:92, 1061-1066). Its only text that can surface is `console.error('Error rendering block <name>')` (web/viewer.js:667) and thrown errors such as `WebGL2 is not available` (web/viewer.js:408) and `Chunk <cx>,<cy>,<cz> is out of the viewer's range` (web/viewer.js:395). It is out of scope for the pane below, and tests test/viewer-reset.test.js, test/viewer-entities.test.js and test/viewer-block-entities.test.js pin it, not the pane.

Many camera behaviours are BlueMap's own code, not this repository's. They were read from BlueMap 5.22's sources as embedded in the baked web app's source map (the baked BlueMap web app's `web/assets/index-BxD3mV_W.js.map`; the same file name, which carries Vite's content hash, is in the test-data backup at `mcbench-lite-data-backup-2026-11-14-before-filters/minecraft-live/bluemap/8d91567eef72f773/web/assets/`, so the citations can be checked there; sources `src/js/controls/...`, `src/js/BlueMapApp.js`, `src/js/PopupMarker.js`, `src/components/...`). They are cited below as `BlueMap:<path>:<line>`. The pane's language is BlueMap's browser-language choice; the strings quoted are BlueMap's `lang/en.conf`.

### 2.2 Layout, size, resize and filling the window

| Aspect | Behaviour | Source |
|---|---|---|
| Container | `<section class="viewer">` holding `BlueMapViewer`; hidden while the place is the prompt library (`hidden=${promptView}`) | web/app.js:7009-7036 |
| Size | iframe and `.bluemap-shell` are `width: 100%; height: 100%`, black background `#000`, `color-scheme: dark` | web/style.css:586-608, 1473-1476 |
| Resize by the user | The `.strata` divider between chat and viewer (`title="Drag to resize"`, `cursor: col-resize`). Dragging sets the chat width to `clamp(clientX - chat.left, 320, max(room - 360, 320))` px, saved to `localStorage['mcb-chat-width']` rounded. While dragging `body.dragging` sets `pointer-events: none` on the viewer iframe | web/app.js:7002-7007, 4199-4217; web/style.css:78-83 |
| Resize inside the frame | BlueMap's `handleContainerResize` on the frame window's `resize`: renderer size, pixel ratio `devicePixelRatio * superSampling`, camera aspect, redraw | BlueMap:src/js/MapViewer.js:131-132, 164-175 |
| Narrow window (`max-width: 980px`) | viewer becomes grid row 2 with `min-height: 38vh`, divider row 3 (horizontal 1 px line), chat row 4 | web/style.css:1521-1523 |
| Fill the window | `.viewer.full` is `position: fixed; inset: 0; z-index: 15` | web/style.css:592-596 |
| Fill toggles | (1) the panel's full button in the frame, (2) a double-click on the map canvas in any mode, (3) a double-click on the `<section>` outside the iframe. Escape (passed out of the frame, or pressed in the page) closes it as the `viewer` layer | web/bluemap-bridge.js:135-142, 951-955; web/app.js:7013, 4122-4143 |
| Section tooltip | `Double-click to expand`, or `Double-click or press Esc to exit fullscreen` when full. It sits on the parent section, so it shows only over non-iframe parts (cover, banner, live controls) | web/app.js:7012 |
| Full button tooltip | `Fill the window`, or `Back beside the chat (Esc)` when full; `aria-label` equals the title; glyph expand or collapse | web/bluemap-bridge.js:5-6, 83-85 |
| Layer order | `viewer` is a non-modal layer, below command, lightbox, settings, drawer; above share. Escape closes only the topmost open layer | web/layers.mjs:6-19 |
| Typing while full | Type-ahead is off when full (`typeAhead=${typeAhead && !viewerFull}`), so printable keys stay in the frame | web/app.js:7029, 6114-6118 |

The page's word "fullscreen" in the section tooltip means filling the browser window, not the browser's Fullscreen API. BlueMap's own menu item `Go Fullscreen` and its spectator click (2.12) do call the Fullscreen API (see 2.20).

### 2.3 Every text and button the pane can show around the world

These are drawn by web/app.js over or instead of the iframe. `live` is the `LiveWorld` snapshot (web/live-world.mjs:190-222).

| Where | When | Text (exact) | Button | Source |
|---|---|---|---|---|
| Placeholder (no iframe yet) | `live.viewerUrl` never seen | first non-empty of `live.viewerError`, `live.syncError`, `live.error`, then `Preparing the live world…` if a view key is active, else `emptyText` | `Try again` (restart) when the sync failed with no world on show | web/app.js:3308-3315, 3305-3307 |
| `emptyText` | no view key | Build mode: `The world shows here once the app is ready.` while the app prepares, else `Open a chat to view its world.`; Bench mode: `null` (empty) | none | web/app.js:7023 |
| Cover (`.bluemap-loading`, `role="status"`) | the frame has not confirmed this chat's world, or there is an error | the boot error, or the view error for this key, else `emptyText` (no view key), `Placing every build…` (a bench round, key starts `bench:`), `Preparing this chat’s world…` (U+2019 apostrophe) | `Reload the viewer` (boot error: reloads the iframe document) or `Try again` (view error: resend the view afresh) | web/app.js:3359-3365, 3320-3324, 3335 |
| Banner (`.bluemap-mismatch`, `role="status"`, bottom, inset 16 px) | `live.syncError || live.error` non-empty, shown over the world as well as over the cover | that text | `Try again` when `syncFailed && !live.live && conversationId === activeId` | web/app.js:3366-3368; web/style.css:637-651 |
| Live controls toolbar | world shown and painted (`!covered`) and `live.live` non-null | `<n> ticks` | Play/Pause, Step, Restart (2.6) | web/app.js:3369, 3377-3393 |
| Notices (page toasts) | any error from a viewer action passed to `onError` / `showError` (interact, export, player input, player leave, control actions, selection) | the error's message | page's own | web/app.js:3749-3754, 3204-3221, 7025 |

Both placeholder and cover show a `build` icon at size 20 before the text (web/app.js:3311, 3361). `aria-busy` on `.bluemap-shell` is true until the frame has painted this exact chat and revision (web/app.js:3327-3328, 3338).

Error texts that reach these places, with their origin:

| Text | Origin |
|---|---|
| `BlueMap viewer did not become ready` | boot timeout after `20000` ms (web/bluemap-bridge.js:31-44) |
| any message the tile sync's `refresh` or `patch` rejects with (test example `The mesher stopped: out of memory`) | web/bluemap-bridge.js:375-380; test/bluemap-bridge.test.js:419-446 |
| `The live world stopped: <engine message>` (example `The live world stopped: The engine failed: unported: a block`) | engine stopped the world, first reason kept (web/live-world.mjs:598-606) |
| `The world placed again differs from the one shown while it was placed` | kept placement did not match (web/live-world.mjs:563-571) |
| `HTTP <status>` or the server's JSON `error` | a failed status poll or import (web/live-host.mjs:64-73) |
| the engine start's error (test examples `The module is unavailable`, `<address>blocks.txt is unavailable (HTTP 404)`) | web/live-world.mjs:385-393; test/live-world.test.js:268-286 |
| `live.viewerError` | the server's assets status (`viewerError`) |
| `live.syncError` | the server's view status (`view.syncError`) |
| `There is no live world on show` | a control, export, step or interaction on a world with no confirmed base (web/live-world.mjs:609-612), surfaces as a notice or as a player-mode note |
| `The live world cannot step while stopped` | step on a stopped world (web/live-world.mjs:742), unreachable from the UI because Step is disabled |
| `A selection needs integer corners within +/-4096 with min at most max` | server, a selection outside the coordinate limit (server/selection.js:31-34; test/selection.test.js:40-49), notice |

### 2.4 The frame's boot and view link

The bridge's boot (web/bluemap-bridge.js:248-302) waits for `window.bluemap.mapViewer.map.isLoaded` and `window.BlueMap`, polled every `16` ms, giving up after `20000` ms. It then loads the tile sync (`/bluemap-tiles.mjs`, a `MesherClient` pool sized from `navigator.hardwareConcurrency`), `bluemap-view.mjs` (`patchViewer`), installs the flight modes, the entity layer and the overlay group.

| Boot state | Goes to | What the page is told | Source |
|---|---|---|---|
| `booting` | `ready`, `failed` | nothing | web/bluemap-bridge.js:257-262 |
| `ready` | none | `mcbench-lite:bluemap-ready`, once, and again on every `mcbench-lite:ping` | web/bluemap-bridge.js:271-274 |
| `failed` | none | `mcbench-lite:boot-failed` with `error`, once and on every ping; the frame shows nothing until the iframe document is reloaded | web/bluemap-bridge.js:264-274 |

The page pings on every iframe `load`, resets its painted state, and on ready sends the view (forced) and the UI state (web/app.js:3344-3357, 3175-3179). One BlueMap document serves every chat; only tiles and camera change (comment: rebooting per chat cost a third of a second, web/app.js:3298-3302).

| View link state | Goes to | Meaning | Source |
|---|---|---|---|
| `none` | `queued` | no view asked | web/bluemap-bridge.js:317-325 |
| `queued` | `queued`, `loading` | a view waits for the loader | same |
| `loading` | `queued`, `shown`, `failed` | its base loads | same |
| `shown` | `queued`, `patching` | base and every patch so far applied; page told `mcbench-lite:refreshed` with the key | web/bluemap-bridge.js:362-365 |
| `patching` | `queued`, `shown`, `failed` | ticks since are applied, all at once | web/bluemap-bridge.js:366-373 |
| `failed` | `queued` | load or patch failed; page told `mcbench-lite:refresh-error` once; later ticks of that view dropped; the same view asked again loads tiles afresh | web/bluemap-bridge.js:375-380, 337-341 |

The page resends an unconfirmed view every `1000` ms until the frame answers for its key (web/app.js:3253-3259). A view for another chat carries `frame` (the build's box) and `fresh: true`: the camera is framed at once (`fly(min, max, 0)`) and tiles stream fresh (web/app.js:3139-3151; web/bluemap-bridge.js:359-361). A view of the same chat (a new revision) swaps in one step, the old world staying in view meanwhile (web/app.js:3329-3333). A refresh clears the entities at once (web/bluemap-bridge.js:231-234).

### 2.5 The live world's states as the user sees them

`LiveWorld` keeps three machines (web/live-world.mjs:59-98) and reports one snapshot (web/live-world.mjs:190-222).

| Machine | States and moves | Source |
|---|---|---|
| Sync phase | `idle` (no view); `waiting` → `syncing`; `syncing` → `waiting` or `failed`; `failed` → `syncing` or `waiting`. A failed version is not retried until the server has another version or a restart is asked | web/live-world.mjs:59-71, 345-367 |
| Running | `playing` ↔ `paused`, either → `stopped`; `stopped` goes nowhere except by a fresh placement (which sets `playing`) | web/live-world.mjs:73-83, 448-452 |
| Player | `none` → `spawning`; `spawning` → `standing` or `none`; `standing` → `standing` or `none` | web/live-world.mjs:85-98 |

Snapshot fields the pane reads, and how each user-visible state maps:

| User-visible state | `status` | Other fields | What the pane shows |
|---|---|---|---|
| No view | `idle` | `conversationId null` | placeholder or cover with `emptyText` |
| Loading (no base yet) | `starting` | `viewerRevision null` | cover `Preparing this chat’s world…` / `Placing every build…` |
| Placing / settling | `syncing` | `phase: 'placing'`, base may be provisional (`live: null`) | old world of the same chat stays shown; a provisional base (kept placement) is shown with no live controls and takes no clicks (test/live-world.test.js:327-373, 416-452) |
| Running | `running` | `live: { playing: true, stopped: false, ticks }` | world, live controls with Pause |
| Paused | `running` | `live.playing false` | live controls with Play and Step enabled |
| Empty build | `running` | `live: null` | empty world, no live controls, no export, no player mode (test/live-world.test.js:311-325) |
| Error (sync failed) | `error` | `error` text, `syncFailed: true`, `live` null on a first sync | banner (and placeholder) with `Try again` only if `live` is null |
| Error (engine stopped) | `error` | `error: 'The live world stopped: …'`, `live.stopped true`, `live.playing false` | banner; live controls with Play and Step disabled, Restart enabled |
| Error (poll failed) | `error` | `error` = poll error | banner |

`frame` (the server's turn counter) advancing after the new revision painted flies the camera to the whole world (web/app.js:3284-3296). The tick count is emitted at most every `250` ms with a trailing emit so the last count is always drawn (web/live-world.mjs:55, 230-242). Ticks reset to `0` on any fresh placement (restart or another build) (web/live-world.mjs:478).

### 2.6 Play, pause, step, restart and the tick counter

`LiveControls` (web/app.js:3377-3393), a toolbar `role="toolbar" aria-label="Live world"`, absolutely placed `left: 12px; bottom: 12px`, buttons `28×28` px, icons size `14` (web/style.css:609-635).

| Control | Title (tooltip) | Enabled | Action | Source |
|---|---|---|---|---|
| Play/Pause | `Pause` while playing, `Play` otherwise; icon `pause` / `play` | disabled when `stopped` | `world.control('pause' | 'play')`: state flips at once, then engine `run(bool)` | web/app.js:3381-3383; web/live-world.mjs:736-740 |
| Step | `Step one tick`; icon `step` | disabled when playing or stopped | `world.control('step')`: engine runs one tick | web/app.js:3384-3386; web/live-world.mjs:741-743 |
| Restart | `Restart from the build`; icon `restart` | always | `world.control('restart')`: whole world placed afresh after any sync in flight; a paused world stays paused; a standing player is respawned where it stood with the keys it held | web/app.js:3387-3389; web/live-world.mjs:726-734, 507-526 |
| Tick counter | `Ticks since the build was placed` | n/a | text `${ticks.toLocaleString()} ticks`, tabular numerals, 12 px | web/app.js:3390 |

Disabled buttons have `opacity: 0.4` (web/style.css:634). There are no keyboard shortcuts for these. Failures go to notices.

### 2.7 Buttons inside the frame

BlueMap's own control bar is kept (BlueMap:src/components/ControlBar/ControlBar.vue). The bridge appends `#mcbench-panel` (`aria-label="mcbench world controls"`) at its end, retrying every `100` ms for up to `20000` ms until `.control-bar` exists (web/bluemap-bridge.js:94-158). Panel buttons are BlueMap-sized (`height: 2em`), separated by a 1 px `var(--theme-bg-light)` left border between visible ones (web/bluemap-bridge.js:51-69).

| Button | Where | Visible when | Title / label (exact) | Action | Source |
|---|---|---|---|---|---|
| Menu | BlueMap bar, first | always | `Menu` | opens BlueMap's side menu (2.13a) | ControlBar.vue |
| Day/Night | BlueMap bar | map loaded | `Day/Night` | BlueMap lighting switch | ControlBar.vue; en.conf |
| Perspective | BlueMap controls switch | always (map has perspective view) | `Perspective-View` | `setPerspectiveView(500, free ? 100 : 0)`; from player mode gives back the saved map view (2.10) | BlueMap:src/components/ControlBar/ControlsSwitch.vue; web/bluemap-bridge.js:617-637 |
| Flat | BlueMap controls switch | always | `Orthographic / Flat-View` | `setFlatView(500, free ? 100 : 0)` | same |
| Player mode (BlueMap's Free-Flight button, retitled) | controls switch, last BlueMap button | always | `Player mode: walk and fly as a Minecraft player` | chooses player mode, then BlueMap's `setFreeFlight(500)` | web/bluemap-bridge.js:566, 648-665 |
| Spectator (eye) | inserted right after the player button, `#mcbench-spectate` | always | `Spectator: fly through everything` | `flightMode = 'spectator'; app.setFreeFlight(500)` | web/bluemap-bridge.js:567-568, 639-647 |
| Reset camera | BlueMap bar | map loaded | `Reset Camera & Position` | BlueMap `resetCamera()`: target `(startPos.x, 0, startPos.z)` = `(0, 0, 0)`, distance `1500`, angle `0` (straight down), rotation `0`, perspective | BlueMap:src/js/BlueMapApp.js:271-298; server/bluemap.js:97 |
| Position input | BlueMap bar | map loaded | x / z number inputs | BlueMap moves the target | ControlBar.vue |
| Compass | BlueMap bar | map loaded | `Compass / Face North` | animates rotation to 0 over `300` ms, easeOutQuad | BlueMap:src/components/ControlBar/Compass.vue:32-41 |
| Select blocks | mcbench panel | `uiState.selectable` (a Build chat's own world, not a bench) | title `Select blocks to refer to in your message (Esc to stop, right-click a box to remove it)`, `aria-label="Select blocks"`, `aria-pressed` and class `active` while selecting | toggles selecting | web/bluemap-bridge.js:104-112, 86-88; web/app.js:3114 |
| Download | mcbench panel (`<a download>`) | a chat (not a bench round) whose world has blocks | `Download as a WorldEdit schematic (.schem)` | link `/api/conversations/<id>/schematic` | web/bluemap-bridge.js:114-121; web/app.js:3112 |
| Export | mcbench panel | world on show with `live` non-null | `Export as a Minecraft world (a zip to unpack in .minecraft/saves)` | posts `mcbench-lite:export`; page builds the save and downloads `<exportName>.zip` (`application/zip`), the chat title, `<bench title or 'Bench'> round <n>`, or `mcbench world` | web/bluemap-bridge.js:123-133; web/app.js:3214-3222, 3756-3764 |
| Fill / back | mcbench panel, last | always | 2.2 | posts `mcbench-lite:full` with `on: !full` | web/bluemap-bridge.js:135-142 |
| Zoom + / − | BlueMap, fixed bottom | BlueMap setting `Show Zoom Buttons` on (default `true`) and not in free flight | none | adds `-3` / `+3` to the wheel zoom's pending delta | BlueMap:src/components/Controls/ZoomButtons.vue; BlueMap:src/App.vue:4; BlueMapApp.js:97 |
| Free-flight touch arrows | BlueMap, `#ff-mobile-controls` | state `free` (player or spectator), enabled after the first touch | none | forward/back moves `0.02` blocks per ms along the heading, up/down `0.01` per ms | BlueMap:src/components/Controls/FreeFlightMobileControls.vue |

While spectating, BlueMap marks its free-flight (player) button active too; the bridge restyles it to look inactive (`.mcbench-spectating>.svg-button.active[data-mcbench-player]`) and sets the eye `active`, checked every `200` ms (web/bluemap-bridge.js:58, 654-667).

#### 2.7a BlueMap's side menu

Root: `Maps`, `Markers`, `Settings`, `Info`, then `Go Fullscreen`, `Reset Camera`, `Take Screenshot` (downloads `bluemap-screenshot.png`, also copies to clipboard by default), `Update Map` (`Clear Tile Cache`) (BlueMap:src/components/Menu/MainMenu.vue:8-18). Settings: `View / Controls` (`Perspective`, `Flat`, `Free-Flight`), `Lighting` (`Sunlight`, `Ambient-Light` sliders 0..1 step 0.01), `Resolution` (`High (SSAA x2)`, `Normal (Native x1)`, `Low (Upscaling x0.5)`), `Render Distance` (`Hires layer`, `Lowres layer`, `Load hires while moving`), `Map Controls` (`Show Zoom Buttons`), `Free-Flight Controls` (`Mouse-Sensitivity` 0.1..5 step 0.05, default 1; `Invert Mouse Y`), `Theme`, `Screenshot` (`Copy to Clipboard`), `Language` (BlueMap:src/components/Menu/SettingsMenu.vue). BlueMap saves these with cookies/local storage (`use-cookies: true`, server/bluemap.js:126). The bridge forces the hires distance to `1500` on every load (web/bluemap-view.mjs:7, 25-34), so the `Hires layer` slider has no effect (slider range from settings.json: `hiresSliderMin 0`, `hiresSliderMax 500`, `hiresSliderDefault 100`). The menu's `Free-Flight` entry calls `setFreeFlight(500)` and so enters whichever of player or spectator was chosen last (see 2.20).

### 2.8 Camera: map controls (perspective and flat)

All of this is BlueMap's `MapControls` (BlueMap:src/js/controls/map/MapControls.js). The camera is an orbit: a target `position`, `distance`, `rotation` (heading: looks along `(sin r, 0, -cos r)`), `angle` (tilt from straight down, `0` = straight down, `π/2` = level), `ortho` (`0` perspective, `1` flat). Every input adds to a pending delta that is applied with exponential smoothing each frame: `smoothing = clamp(stiffness / (16.666 / delta_ms), 0, 1)`, the delta then decays by `(1 - smoothing)` and stops below `0.0001`. This is the inertia.

| Input | Effect | Speed, stiffness | Source |
|---|---|---|---|
| Left drag (no Alt) | pan the target across the ground, rotated by heading; scaled by `distance` and by `1/clientHeight` | speed `1.5`, stiffness `0.3` | MouseMoveControls.js:105-107; MapControls.js:70 |
| Right drag, horizontal | rotate | speed `6` × px/`clientWidth`, stiffness `0.3` | MouseRotateControls.js:97-100; MapControls.js:71 |
| Right drag, vertical | tilt: dragging down lowers `angle` toward straight down, dragging up raises it toward level (`deltaAngle -= dy`); below distance 1000 the distance also eases toward the largest allowed for the new angle ("dynamic distance") | speed `3` × px/`clientHeight`, stiffness `0.3` | MouseAngleControls.js:114-120; MapControls.js:72 |
| Ctrl+left drag or Alt+left drag | rotate and tilt as right drag; Ctrl+left also pans (pan only excludes Alt) | as above | same files |
| Wheel | zoom: `distance *= 1.5^(delta × smoothing × 1)`, wheel pixels × `0.01`, lines × `0.33`; down zooms out | speed `1`, stiffness `0.2` | MouseZoomControls.js; MapControls.js:73 |
| Wheel during a right-drag tilt | turns off "dynamic distance" for that drag | | MouseAngleControls.js onWheel |
| `KeyW` / `ArrowUp`, `KeyS` / `ArrowDown`, `KeyA` / `ArrowLeft`, `KeyD` / `ArrowRight` (no modifiers) | pan forward / back / left / right, scaled by `distance` | speed `0.025`, stiffness `0.2` | map/keyboard/KeyMoveControls.js:32-49; MapControls.js:75 |
| `Alt+ArrowUp`, `Alt+KeyW`, `PageUp` / `Alt+ArrowDown`, `Alt+KeyS`, `PageDown` | tilt up / down | speed `0.04`, stiffness `0.15` | KeyAngleControls.js:33-44; MapControls.js:77 |
| `Alt+ArrowLeft`, `Alt+KeyA`, `Delete` / `Alt+ArrowRight`, `Alt+KeyD`, `End` | rotate left / right | speed `0.06`, stiffness `0.15` | KeyRotateControls.js:31-42; MapControls.js:76 |
| `NumpadAdd`, `Insert` / `NumpadSubtract`, `Home` | zoom in / out | speed `0.2`, stiffness `0.15` | KeyZoomControls.js:32-41; MapControls.js:78 |
| One-finger touch drag | pan | speed `1.5`, stiffness `0.3` | TouchMoveControls.js; MapControls.js:80 |
| Two-finger vertical drag | tilt | speed `3`, stiffness `0.3` | TouchAngleControls.js; MapControls.js:82 |
| Two-finger twist | rotate, `0.0174533` per degree | stiffness `0.3` | TouchRotateControls.js; MapControls.js:81 |
| Pinch | zoom, `distance /= scale ratio`, no smoothing | | TouchZoomControls.js; MapControls.js:83 |
| Click or one-finger tap (hammer `Tap`, threshold `5` px) | BlueMap map interaction: the block popup (2.13), and the bridge's block click (2.13) | | MapControls.js:223-279 |
| Double click or double tap (second tap within `500` ms and `5` px) | BlueMap zooms toward the hit point over `500` ms: `distance → max(distance × 0.25, 5)`, target x/z → hit; and the bridge toggles fill-the-window (2.17) | | MapControls.js:260-275; BlueMapApp.js:799-819; web/bluemap-bridge.js:951-955 |

A key combination matches only with exactly its modifiers (`KeyCombination.testDown` compares ctrl, shift and alt), so Shift+W does not move. Keys are released on window `blur`. The keyboard listeners are on the frame's `window`, so keys work only while the frame has focus.

Limits: `distance` soft-clamped to `[minZoomDistance 5, maxZoomDistance 100000]` with softness `0.8` (MapControls.js:90-91, 173; settings.json). `angle` soft-clamped to `[0, π/2]`, and further to `getMaxPerspectiveAngleForDistance(distance) = clamp((1 - sqrt(max(distance - 5, 0.001) × 0.0005)) × π/2, 0, π/2)`: the further out, the more top-down (MapControls.js:193, 211-214). In flat view tilt is ignored and rotation snaps to north when within `0.3` rad (`0.05` while rotating) (MapControls.js:183-186). The target's height follows the terrain under it plus `3`, eased with stiffness `0.1`, and lerps toward `y 0` as `distance / 500` grows to `1` (MapHeightControls.js). Inside the shown world's footprint the bridge raises "terrain" to at least the build's middle height minus `3`, so the target stays level with the build instead of sinking into the void between parts (web/bluemap-view.mjs:121-135). The render pass sees `distance` clamped to `799` so hires tiles always draw (web/bluemap-view.mjs:9-17, 46-58); camera input is unaffected.

BlueMap writes the camera to the frame's URL hash after `1500` ms without moves (`#live:x:y:z:distance:rotation:angle:tilt:ortho:state`) and reads it on load (BlueMapApp.js:699-793). The iframe's `src` is fixed, so a reload loses it (web/app.js:3349-3351).

### 2.9 How the camera frames a build

The single framing function is `fly(min, max, ms)` (web/bluemap-bridge.js:493-500), Minecraft coordinates, inclusive corners (the page adds `yOffset`, default `64`, to y).

| Trigger | ms | Source |
|---|---|---|
| Another chat's world loaded (view carries `frame`) | `0` (placed at once, inside the load turn) | web/bluemap-bridge.js:359-361; web/app.js:3141-3150 |
| A finished turn advanced `live.frame` and the new revision painted | `700` | web/app.js:3284-3296; web/bluemap-bridge.js:192-195 |
| `focus` prop changed: a bench row clicked for the shown round, a digit `1`–`9` typed in a comparison, a round's prompt clicked (whole round) | `700` | web/app.js:5220-5235, 6122-6124, 6381; 3276-3278 |

During a BlueMap move between modes (`cameraMode 'changing'`) the fly waits and runs when the move ends, so it never cancels BlueMap's move (web/bluemap-bridge.js:469-491; test/bluemap-bridge.test.js:471-494). In player mode it teleports the player instead (`teleportTo`). Otherwise `flyTo` (web/bluemap-bridge.js:511-554):

| Quantity | Value |
|---|---|
| centre | `((min + max + 1) / 2)` per axis |
| radius | `hypot(size_x, size_y, size_z) / 2` with `size = max - min + 1` |
| distance | `max(radius / sin(min(vFov, hFov) / 2) × 1.2, 12)`; `hFov = 2·atan(tan(vFov/2)·aspect)`; the `1.2` keeps labels at the edges in view |
| angle | current angle clamped to `[0.45, 1.3]` rad (BlueMap's straight-down start becomes `0.45`) |
| heading | unchanged |
| map controls | target x, z = centre; distance `/ (1 - centre.y · cos(angle) / 500)` to cancel the target's lowering |
| spectator | eye = centre − look × distance, look = heading tilted down by `π/2 − angle` |
| motion | `BlueMap.animate` over `ms` with `easeInOutQuad`; `ms 0` sets at once |
| cancels | the previous fly, and BlueMap's double-click zoom animation |
| cancelled by | any pointer down in the frame outside player mode (web/bluemap-bridge.js:1043) |

Test: a box `[-1,64,-2]..[14,66,2]` from BlueMap's start (`distance 1500, angle 0, rotation 0.3`) frames to `x 7`, `angle 0.45`, distance between 12 and 40, the same whether placed at once or flown (test/bluemap-bridge.test.js:560-581).

There is no "frame the build" button. BlueMap's `Reset Camera` goes to `(0, 0)` from `1500` straight down, not to the build.

### 2.10 Camera modes and switching between them

The bridge tracks `cameraMode` from what BlueMap gives its controls manager (web/bluemap-bridge.js:469-491, 576-599): `changing` (none: booting or moving between modes), `map` (perspective or flat), `player`, `spectator`.

| Switch | What happens | Source |
|---|---|---|
| Map → player (player button, or menu `Free-Flight` after player was last chosen) | no move down to eye level: BlueMap's animation cancelled, controls set to none (the map view `{state, position, distance, rotation, angle, tilt, ortho}` is saved), state `free`, controls = player. The camera keeps the map's view until the player stands | web/bluemap-bridge.js:600-613, 584-597; test/bluemap-bridge.test.js:503-531 |
| Player → player (button again) | nothing | web/bluemap-bridge.js:607 |
| Player → perspective | the saved map view is restored exactly, at once, with no BlueMap move | web/bluemap-bridge.js:614-637; test/bluemap-bridge.test.js:533-549 |
| Player → flat | saved view restored, then BlueMap moves to flat over `500` ms | same; test/bluemap-bridge.test.js:550-557 |
| Any → spectator | BlueMap's own `setFreeFlight(500)`: over `500` ms lerps y to terrain+3, distance to `0`, angle to `π/2` | web/bluemap-bridge.js:606; BlueMapApp.js:533-565 |
| Map ↔ map | BlueMap's own `500` ms easeInOutQuad moves | BlueMapApp.js:461-531 |
| Player ends on its own | `leavePlayerMode(note)` calls `setPerspectiveView(500)` (restores the map view) and shows the note | web/bluemap-bridge.js:811-815 |

### 2.11 Player mode

A creative Minecraft player held by the page's engine (core/src/player, the 1.21.10 client's movement ported: keys, double-tapped jump to fly, sprinting, sneaking at edges, fluids, climbing; core/src/player/mod.rs:1-14). Movement speeds, gravity, flight and collisions are the engine's, not the pane's. The pane sends keys and look and draws the camera from each tick's view.

Entry (web/bluemap-bridge.js:723-744): the player starts with yaw from the camera heading (`rotation·180/π − 180`) and pitch from the tilt (`90 − angle·180/π`, clamped to ±90), and asks the page to spawn it `beside` what the camera looks at. The page stands it (web/live-world.mjs:24-51, 665-685): the target is clamped into the build footprint; the player goes `0.5` block past the footprint edge on the camera's side, at the build's floor; the engine finds the nearest standable column top (`standingSpot`); it faces the footprint middle at mid-height; with no standable spot it flies at the edge (test/live-world-states.test.js:651-669; test/live-world.test.js:230-249). The camera then goes to its eyes.

Entry needs a running world. A UI update with `playable` false ends player mode with the note `Player mode needs a running world.` (web/bluemap-bridge.js:182). Entering when no confirmed world is on show makes the spawn fail with `There is no live world on show`, shown as the note.

| Input (player mode) | Effect | Source |
|---|---|---|
| Click on the canvas, mouse not taken | requests pointer lock; if the browser refuses (Chrome before the frame has focus) the hint stays and the next click takes it | web/bluemap-bridge.js:930-941 |
| Mouse move, pointer locked to the canvas | yaw `+= movementX × 0.15°`, pitch `+= movementY × 0.15°` clamped to `[-90, 90]`; sent on the next frame | web/bluemap-bridge.js:697, 921-926, 757-760 |
| Right click, locked | uses the block at the screen centre if its centre is within `REACH 5 + 0.87` of the eye; posts `mcbench-lite:interact` with the player's yaw | web/bluemap-bridge.js:698, 942-948 |
| Left click, locked | nothing (nothing is broken or placed) | web/bluemap-bridge.js:927-929, 942 |
| Context menu | suppressed in player mode | web/bluemap-bridge.js:950 |
| `KeyW` | forward, bit `1` | web/bluemap-bridge.js:696 |
| `KeyS` | backward, bit `2` | same |
| `KeyA` | left, bit `4` | same |
| `KeyD` | right, bit `8` | same |
| `Space` | jump, bit `16`; double-tap toggles flying (engine) | same; core/src/player/mod.rs:2 |
| `ShiftLeft`, `ShiftRight` | sneak (in flight: down), bit `32`; held while either is | same |
| `ControlLeft`, `ControlRight` | sprint, bit `64` | same |
| `Escape` | the browser frees the mouse; no message to the page while locked; a further Escape (unlocked) goes to the page | web/bluemap-bridge.js:1079-1086 |
| Frame loses focus (`blur`) | all keys released, input sent | web/bluemap-bridge.js:915-920 |

Key rules (web/bluemap-bridge.js:888-914, test/bluemap-bridge.test.js:585-622): a key down with Meta or Alt, or with focus in `input, textarea, select, [contenteditable]`, is not the player's; a held key's keyup always releases it, wherever it lands and whatever is held. Repeats and a second press of a held key send nothing. Each change sends `mcbench-lite:player-input { keys, taps, yaw, pitch }`, `taps` being keys newly pressed since the last send so a tap shorter than a tick still counts. Player keys are stopped (`stopImmediatePropagation`) from reaching BlueMap and the type-ahead.

Camera (web/bluemap-bridge.js:757-786, 817-886): until the first spawn answer the camera keeps the map's view. Then each frame: distance `0`, ortho `0`, eye position interpolated between the last two ticks by a smoothed clock (`TICK_MS 50`, re-synced if more than 150 ms off, else eased by `0.1`), plus eye height eased halfway per tick; rotation `(yaw + 180)°`, angle `(90 − pitch)°`, tilt `0`; view bob from walking (`walk += 0.6 × ground distance`, bob eased `0.4` toward `min(0.1, ground speed)` when on ground and not swimming); FOV `70 × eased modifier`, modifier `(flying ? 1.1 : 1) × ((sprinting ? 1.3 : 1) + 1) / 2`, eased halfway per tick within `[0.1, 1.5]`, times `0.85714287` with the eye in water or lava. The original FOV is restored on leaving. Spawn answers are counted: an answer superseded by a later spawn, and ticks before it, are ignored (test/bluemap-bridge.test.js:361-381).

Falling: when the player's y is below `VOID_Y -128`, the note `Fell out of the world` shows and it is respawned at the last spawn answer's position, with its current look and that spawn's flying (web/bluemap-bridge.js:700-703, 879-885; test/bluemap-bridge.test.js:523-526).

HUD (`#mcbench-player`, fixed over the frame, `z-index: 20`, `pointer-events: none`, font `12px/1.3 system-ui`; web/bluemap-bridge.js:59-67, 958-989):

| Element | Shown when | Content | Look |
|---|---|---|---|
| Crosshair | player and pointer locked | a `16×16` white cross of `2` px bars | centred, `mix-blend-mode: difference` |
| Hint | player | locked: `WASD move · Space jump (twice: fly) · Shift sneak · Ctrl sprint · Right-click use · Esc frees the mouse`; not locked: `Click to look around` | bottom `64px`, centred pill `rgba(12,12,12,.62)`, colour `#d8d3c8`, radius 7 |
| Note | a note was shown in the last `5000` ms (a new one replaces it and its timer) | the note text | top `58px`, colour `#ffd7a8` |

Notes, all texts: `Player mode needs a running world.`, `The player is no longer in the world.` (the world let the player go: engine gone or a failed fresh placement), `Fell out of the world`, `Another world came on show before the player stood in this one` (web/app.js:3198), and any spawn error message. A note outlives player mode, drawn without crosshair or hint (test/bluemap-bridge.test.js:624-652).

Exit: any change of controls away from the player (Perspective, Flat, Spectator, BlueMap reset camera) stops it: FOV restored, pointer lock released, `mcbench-lite:player-leave` posted; the page removes the player after any spawn still on its way (web/bluemap-bridge.js:745-756; web/app.js:3206-3209; test/bluemap-bridge.test.js:322-359). A restart or a new revision of the same build keeps player mode and the player where it stood (test/bluemap-bridge.test.js:383-402; test/live-world.test.js:199-228). Another build placed afresh (`fresh` refresh) ends player mode with no note (web/bluemap-bridge.js:229-230). A fly in player mode spawns the player flying at the framing eye position minus `1.62` (web/bluemap-bridge.js:670-685).

### 2.12 Spectator (BlueMap's own free flight)

BlueMap's `FreeFlightControls` (BlueMap:src/js/controls/freeflight/FreeFlightControls.js). It flies through everything; no engine player exists.

| Input | Effect | Values | Source |
|---|---|---|---|
| `KeyW`/`ArrowUp`, `KeyS`/`ArrowDown`, `KeyA`/`ArrowLeft`, `KeyD`/`ArrowRight` | move along heading | speed = `moveSpeed` (start `0.5`), stiffness `0.1` | FreeFlightControls.js:55; freeflight/keyboard/KeyMoveControls.js:32-49 |
| `Space`, `PageUp` / `ShiftLeft`, `ShiftRight`, `PageDown` | up / down | speed `moveSpeed`, stiffness `0.2` | KeyHeightControls.js:31-40 |
| Wheel | changes `moveSpeed *= 1.5^(-delta × 0.25)` within `[0.05, 5]` | | FreeFlightControls.js:183-194 |
| Left drag / right drag (not locked) | look, speed `1.5` left, `-2 × sensitivity` right | stiffness `0.5` | FreeFlightControls.js:57-58; BlueMapApp.js:443-450 |
| Click (moved ≤ `5` px) | `document.body.requestFullscreen()`, then pointer lock (`unadjustedMovement` first) | | FreeFlightControls.js:146-165 |
| Mouse move, locked | look, speed `-1.5 × sensitivity` | | same |
| One-finger touch drag | look | speed `5`, stiffness `0.15` | FreeFlightControls.js:59 |
| Context menu | suppressed | | FreeFlightControls.js |

Angle is clamped to `[0, π]`. BlueMap's block popup does not show in spectator (it needs the map controls' tap). The bridge's block click does work in spectator: it is suppressed only in player mode, so a left click (pointer down and up within 500 ms and 4 px) on a block posts `mcbench-lite:interact` with the camera heading's yaw (web/bluemap-bridge.js:1115-1125).

### 2.13 Clicking blocks

| Step | Behaviour | Source |
|---|---|---|
| Gesture | left pointer down on the canvas, not selecting, not player mode, boot done; pointer up with button 0 within `500` ms and `4` px | web/bluemap-bridge.js:1110-1125 |
| Hit | the viewer's own cast against hires tiles and markers (no lowres); the hit point moved `0.01` against the face normal, floored, gives the block; overlays let picks pass through | web/bluemap-bridge.js:1006-1016, 431; web/bluemap-view.mjs:72-114 |
| Sent | `mcbench-lite:interact { x, y, z, yaw }`, yaw = `rotation·180/π − 180` (camera heading) or the player's yaw; a fence gate opens away from the side faced | web/bluemap-bridge.js:718-722, 1124 |
| Page | `LiveWorld.input(x, y, z, yaw)`: only blocks whose state takes input are sent to the engine (`engine.click(x, y, z, Math.fround(yaw))`); others do nothing, silently; a provisional base takes no clicks | web/app.js:3210-3213; web/live-world.mjs:711-722 |
| Input kinds | `toggle` (lever), `press` (buttons), `use` (door, trapdoor, fence gate, repeater, comparator, note block, daylight detector, decorated pot, redstone dust); iron doors none; a dust cross whose four sides connect is left as is | web/live-world.mjs:711-718; web/core-world.mjs:20, 128-142; test/live-world.test.js:113-154; test/live-patch.test.js:24-40 |
| Feedback | none from the pane itself; the change arrives as the next tick's patch (lever flips, lamp lights). Errors go to notices | web/app.js:3212 |
| BlueMap popup (map modes only) | on the same click: a white `1.01` cube at opacity `0.5` on the hit block and a label `Block:` with `x:`, `y:`, `z:` values, fading in over `300` ms; clicking the label copies `"x y z"` (tooltip `Copy to Clipboard`); closes (fade `300` ms) on any mousedown, touchstart, keydown or mousewheel outside it | BlueMap:src/js/PopupMarker.js:43-73, 91-115, 243-273; en.conf |
| Test | a click on a floor lever at `[1,65,0]` posts exactly that block, and BlueMap's own tap hits it too; a click on the sky posts nothing; nothing throws | test/viewer-click.integration.test.js:18-131 |

### 2.14 The selection tool and the composer

| Step | Behaviour | Source |
|---|---|---|
| Turn on/off | the Select button, or the app bind `Alt+S` (default; `Select blocks in the viewer`) pressed in the page or in the frame; only when selectable | web/bluemap-bridge.js:111, 188-191; web/app.js:2626, 4912, 3279-3282 |
| While on | `body.mcbench-selecting canvas { cursor: crosshair }`; button active | web/bluemap-bridge.js:73, 997-1004 |
| Stop | Escape (first Escape stops selecting and goes no further), the button, or a UI update making it unselectable | web/bluemap-bridge.js:1083-1084, 183 |
| Drag | left pointer down on the canvas: no pan (event stopped in capture, `mousedown` too); the block under the pointer starts a box (nothing starts on sky); moves pick once per animation frame; a miss keeps the last block; right drag and wheel still move the camera | web/bluemap-bridge.js:1038-1076 |
| Drag box look | outlined box over whole blocks, line width `2`, colour `rgb(245,245,245)` alpha 1, fill alpha `0.16`, depth-tested | web/bluemap-bridge.js:388, 411-433, 1056 |
| Release | posts `mcbench-lite:selection { min, max }` (inclusive, Minecraft y); the drag box goes; selecting stays on for more boxes | web/bluemap-bridge.js:1025-1036, 1077 |
| Cancel | `pointercancel` ends with nothing; a second pointer during a drag starts nothing; the drag's own pointer pressing again (release lost outside the frame) ends that drag and starts a new one | web/bluemap-bridge.js:1048-1052, 1078; test/bluemap-bridge.test.js:686-717 |
| Naming | the page subtracts `yOffset`, POSTs `/api/conversations/<id>/selection { min, max }`; the server answers `{ blocks, block, label }`: label `<Minecraft name of the most common block>, <place>` or `Empty space` (block null); place is `top`/`base` (y share > 2/3 or < 1/3), `north`/`south` (z), `west`/`east` (x) joined as `north-west side`, or `middle`; axes under 3 blocks count as middle; ties go to the block met first by chunk order x, y, z | web/app.js:5362-5377; server/selection.js:9-57; test/selection.test.js:20-61 |
| Letter | the next free letter `A`–`Z`; a 27th box is dropped silently | web/app.js:5372-5376 |
| Chip in composer | `.selection-chip`: block icon `/api/blocks/<name>/icon.png` (or empty square), the label, size `X×Y×Z` (U+00D7), tooltip `<n> blocks`, an `x` button `aria-label="Remove <label>"`; right-click on a draft chip removes it; hovering lights its box | web/app.js:2531-2555, 6928-6929 |
| Drawn in viewer | each draft box: fill alpha `0.08`, `0.22` while its chip is hovered; an icon marker `20×20` at `(centre x, max y + 1.6, centre z)`; a sent message's chip hovered shows its box only (lit, not removable); drawn only in Build mode for a chat | web/bluemap-bridge.js:450-460, 70-72; web/app.js:5383-5397 |
| Remove in viewer | right click (moved ≤ `4` px, not player mode) on a removable box: the nearest box the ray enters is removed (`mcbench-lite:deselect`) | web/bluemap-bridge.js:1127-1152; web/app.js:3226, 7026 |
| Sent | message content gets `{ type: 'selection', selection: { letter, label, min, max, blocks, block } }` parts; the model reads `[Selection <n>( of <m>) (<label>): x a..b, y c..d, z e..f; <blocks> blocks]`, preceded by `[The user picked <m> regions in the viewer, numbered in the order picked]` when more than one | server/index.js:405-413; server/selection.js:59-67; server/agent.js:543-561 |

### 2.15 Labels and overlays

| Overlay | When | Look | Source |
|---|---|---|---|
| Build name labels | a bench comparison (`bench:` view) with a layout | `.mcbench-tag` at `(centre x, max y + 2 + yOffset, centre z)`, text the model's name or `Build <letter>` when names are hidden; dark pill `rgba(12,12,12,.86)`, `#f5f5f5` 500 12px, centred above the point, no pointer events | web/app.js:5346-5357, 5282-5285; web/bluemap-bridge.js:70, 445-449 |
| Selection boxes and icons | 2.14 | | |
| Entities | each tick's entity list from the engine | drawn as Minecraft's entity renderers draw them, lit by the mesher's light; positions and angles glide over the last frame's interval (`50` ms default, clamped `10`–`250` ms) and jump beyond `4` blocks; item and orb entities keep the frame awake | web/entities.mjs:1-23, 360-373 |
| Hover info | none: no hover readout of blocks or entities exists in the pane | (absence; grep of web/bluemap-bridge.js) |
| Grid / chunk borders | BlueMap's `Show chunk borders` setting only; no labelled grid | BlueMap |

Overlays live in a plain group under BlueMap's marker scene so its marker-file sync leaves them alone (web/bluemap-bridge.js:383-386). Entities fail quietly: a failed light request logs `The entity layer could not draw the entities` and retries with the next frame (web/entities.mjs:316-320).

### 2.16 Keyboard routing and focus

The frame is a separate document: keys pressed while it has focus never reach the page's listeners, and the reverse (web/bluemap-bridge.js:1087; web/app.js:4118-4121). The bridge forwards what the page needs.

| Key or class | Frame has focus, map or spectator mode | Frame has focus, player mode | Page has focus | Source |
|---|---|---|---|---|
| `Escape` | if selecting: stop selecting; else post `mcbench-lite:escape`, page closes its topmost layer (e.g. full window) or cancels an edit; the frame never leaves full window on its own | locked: browser frees the mouse, nothing posted; unlocked: as map mode | page closes topmost layer | web/bluemap-bridge.js:1079-1086; web/app.js:4132-4143; test/bluemap-bridge.test.js:656-684 |
| App binds (`Mod+1`, `Mod+2`, `Mod+K`, `Mod+Shift+O`, `Mod+B`, `Alt+S`; Mod = Cmd on Mac, Ctrl elsewhere; user-rebindable) | if the combo is in `uiState.binds`: `preventDefault`, post `mcbench-lite:bind`; page runs it if no modal layer blocks it | same, except Ctrl alone is the sprint key | page runs it | web/bluemap-bridge.js:9-18, 1087-1091; web/app.js:2620-2627, 4918-4927; web/layers.mjs:25-31 |
| Printable character, no Meta/Ctrl/Alt, not composing, not in a field, code not in `KeyW KeyA KeyS KeyD Space NumpadAdd NumpadSubtract`, type-ahead on | stopped, posted as `mcbench-lite:type`; page: in a comparison `1`–`9` frame that model's build, else the composer takes focus and inserts the character at the end (or at the caret if it was inside) | not forwarded | page type-ahead does the same (it excludes `' '`) | web/bluemap-bridge.js:1094-1108; web/app.js:6114-6144 |
| `W A S D`, `Space`, `NumpadAdd`, `NumpadSubtract`, arrows, PageUp/Down, Insert, Home, Delete, End | BlueMap camera (2.8, 2.12) | player keys as 2.11 | page | |
| Type-ahead on | `composerShown && !modalOpen(layers) && !viewerFull` | | | web/app.js:6118, 7029 |
| Fields inside the frame (BlueMap position inputs, menu) | keys typed into them stay there | | | web/bluemap-bridge.js:1104 |

Any `pointerdown`, `wheel`, `keydown`, `resize` or `hashchange` in the frame wakes BlueMap's render loop for one second (web/bluemap-view.mjs:137-212).

### 2.17 Double-click

A double-click on the map canvas, in any mode, posts `mcbench-lite:full` with `on: !full` (web/bluemap-bridge.js:951-955). In map modes the same double-click is also BlueMap's double tap (zoom toward the point, 2.8) and two single clicks (two block clicks and two popups, 2.13). A double-click on the parent `<section>` outside the iframe also toggles (web/app.js:7013).

### 2.18 Messages between page and frame

| Direction | Type | Payload | Source |
|---|---|---|---|
| page → frame | `mcbench-lite:ping` | | web/app.js:3353-3356 |
| page → frame | `mcbench-lite:ui` | `exportable, playable, download, labels, selectable, selections, binds, typeAhead, full, bounds` | web/app.js:3108-3127; web/bluemap-bridge.js:169-187 |
| page → frame | `mcbench-lite:refresh` | `key "<conversationId>:<viewerRevision>", blueprint, mask, wasm, pack, frame, fresh` | web/app.js:3142-3151 |
| page → frame | `mcbench-lite:frame` | `key, patch, entities` | web/app.js:3152-3170 |
| page → frame | `mcbench-lite:player` | `{ view }`, `{ view, tick }`, `{ error }`, `{ gone: true }` | web/app.js:3122, 3166, 3195-3201, 3251 |
| page → frame | `mcbench-lite:fly` | `min, max` | web/app.js:3277, 3295 |
| page → frame | `mcbench-lite:toggle-select` | | web/app.js:3281 |
| frame → page | `mcbench-lite:bluemap-ready`, `mcbench-lite:boot-failed {error}`, `mcbench-lite:refreshed {key}`, `mcbench-lite:refresh-error {key, error}` | | web/bluemap-bridge.js:271-274, 364, 379 |
| frame → page | `mcbench-lite:player-spawn {spawn}`, `mcbench-lite:player-input {keys, taps, yaw, pitch}`, `mcbench-lite:player-leave` | | web/bluemap-bridge.js:755, 792, 799 |
| frame → page | `mcbench-lite:interact {x, y, z, yaw}`, `mcbench-lite:selection {min, max}`, `mcbench-lite:deselect {key}`, `mcbench-lite:export`, `mcbench-lite:full {on}`, `mcbench-lite:escape`, `mcbench-lite:bind {combo}`, `mcbench-lite:type {key}` | | web/bluemap-bridge.js |

Both sides check `event.origin` equals the page origin and the source window (web/bluemap-bridge.js:162; web/app.js:3174).

### 2.19 Lost WebGL context, world too large

Neither the bridge, the page nor BlueMap's MapViewer handles `webglcontextlost` (no match in web/, none in BlueMap's MapViewer.js). A lost context leaves the canvas as the browser leaves it, with no message. There is no world-size message in the pane. Limits that exist: hires tiles load within `1500` blocks of the loaded centre and must stay under `1600` because BlueMap's tile map tracks ±50 tiles of 32 blocks (web/bluemap-view.mjs:4-7); selections refuse corners beyond ±4096 (server/selection.js:31-34); a world export fails for a chunk over 255 sectors with `Chunk <x>, <z> is too large for a region file` (web/world-export.mjs:175), shown as a notice.

### 2.20 Workarounds and known defects in code or comments

| Item | What the code says or does | Source |
|---|---|---|
| BlueMap's click cast throws on maps baked without lowres | replaced by the viewer's own cast | web/bluemap-view.mjs:72-114; test/bluemap-view.test.js |
| HtmlMarker `dispose()` fails once its inner element is gone | outer element removed by hand | web/bluemap-bridge.js:403-409 |
| ExtrudeMarker pads its shape by `0.01` | shape offset `-0.02` so faces land on block edges | web/bluemap-bridge.js:418-421 |
| A cancelled BlueMap mode move leaves the camera with no controls | flies wait for the move to end | web/bluemap-bridge.js:469-500 |
| Chrome refuses pointer lock until the frame has focus | hint stays, next click takes it | web/bluemap-bridge.js:937-939 |
| BlueMap stops drawing hires past distance 1000 and drapes lowres "curtains" over void builds | distance clamped to 799 for rendering, hires forced over the build | web/bluemap-view.mjs:9-17, 46-58 |
| Map target sinks into the void between build parts | terrain height raised inside the build's bounds | web/bluemap-view.mjs:121-135 |
| BlueMap listens for a tile event its map never sends; idle loop at 20 fps | own idle gating | web/bluemap-view.mjs:137-212 |
| The frame cannot hear the page's keys and vice versa | binds, Escape and typing forwarded | web/bluemap-bridge.js:1079-1108 |
| A lost refresh could strand stale tiles | page resends every second until answered | web/app.js:3253-3259 |
| Inferred from code: `Alt+S` in the frame both toggles selecting and is BlueMap's tilt-down (`Alt+KeyS`) until keyup | both listeners on the frame window, neither stops the other | web/bluemap-bridge.js:1079-1092; KeyAngleControls.js:39-43 |
| Inferred: a map double-click toggles full window, zooms 4× toward the point, and sends two block clicks | 2.17 | |
| Inferred: spectator's click calls the Fullscreen API inside an iframe that has no `allow="fullscreen"`; the code proceeds to pointer lock in `.finally` | ambiguous what the user sees per browser | FreeFlightControls.js:146-165; web/app.js:3339-3358 |
| Inferred: BlueMap's touch arrows show in player mode (state `free`); their position changes are overwritten by the player camera once it stands | | App.vue:3; web/bluemap-bridge.js:757-786 |
| Inferred: the menu's `Free-Flight` entry enters whichever of player/spectator was chosen last | `flightMode` only reset by the player button | web/bluemap-bridge.js:561, 648-652 |
| Inferred: BlueMap `Reset Camera` from player mode leaves the player (leave is posted) but does not consume the saved map view | | web/bluemap-bridge.js:584-597 |
| Inferred: the wheel during a `flyTo` animation is overwritten each frame until it ends | flight sets distance every step; only pointerdown cancels it | web/bluemap-bridge.js:538-553, 1043 |
| Inferred: type-ahead letters `w a s d` and Space typed in the frame move the camera instead of reaching the composer | CAMERA_KEYS exclusion | web/bluemap-bridge.js:1094-1103 |
| Inferred: a double-click on the live controls toggles full window (section's `onDblClick`) | | web/app.js:7013 |
| A 27th selection is dropped with no message | | web/app.js:5374-5375 |

### 2.21 Tests that pin the pane

test/live-world-states.test.js defines 27 cases against a stand-in server and engine (version 1, core `core-a`, one block, mask `0..15`).

| # | Line | Case | Inputs | Expected state and text |
|---|---|---|---|---|
| 1 | 171 | an engine that stopped the world keeps its error through polls, and a restart plays the world again | running; `onfailed('The engine failed: unported: a block')`; restart | `status 'error'`, `error` starts `The live world stopped: The engine failed: unported: a block`, `live.playing false`; after restart `running`, playing; two fresh placements; runs `[true, true]` |
| 2 | 185 | a world the engine stopped refuses to play or step, and keeps its first reason until it is placed afresh | stopped; play, pause, step; second `onfailed`; restart; pause; step | rejects `cannot go from stopped to playing`, `cannot go from stopped to paused`, `cannot step while stopped`; `live.stopped true`; engine not run or stepped; error stays exactly `The live world stopped: The engine failed: unported: a block`; after restart error `null`, `stopped false`; runs `[true, true, false]` |
| 3 | 208 | a first sync that failed has nothing on show and says so, and a restart tries it again | import fails `HTTP 503` | `status 'error'`, `error 'HTTP 503'`, `viewerRevision null`, `live null`, `syncFailed true`; polls do not retry; restart makes one more import; then running, `syncFailed false` |
| 4 | 230 | a failed sync is not tried again until the server has another version or a restart is asked for | engine refuses at version 2, then version 3 | `status 'error'`, phase `failed`, no further imports; version 3 runs with one more import |
| 5 | 250 | a failed engine start is tried again by a restart, not by every poll | start fails `The module is unavailable` | `status 'error'`, error exactly that; 2 starts (engine and spare); restart → running, 4 starts |
| 6 | 265 | another engine module ends the engine and places the view afresh in a new one | core changes to `core-b` | new engine running; old engine and old spare dead; first call `place` fresh |
| 7 | 275 | a restart during a sync runs after it | version 2 sync held; restart | imports `import chat since 1` then `import chat`; shown version 2 |
| 8 | 289 | a switch of view takes the new view at once: no look in between syncs the old one | show `b` while `a` runs | key and `conversationId` `b`, `viewerRevision null` at once; an old-engine frame reaches no listener; every later request is for `b` |
| 9 | 309 | a world let go while its engine starts ends that engine | dispose during start | engine dead once started |
| 10 | 322 | a wake during a look looks again right after it | wake during held status | a second status request |
| 11 | 332 | the last tick count is drawn after a burst of ticks, between polls | 5 frames at once | `live.ticks 5` within 400 ms |
| 12 | 341 | a base is drawn with the description of the version it shows, though the build changed during its sync | version 2 with mask `0..31` during first sync | runs version 2 with `viewerMask.maxX 31`; placed once |
| 13 | 356 | an import that answers with a later version than the poll described is not placed: the page looks again | import answers version 2 before a poll described it | phase `waiting`, shown version `null`, no engine start; after the poll version 2 runs with mask `maxX 31`; imports `['import chat', 'import chat']`; placed once |
| 14 | 379 | a delta asked for before the engine's worker failed is not placed as the whole world in the next engine | worker dies during version-2 delta import | next engine places the whole world fresh; last import without `since`; version 2 runs |
| 15 | 395 | a restart whose import failed is still asked for, with the world it would have replaced still held | restart with import `HTTP 503` | phase `failed`, `revision 1`, `restarting true`; not retried alone; asked again tried once; version 2 carries it out fresh, `restarting false`, once |
| 16 | 422 | the world on show is needed to export, input or play, and a frame with nothing since the base carries no patch | export and play before a world; then running | both reject `no live world on show`; `accumulated()` patch `null`, entities `[]` |
| 17 | 432 | a removal waits for the spawn before it, and with no player asks nothing of the engine | remove with no player; spawn held then remove | no engine removal at first; order `spawn`, `removePlayer`; `playerView null` |
| 18 | 449 | a spawn answered after a switch of view leaves no player in the new view, and input for no player goes nowhere | spawn held; show `b` | spawn rejects `The engine was stopped`; `playerView null`; input and removal reach no engine |
| 19 | 485 | the world letting its player go on its own is told; a removal is not, nor a restart, which keeps it standing | spawn, remove, spawn, restart, worker dies | gone count `0` after removal and restart (player `standing`, engine has a player); `1` after the worker dies; `engine null` |
| 20 | 506 | a restart spawns the standing player again where its latest tick has it, with what it held, before the world runs | input `{keys 1, taps 16, yaw 30, pitch -10}`; ticks 40, 41 views; restart; input during placement | calls: spawn `{x 3}`, input, `place true`, spawn `{x 8, y 66.25, z -2, yaw 45, pitch -10, flying true}`, input with `taps 0`, `run true`, the later input; ticks `0` then `1` |
| 21 | 544 | a player who gave no input since its spawn is spawned again with none, and a paused world stays paused | spawn, pause, restart | calls spawn, `run false`, `place true`, spawn at the spawn's view (all 3s, `flying false`); no input, no run; `live.playing false`; player `standing` |
| 22 | 561 | a spawn on its way when a fresh placement is asked stands, and the placement spawns it again where it stood | respawn `{x 4}` held; restart | order `spawn 3`, `spawn 4`, `place`, `spawn 4`; standing; gone `0` |
| 23 | 583 | a first spawn on its way when a fresh placement is asked stands in the placed world | first spawn held; restart | order `spawn 3`, `place`, `spawn 3`; standing; input reaches the engine |
| 24 | 603 | a fresh placement that fails lets the player go and tells the viewer | engine refuses on restart | phase `failed`; player `none`; `playerView null`; gone `1`; no respawn; input goes nowhere |
| 25 | 620 | another view switched to during a fresh placement spawns no player in it | show `b` during restart's placement | `b` running; no respawn; player `none`; gone `0` |
| 26 | 638 | a first spawn that fails stands no one, and the player goes only where its states allow | spawn rejects `The engine failed: unported: a block` | rejects with it; player `none`; `playerTo('standing')` throws `cannot go from none to standing`; `playerTo('spawning', view)` throws `A standing player, and only one, has a view` |
| 27 | 651 | player mode stands the player on the spot the engine finds beside the build, facing its middle, or flies it at the edge when there is none | bounds `[0,0,0]..[9,2,9]`, target `[5,70,5]`, rotation `0`, spot `[5.5,65,9.5]`; then spot `null` | stand asked with min `[0,64,0]`, max `[9,66,9]`, near `[5,64,10.5]`; spawn at `5.5, 65, 9.5`, not flying, yaw within 10° of ±180, pitch between 0 and 30; without a spot `5, 64, 10.5` flying |

Other tests:

| File | Cases | What they pin |
|---|---|---|
| test/bluemap-bridge.test.js | 19 (lines 270-717) | ready only after BlueMap and tile sync; boot failure told once and on ping, never ready; 20 s timeout text; refresh clears entities, patchless frame draws entities; every end of player mode posts a leave; leave while a respawn is pending; superseded spawn answers ignored; restart keeps player mode; a note replaces the last and its timer; a failed patch told once, same view reloads afresh; a view asked twice while loading loads once; fly waits for mode moves, 700 ms, player fly is a flying spawn; player mode starts at once beside the map target, camera stays until it stands, void respawn; leaving restores the map view exactly, flat then moves; a chat opened is framed as a turn frames it; key hold/release rules, two shifts, repeat, blur; note without crosshair/hint; Escape routing; selection drag belongs to its pointer |
| test/bluemap-view.test.js | 6 (lines 60-137) | hires-only cast on a map without lowres; hidden objects skipped; no hit; a marker in front taking the click answers nothing; no map no cast; BlueMap's own taps go through the cast |
| test/viewer-click.integration.test.js | 1 (line 18, needs `MCBENCH_UI_INTEGRATION=1`) | a lever click in real BlueMap names `[1,65,0]`; sky click names nothing; a select drag from lamp to lever gives `[0,64,0]..[1,65,0]`; no page errors |
| test/live-world.test.js | 18 (lines 94-454) | real engine: placement, ticks, late viewer catch-up; lever input; input index for doors, iron door, dust; edit inside the mask as a patch, outside as a new base; another build counts from 0; restart keeps the player; standing beside the build; unported block stops with `The engine failed: unported: …`; table fetch failure; export as a save named as given; empty build has no controls; kept placements shown provisionally and confirmed; mismatch error text |
| test/selection.test.js | 4 | labels (`Dark Oak Stairs, top`, `Stone Bricks, base, north-west side`, `Stone Bricks, middle`, `Empty space`), refusals, cost, ties |
| test/live-patch.test.js | 3 | patch round trip, merging, which blocks take a click (`toggle`, `press`) |
| test/viewer-reset.test.js, test/viewer-entities.test.js, test/viewer-block-entities.test.js | 2, 2, 3 | see_build's headless renderer (web/viewer.js), not the pane |

### 2.22 Coverage

| File | Status |
|---|---|
| web/viewer.js (1545) | covered (headless see_build renderer, not the pane) |
| web/bluemap-bridge.js (1154) | covered |
| web/bluemap-view.mjs (212) | covered |
| web/layers.mjs (31) | covered |
| web/live-world.mjs (778) | covered |
| web/live-host.mjs (91) | covered |
| web/entities.mjs (386) | covered (skimmed for user-visible behaviour) |
| web/entity-draw.mjs (504) | covered (skimmed; drawing only, nothing user-facing beyond 2.15) |
| web/live-patch.mjs (162) | covered (skimmed: InputBlocks) |
| web/place.mjs (63) | covered (skimmed: view keys) |
| web/core-world.mjs (209) | covered (skimmed: click kinds, player view, standing spot) |
| web/resolution.mjs (27) | covered (skimmed; nothing in the pane) |
| web/app.js (7241, grepped) | covered for BlueMapViewer, LiveControls, mount, selections, binds, type-ahead, layers, strata |
| web/style.css (viewer rules) | covered |
| server/selection.js, server/index.js (selection route, message parts), server/agent.js (selection text), server/bluemap.js (webapp config) | covered for the parts cited |
| core/src/player/mod.rs (header, View.encode) | covered for the parts cited |
| BlueMap 5.22 sources from the baked source map (MapControls and sub-controls, FreeFlightControls and sub-controls, BlueMapApp, PopupMarker, ControlBar, ControlsSwitch, Compass, ZoomButtons, FreeFlightMobileControls, MainMenu, SettingsMenu, MapViewer resize, lang/en.conf, settings.json) | covered for the parts cited |
| test/live-world-states.test.js | covered, 27 of 27 cases |
| test/viewer-click.integration.test.js, test/viewer-reset.test.js, test/bluemap-view.test.js, test/bluemap-bridge.test.js, test/live-world.test.js, test/viewer-entities.test.js, test/viewer-block-entities.test.js, test/selection.test.js, test/live-patch.test.js | covered |

| Count | Value |
|---|---|
| Distinct viewer controls recorded (keys and key groups, mouse and touch gestures, buttons) | 76 |
| Live-world-state cases recorded | 27 |

The 76 controls are: map mode 22 (left drag, right drag horizontal, right drag vertical, Ctrl/Alt+left drag, wheel, click/tap, double click/tap, right click on a box, four move key groups, tilt up, tilt down, rotate left, rotate right, zoom in, zoom out, one-finger pan, two-finger tilt, twist, pinch); player mode 12 (click to lock, mouse look, right-click use, left click no-op, W, S, A, D, Space, Shift, Ctrl, Escape); spectator 8 (move keys, up keys, down keys, wheel speed, drag look, click to lock, locked look, touch look); selection 4 (toggle, drag, release, cancel); frame buttons 17 (Menu, Day/Night, Perspective, Flat, Player, Spectator, Reset camera, Position input, Compass, Select, Download, Export, Fill, Zoom +, Zoom −, touch forward/back, touch up/down); page buttons 6 (Play/Pause, Step, Restart, cover Try again, Reload the viewer, sync Try again); keys routed by the pane 4 (Escape routing, app binds, type-ahead characters, comparison digits 1–9); plus the strata divider drag, the section double-click, and frame blur releasing keys.

## Part 3: the server's routes as the page uses them

All paths below are relative to the repository root. `server/index.js` is "index.js" in the tables; other files are named in full. "The page" is `web/index.html` with `web/app.js` and its modules. "The renderer" is `web/shot.html` with `web/shot.mjs` and `web/viewer.js`, the see_build page the server itself opens in headless Chromium (server/shot.js:79 sends `authorization: Bearer <token>`); it is a client of the routes, but not a feature of the page.

### How every request reaches a route

`route` (index.js:1347-1361) runs the same steps for every request, in this order. Security headers go on first (server/http-guard.js:91-95: `x-content-type-options: nosniff`, `referrer-policy: no-referrer`, `cross-origin-resource-policy: same-origin`), then the CSP (server/http-guard.js:97-99: `BLUEMAP_CSP` for paths under `/bluemap/`, `APP_CSP` for every other path). Then `foreignRequest` (server/http-guard.js:58-65) refuses with 403, `content-type: text/plain`, body `Forbidden: requests must come from this app on this machine`, when the `Host` header is none of `mcbench-<10 hex>.localhost:<port>`, `127.0.0.1:<port>`, `localhost:<port>`, `[::1]:<port>` (server/http-guard.js:54-56); when a non-GET/HEAD request carries an `Origin` other than `http://<Host>`; or when `Sec-Fetch-Site` is present and not `same-origin` or `none`, unless the request is a GET navigation (`Sec-Fetch-Mode: navigate`) to a non-`/api/` path.

Then authentication (server/auth.js). Any request whose query has `token` goes to `signIn` (server/auth.js:94-108), whatever its path. Otherwise `authorized` (server/auth.js:79-84) accepts `Authorization: Bearer <64 hex token>` on any of the four hosts, or the cookie `mcbench_token` only on the app's own host name. A refused request gets 401 with `cache-control: no-store`: as HTML when `Accept` includes `text/html`, otherwise JSON `{ "error": "mcbench-lite needs its access link: open the address the server printed when it started (it ends in ?token=...), or print it with npm run url." }` (server/auth.js:110-119). After that, `/bluemap/` paths, `/api/` paths, `/core/` paths and static files are tried in that order (index.js:1357-1360).

Inside `/api/`, a thrown error ends the request: if no header was sent, JSON `{ "error": String(err.message) }` with `err.statusCode` or 500; if headers were sent (a stream had started), the response is only ended (index.js:1364-1370). Errors under 500 are not logged. JSON bodies are read by `readJsonBody` (server/http-guard.js:19-36): an empty body is `{}`; over the route's limit is 413 `Request body exceeds <n> MiB`; unparsable is 400 `Invalid JSON body: <parser message>`; a non-object is 400 `Expected a JSON object`. The limits are `form` 1 MiB (the default), `message` 280 MiB, `world` 128 MiB (server/http-guard.js:11). The server cuts off a client whose headers take over 10 s or whose request takes over 120 s to arrive (index.js:1377-1378). Tests: test/http-guard.test.js:14 (`only this app on this machine gets through`), :36, :54, :79; test/auth.test.js:50, :91.

A refusal from `foreignRequest` is plain text. The page's `api()` helper (web/app.js:26-31) calls `res.json()` before it checks the status, so a 403 or any non-JSON answer reaches the page as a JSON `SyntaxError`, not as the server's text.

### The page's request helpers

`api(path, options)` (web/app.js:26-31) parses the body as JSON and throws `new Error(body.error || 'HTTP <status>')` on a non-2xx status. `postJson(path, body)` (web/app.js:4781) is `api` with POST and a JSON body. `readLines(res, signal, onMessage)` (web/app.js:35-47) reads an NDJSON body line by line until it ends or `signal` aborts. `readShareStream(body, onMessage)` (web/share.mjs:98-110) is the same reader without a signal. `browserHost().json` (web/live-host.mjs:64-83) is the live world's fetch: it throws the JSON `error` or `HTTP <status>`. `shared.load(url)` (web/app.js:50-65, web/shared.mjs) caches GET answers that several views read (the model lists and model info).

"showError" in the tables means the failure goes to `showError` (web/app.js:3749-3754): a notice at the top of the chat column, one per distinct text, kept until dismissed.

### Routes outside /api/

| Method | Path | What it does | Errors | Who uses it |
|---|---|---|---|---|
| GET (any path) | `<path>?token=<token>` | On the app's host name: 303 to `<path>` without `token`, sets `mcbench_token=<token>; Max-Age=34560000; Path=/; HttpOnly; SameSite=Strict`, `cache-control: no-store`. On another of the four hosts: 303 to `http://<app host>:<port><path>?token=...`, no cookie. Leading slashes of the path collapse to one (server/auth.js:94-108). | A non-GET or a wrong token gets the 401 above. | The link the server prints (server/auth.js:53-56). Test: test/auth.test.js:91. |
| GET, HEAD | `/bluemap/mesher-<hash>.wasm` | The mesher's WebAssembly, `cache-control: public, max-age=31536000, immutable`, with ETag/304 (server/bluemap.js:273-276, server/static-file.js:10-22). | None of its own. | web/mesher-worker.mjs:37 via `mesher.wasm` from `/api/minecraft/status`; web/viewer.js:631-635 (renderer). |
| GET, HEAD | `/bluemap/<assets id>/<file>` | The baked BlueMap web app and its files; an empty file part is `index.html` (server/bluemap.js:277-298). | 404 empty body when the path is not under the current bake's prefix or the file is missing; 204 for a missing file under `maps/` (the web app polls markers); 400 empty for a malformed percent-encoding. Other methods fall through to `/api` and static handling (server/bluemap.js:267). | The viewer iframe, `src=${viewerUrl}` (web/app.js:3342); the iframe's bridge (web/bluemap-bridge.js). `viewerUrl` comes from `/api/minecraft/status`. |
| GET, HEAD | `/core/<version>-<tables hash>/{engine.wasm, blocks.txt, loot.json}` | The engine module and the two Minecraft tables, `cache-control: private, max-age=31536000, immutable` (server/core-wasm.js:81-94). | 404 `Not found` text for a name outside the three or a prefix other than the current one. Awaits `prepare()`, so a failed preparation is a thrown error, answered by index.js:1364-1370. | web/core-engine.mjs:20 (`fetched(url)`), address from `/api/minecraft/status` `core`. |
| GET, HEAD | `/vendor/preact-htm.mjs`, `/vendor/deepslate.js` | Files from node_modules (index.js:1318-1321), `cache-control: no-cache`, ETag/304. | 404 `Not found` text. | web/app.js:1 imports `/vendor/preact-htm.mjs`. |
| GET, HEAD | `/vendor/gl-matrix/<file>` | node_modules/gl-matrix/esm files (index.js:1329-1332). | 404 `Not found` text outside that folder. | The renderer (web/viewer.js). |
| GET, HEAD | A page address: `/`, `/new`, `/chat/<id>`, `/bench`, `/bench/<id>`, `/bench/<id>/<memberId>`, `/leaderboard`, `/prompts` | `web/index.html` (index.js:1334, web/address.mjs:1-15, :64). | None. | The page. Test: test/page-addresses.test.js. |
| GET, HEAD | Any other path | The file under `web/` (`.cjs` as JavaScript, else by extension, index.js:1341), `cache-control: no-cache`, ETag/304. | 404 `Not found` text for a missing file or a path outside `web/`. | Every page module, `/mesher-worker.mjs`, `/bluemap-tiles.mjs` (web/bluemap-bridge.js:284-290). |

### API routes: setup, configuration and sign-in

Every row passes the guards above. "form" is the 1 MiB body limit.

| Method | Path | Query or body | Answer | Errors (status, text) | Page feature (web file:line) |
|---|---|---|---|---|---|
| GET | `/api/config` | none | `{ providers, modelFavorites, mcVersion, hosted? }` (index.js:693-700). `providers` is `publicSettings().providers`: per provider id `{ label, connected, envKey, auth, harness, enabled, hiddenModels, modelOrder }` (server/settings.js:170-189); `auth` is `api-key`, `cli-login` or `api-key-or-login`. `modelFavorites` is an array of `"<provider>::<model>"`. `mcVersion` is the Minecraft version string. `hosted: true` only when `MCBENCH_HOSTED=1`. | None of its own. | App start, `loadConfig` (web/app.js:3978-3993): reads `providers`, `hosted`, `modelFavorites`; failure sets the config's `failed` state and showError. Test: test/provider-login.test.js:278. |
| GET | `/api/setup` | none | `{ ready, steps: [{ id, title, doing, optional, state, detail?, progress?, seconds?, error? }] }` (server/setup.js:109-112). Step ids in order: `jdk`, `minecraft`, `tables`, `blocks`, `viewer`, `engine`, `browser`, `shell` (server/setup.js:25-34; the comment at index.js:702-714 lists `rust` and `wasm` steps that `STEPS` does not have). `state` is `waiting`, `running`, `done`, `failed` or `unavailable`; `progress` is `{ done, total }` bytes, `total` null when unknown; `seconds` is a one-decimal number. `ready` is true when every non-optional step is `done`. | None. | The setup screen, polled each second until `ready` (web/app.js:3999-4010); fields read in web/setup-status.mjs (`id`, `title`, `doing`, `optional`, `state`, `detail`, `progress`, `error`). Failure: showError. Test: test/setup.test.js. |
| POST | `/api/setup/retry` | none | The same document as GET `/api/setup`, after starting the failed steps' work again (server/setup.js:101-105). | 409 `No setup step failed`. | Setup screen Retry button (web/app.js:6795), answer becomes the setup state; failure: showError. Test: test/setup.test.js:73. |
| POST | `/api/cli-installs` | form: `{ program }`, `claude` or `codex` | `{ state, progress, error }`: `state` `running`, `done` or `failed`; `progress` `{ done, total }` or null; `error` string or null (server/cli-install.js:25-51). A running install answers its own state. | 400 `No CLI named <name> can be installed: claude or codex`; 409 `<name> is installed already, and the app runs that one`. | Settings, Connect an AI, the install-then-sign-in button (web/app.js:984-988); failure clears the pending sign-in and goes to `onError` (showError). Test: test/cli-install.test.js:50, :63. |
| GET | `/api/cli-installs/<program>` | none | `{ state, progress, error }`, or JSON `null` when none was started (server/cli-install.js:54-57). | 400 as above for an unknown program. | Settings sign-in rows, read on open and each second while `running` (web/app.js:945, :961-973); reads `state`, `progress`, `error`. |
| GET | `/api/activity` | none | `{ busy }`: true while any hold, title task, or a queued bench round exists (index.js:501-507, :737-739). | None. | Not used by the page; the hosted gateway reads it (index.js:735-736). Test: test/provider-login.test.js:262. |
| POST | `/api/provider-logins` | form: `{ provider, method }`; `method` is `claudeai` or `console` for `claude-code`, ignored otherwise | `{ state, url, userCode, wantsInput, message, error }` (server/provider-login.js:135-137). `state` is `starting`, `waiting`, `done`, `failed` or `cancelled`. | 400 `<provider> has no sign-in`; 409 `A sign-in is already under way here. Finish it or cancel it first.`; 400 `Choose a sign-in method: claudeai or console`; a CLI that cannot be resolved throws from `entry.login` (status as thrown). | Settings sign-in buttons (web/app.js:975, :984-985); errors to `onError` (showError). Test: test/provider-login.test.js:96, :155, :178, :232. |
| GET | `/api/provider-logins/<provider>` | none | The same view, or JSON `null` when no sign-in was started (server/provider-login.js:212). | 400 `<provider> has no sign-in`. | Settings sign-in rows: on open, then each second while `starting` or `waiting`; `done` triggers a settings recheck (web/app.js:944, :948-959). Reads `state`, `url`, `userCode`, `wantsInput`, `message`, `error` (web/app.js:1010-1056). |
| DELETE | `/api/provider-logins/<provider>` | none | The view after cancelling (server/provider-login.js:223-227). | 400 `<provider> has no sign-in`; 404 `No sign-in was started here`. | Cancel button (web/app.js:994). |
| POST | `/api/provider-logins/<provider>/input` | form: `{ text }`, the pasted code | The view (server/provider-login.js:215-221). The text goes to the CLI's stdin only. | 400 `<provider> has no sign-in`; 404 `No sign-in was started here`; 409 `This sign-in is not waiting for a code`; 400 `Paste the code as one line`. | The code form (web/app.js:995-998). Test: test/provider-login.test.js:96, :163. |
| POST | `/api/provider-logouts` | form: `{ provider }` | `{ ok: true }` (index.js:758-762). | 400 `<provider> has no sign-in`; 409 `Finish or cancel the sign-in first`; the CLI's last lines, or `<file> <args> ended with exit code <n>`, as a 500 (server/provider-login.js:245, a plain Error). | Sign out button, then a settings recheck (web/app.js:1085-1086); errors: showError. |

### API routes: models, prompts and settings

| Method | Path | Query or body | Answer | Errors | Page feature |
|---|---|---|---|---|---|
| GET | `/api/models` | `?provider=<id>` | `{ provider, models }` (index.js:769-773). For catalogue providers each model is `{ id, name, created, known, imageInput, imageOutput, tools, reasoning, efforts, defaultEffort, mandatoryReasoning, contextWindow }` (server/llm.js:116-150, :270-276), newest first, image-output models dropped. `codex`, `codex-cli`, `cursor` and `qwencloud` lists come from their own modules (server/llm.js:200-214) and are not shaped here. | 400 `Unknown provider`; a provider failure is a 500 with the provider's message (for example `LLM request failed (HTTP <status>): <message>`, server/llm.js:280-291). | Model pickers, through `shared.load` (web/app.js:67, :745, :751, :3987). Errors land on the cached entry, not showError. |
| GET | `/api/model-info` | `?provider=<id>&model=<id>` | The model's capabilities, the same fields as one model above without `id`, `name`, `created` (server/llm.js:152-159). | 400 `provider query parameter is required`; 400 `model query parameter is required`; provider failures as 500. | Effort and image gating in the composer and Settings (web/app.js:68-73, :3857, :4292). |
| PUT | `/api/model-favorites` | form: `{ favorites: [string] }` | `{ favorites }`, normalized: `provider::model` entries for known providers, deduplicated, at most 500 (server/settings.js:86-99, :151-155). | 500 `favorites must be an array` (a plain Error). | Favorite stars (web/app.js:3480-3482, web/favorites.mjs); errors from `toggle` go to showError (web/app.js:4429-4435). |
| POST | `/api/prompts/generate` | form: `{ provider, model, effort, text }` | `{ text }`, the rewritten prompt (index.js:784-792, server/prompt-generation.js:13-31). | 400 `Choose a configured provider`; 400 `<label> is not configured`; 500 `Choose a model`, `Write something to improve first`, `The model returned an empty prompt` (plain Errors), or the provider's error. | Prompt library "Improve with AI" (web/app.js:2319-2345); errors shown inline under the prompt (`setAiError`), not showError. |
| GET | `/api/prompts` | none | `{ prompts }`: each `{ id, title, text, tags, favorite, attachments: [{ name, type, size, data }], usedAt?, addedBy?: { chat, model } }` (server/prompts.js:79-91 `clean`; seed prompts when no file). | None of its own. | Prompt library and `/` palette, `PromptSync.read` (web/app.js:3665-3666, web/prompt-sync.mjs). Reloaded after a turn's `prompt_library` tool result (web/app.js:4652). |
| PUT | `/api/prompts` | message limit: `{ prompts: [...] }` | `{ prompts }`, as saved (each through `clean`). | 400 `prompts must be an array`; 400 `A prompt attachment needs a name and its bytes as a base64 data URL` (server/prompts.js:69-74). | `PromptSync.write` (web/app.js:3667). Test: test/prompts.test.js:21, :32. |
| GET | `/api/settings` | optional `?refresh=1` (clears CLI version cache and Codex model cache) | `{ titleModel, modelFavorites, shareRepo, providers, checkedAt }` (server/settings.js:211-234, index.js:804-814). `titleModel` is `{ provider, model, effort }` or null; `shareRepo` is `owner/name` or null. Each provider has the `/api/config` fields plus `endpoint` (base URL or null), `protocol` (`Chat Completions`, `Responses`, `Messages`, `Cursor SDK agent`, `Claude Code CLI over MCP`, `Codex CLI over MCP`), `summary` (one line, server/settings.js:239-248), and where they apply `keySource` (`saved`, `env` or null), `runtime` `{ binary, binaryEnv, homeEnv, home, binaryValue, resolvedHome }`, `signedIn`, `account`, `plan`, `version`, `signIn` `{ methods, program: { name, title, bytes } \| null, current, installed }`. | None of its own. | Settings opening (web/app.js:4287-4305), recheck with `?refresh=1` (web/app.js:4378-4394), and the Share panel's repository (web/app.js:4318). Failure on open closes Settings and the caller shows it. |
| PUT | `/api/settings` | form: the whole Settings draft; read are `providers.<id>.{ apiKey, enabled, hiddenModels, modelOrder }`, `titleModel` (null or `{ provider, model, effort }`), `shareRepo` (null or string) | `{ ok: true, titleModel, modelFavorites, shareRepo, providers }` (index.js:815-819). An empty `apiKey` keeps the saved key (server/settings.js:47-56). `modelFavorites` in the body is ignored (server/settings.js:129). | 500 (plain Errors) `Unknown title provider: <id>`, `Choose a title model`, `The repository for shared builds must be owner/name`, `The settings were not saved: <temp> already exists. Remove it and save again.` | Settings Save (web/app.js:4399-4416): reads `providers`; a failure reopens Settings with the error text in the panel. Tests: test/settings.test.js, test/settings-data-dir.test.js. |

### API routes: static data the viewers read

| Method | Path | Answer | Errors | Who uses it |
|---|---|---|---|---|
| any | `/api/assets/<name>` with `<name>` one of `blocks.json`, `block_definitions.json`, `models.json`, `atlas_map.json`, `atlas.png` (server/assets.js:18-23) | The file's bytes, `image/png` for `.png` else `application/json`, `cache-control: private, max-age=86400` (index.js:822-831). | 404 `Unknown asset`. | The renderer only (web/viewer.js:206-210). |
| GET | `/api/block-states` | Binary block-state table, `application/octet-stream`, `cache-control: private, no-cache` (index.js:835-839). | None. | The renderer (web/viewer.js:211). |
| GET | `/api/block-tints` | Binary tint table, same headers (index.js:843-847). | None. | The renderer (web/viewer.js:212). |
| GET | `/api/blocks/<block>/icon.png` | PNG, `cache-control: private, max-age=31536000, immutable` (index.js:849-854). | 404 `Unknown block`. | Selection chips and viewer labels, `<img src>` (web/app.js:3062, :2544, :5389). |
| GET, HEAD | `/api/images/<chat>/<file>` | A stored screenshot or image, `image/jpeg` for `.jpg` else `image/<ext>`, `cache-control: private, no-cache`, ETag/304 (index.js:860-867). | 404 `Image not found`; a path that escapes the folder throws `Bad image path` (500, server/store.js:542-547). | Transcript shots, `<img src>` and the lightbox (web/app.js:5970, :7042), from `seeBuild` messages and `tool_result.images`. Test: test/attachment-serving.test.js:89. |
| GET, HEAD | `/api/attachments/<owner>/<file>` | The stored file. Types kept for `.png .jpg .jpeg .webp .gif .svg .pdf .txt .md .csv` and sent `inline`; any other extension is `application/octet-stream` with `content-disposition: attachment`. Always `content-security-policy: sandbox; frame-ancestors 'none'` and `cache-control: private, no-cache`, ETag/304 (index.js:869-882). | 404 `Attachment not found`; `Bad attachment path` (500, server/store.js:621-626). | Attachment thumbnails and links (web/app.js:5837, :6154, :6936); re-read for edit (`withData`, web/app.js:5238-5242) and copy (web/clipboard.mjs:20), each throwing `Could not load "<name>" (HTTP <status>)` or `Could not copy ...`. Test: test/attachment-serving.test.js:50. |

### API routes: chats

`<id>` is a conversation id. `ownChat` (index.js:429-434) answers 404 `Conversation not found` for an unknown or archived chat (store.getConversation filters archived rows, server/store.js:750-754) and 409 `A bench chat is changed only through its bench` for a bench model's chat; only the rows that say "ownChat" apply it.

| Method | Path | Query or body | Answer | Errors | Stream | Page feature |
|---|---|---|---|---|---|---|
| GET | `/api/conversations/stream` | none | NDJSON, one whole list per line (index.js:884-890, :179-187); see the list stream below. | None of its own. | yes | Sidebar lists, held by every open page (web/app.js:3904-3975). |
| POST | `/api/conversations` | form: `{ provider, model }` | `{ id, provider, model }` (index.js:892-905). The row is empty and unlisted until it has a message (server/store.js:671-675). | 400 `Provider and model are required`. | no | New chat and first send (web/app.js:4659-4683); a failure is thrown to the send or New chat caller. |
| GET | `/api/conversations/<id>` | none | `{ id, title, provider, model, effort, messages, messageTree, runningSince }` (index.js:1120-1133). `messages` are stored messages without `parentId` and `position` (server/store.js:637-642): each has `id`, `role`, `createdAt`, `content`, and by role `tool_calls`, `tool_call_id`, `reasoning`, `reasoning_details`, `finish_reason`, `telemetry`, `notices`, `plan`, `rateLimit`, `seeBuild`, `round`, `prompts`, `stopped`, `reply`, `dismissed`. `messageTree` is `{ headId, messages: [{ id, parentId, position, role, branchable }] }` over every branch (server/store.js:777-791). `runningSince` is the running turn's start (epoch ms) or null. | 404 `Conversation not found`. | no | Opening a chat and resync after a turn (web/app.js:3766-3773, :3811-3823). The page reads `messages` through `itemsFromMessages` (web/app.js:336-401: `role`, `content` parts `text`, `image_url`, `attachment`, `selection`, `seeBuild`, `dismissed`, `telemetry`, `rateLimit`, `notices`, `plan`, `reasoning`, `tool_calls`, `tool_call_id`) and `messageTree`. A failure sets the transcript's `failed` state, shown in the transcript pane with a read-again action. |
| GET | `/api/conversations/<id>/schematic` | none | `application/octet-stream`, `content-disposition: attachment; filename="<title>.schem"`, `cache-control: no-store` (index.js:1134-1145). | 404 `Conversation not found`. | no | The viewer's download link, set as an `href` in the iframe (web/app.js:3112, web/bluemap-bridge.js:89-90); errors are whatever the browser does with a JSON 404. |
| GET | `/api/conversations/<id>/share` | none | `{ url }`, the shared page's address or null (index.js:1154). | 404 `Conversation not found`. | no | Share panel opening (web/app.js:4316-4331, web/share.mjs:55). |
| POST | `/api/conversations/<id>/share` | none | NDJSON publish stream (index.js:1155-1161); see the publish stream below. | 404 `Conversation not found`; 400 `Set the GitHub repository for shared builds in Settings`; 400 `The build is empty: there is nothing to share`; 409 `Wait for the running turn before sharing`; 409 `This build is already being published` (server/share.js:256). All are JSON before the stream starts. | yes | Share panel Publish (web/app.js:4336-4347): a refusal fails the share with the server's text, shown in the Share panel. Tests: test/share-routes.test.js:102, :120, :139. |
| DELETE | `/api/conversations/<id>/share` | none | `{}` (index.js:1162-1165). | 404 `Conversation not found`; 404 `This build is not shared` (also when no repository is set); 409 `Wait for the build to finish publishing`; git or gh failures as 500 with their message. | no | Share panel Remove (web/app.js:4350-4359); a failure keeps the share and goes to showError. |
| POST | `/api/conversations/<id>/selection` | form: `{ min: [x,y,z], max: [x,y,z] }` integers | `{ blocks, block, label }`: `block` is `minecraft:<id>` or null, `label` is `<Minecraft name>, <placement>` or `Empty space` (server/selection.js:30-57). | 404 `Conversation not found`; 400 `A selection needs integer corners within +/-<MAX_COORD> with min at most max`. | no | Picking a box in the viewer (web/app.js:5364-5377, :7025); failure: showError. Test: test/selection.test.js. |
| POST | `/api/conversations/<id>/seen` | none | `{ ok: true }`, then a list push (index.js:1173-1177). The id is not checked. | None. | no | Marking a chat read (web/app.js:4052-4055, :4608, :5341-5344); errors ignored (`.catch(() => {})`). |
| POST | `/api/conversations/<id>/world` | world limit: `{ known: { <chunk key>: <version> }, renderLod, clip, tick }` | NDJSON world stream (index.js:1185-1234); see below. | 404 `Conversation not found`; 409 `No engine world placed for this conversation`. | yes | The renderer only (web/shot.mjs:79-121); it throws `World load failed (HTTP <status>)`. |
| POST | `/api/conversations/<id>/dismiss-error` | form: `{ messageId }` | `{ ok: true }` (index.js:1235-1244). | 404 `Conversation not found`; 409 `Wait for the running turn before dismissing an error`; 400 `messageId must point at an error message` (server/store.js:660-666). | no | The x on a stored error (web/app.js:5925-5935); the page drops the item first and forgets it in its cache; failure: showError. Tests: test/server-turns.test.js:733, :774. |
| POST | `/api/conversations/<id>/archive` | none | `{ ok: true }`, then a list push (index.js:1245-1252). ownChat. | 404, 409 from ownChat; 409 `Stop the running turn before archiving`. | no | Sidebar archive button (web/app.js:4708-4712, :6603); failure: showError. Test: test/server-archive.test.js:120. |
| POST | `/api/conversations/<id>/fork` | form: `{ round }`, integer from 1 | The new chat's list row (the `conversations` row shape below) (index.js:1254-1283). Reveals a blind bench. | 404 `Bench chat not found`; 400 `Unknown round`; 409 `Wait for this round to finish before forking it`; a copy or store failure as 500. | no | "Continue in Build" on a bench model (web/app.js:5206-5218, :6709, :6717); the page puts the row into its list and opens it; failure: showError. Tests: test/server-turns.test.js:477, :653. |
| POST | `/api/conversations/<id>/chat` | message limit: `{ provider, model, resolution, text, files, selections, prompts, effort?, editOf? }` (see the notes below the table) | On acceptance, the turn's NDJSON stream (index.js:436-496); see the turn stream below. | ownChat 404, 409; 409 `A turn is already running for this conversation`; 503 `The server is shutting down`; 400 `Choose a configured provider`; 400 `Choose a model`; 400 `Resolution must be one of 1, 2, 5, 10`; 400 `A message supports at most 8 attachments`; 400 `Message text or an attachment is required`; 400 `Each prompt a message came from needs its id and title`; 400 `Each selection needs a letter, a label, integer corners, a block count and its main block`; 400 `Attachment name is required`, `Attachment "<name>" is not base64 data`, `Attachment "<name>" is empty`, `Attachment "<name>" exceeds 100 MB`, `Attachments exceed 200 MB total` (server/store.js:553-575); 507 storage refusals (server/storage.js:32, :49-53, :117-121); 400 `messageId must point at a user message` or `No world checkpoint for this message` for a bad `editOf` (server/store.js:942-949, :965-975). All are JSON before the stream starts. | yes | Composer send (web/app.js:5487-5556). On a non-2xx the page puts the draft back, restores the transcript and shows the error (showError). Tests: test/server-turns.test.js:158, :183, :464, :514; test/resolution.test.js:140, :154. |
| GET | `/api/conversations/<id>/stream` | none | The running turn's NDJSON stream, replayed from its first line (index.js:1288-1292, server/turn-stream.js:22-32). No ownChat: a bench model's turn is joined here too. | 404 `No turn is running`. | yes | Joining a turn after reload, from another window, or after a broken stream (web/app.js:4544-4562). 404 means the turn ended: the page resyncs. Another refusal: showError and the turn is left `detached`. |
| POST | `/api/conversations/<id>/stop` | none | `{ ok: true }`, whether or not a turn runs (index.js:1293-1296). | None. | no | Stop button (web/app.js:5473-5480, :5551-5554) and a bench model's stop (web/app.js:6383); failure: showError. Test: test/server-turns.test.js:136. |
| POST | `/api/conversations/<id>/branch` | form: `{ messageId }` | `{ messages, messageTree }`, as in GET `/api/conversations/<id>` (index.js:1299-1310). | ownChat 404, 409; 409 `Stop the running turn before switching branches`; 503 `The server is shutting down`; 400 `Unknown message branch`. | no | Branch arrows on a prompt (web/app.js:5564-5585); the caller shows the failure. Tests: test/server-turns.test.js:183, :774. |

The chat POST body, as the page sends it (web/app.js:5528-5531) and the server reads it (index.js:446-458, :393-401): `provider` must be a key of `settings.PROVIDERS`; `model` a non-empty string; `resolution` one of `1`, `2`, `5`, `10` (web/resolution.mjs:9-12); `text` a string (trimmed); `files` up to 8 `{ name, type, size, data }` with `data` a base64 data URL; `selections` `[{ letter, label, min, max, blocks, block }]` (server/selection.js:60-67); `prompts` `[{ id, title }]`; `effort` a string (empty means none; any non-string leaves the stored effort); `editOf` the message id of the prompt being replaced. A stored attachment becomes `{ name, type, size, url: "/api/attachments/<owner>/<stored name>", path: "/work/attachments/<stored name>" }` (server/store.js:589-596), and a message with attachments or selections is stored as content parts `[{ type: "text", text }, { type: "attachment", attachment }..., { type: "selection", selection }...]` (index.js:406-421).

### API routes: benches

`<id>` is a bench id. For every path `/api/benches/<id>/...` other than `restore` and the bare DELETE, a POST body is read first (message limit for `rounds`, form otherwise), then an unknown or archived bench is 404 `Bench not found` (index.js:998-1001). A path under it that matches nothing falls through to 404 `Not found`.

| Method | Path | Body | Answer | Errors | Page feature |
|---|---|---|---|---|---|
| POST | `/api/benches` | message limit: `{ members: [{ provider, model, effort }], text, files, prompts, blind, resolution }` | `{ id }` (index.js:923-939). The first round starts for every model before the answer. | 400 `A bench runs 2 to 8 models`; 400 `Every model needs a configured provider`; 400 `Selections belong to one build, not a bench`; the resolution, message and attachment 400s of the chat POST; 507 storage refusals. | New bench Run (web/app.js:5168-5181); a failure puts the draft back and is thrown to the submit caller (showError). Tests: test/server-turns.test.js:302; test/resolution.test.js:154, :166. |
| GET | `/api/benches/<id>` | none | `{ id, title, createdAt, head, rounds, blind, revealed, order, votes, stats }` (index.js:1002-1006, server/store.js:1356-1374). `rounds` is the active line: each `{ id, parentId, content, prompts?, createdAt, branch: { index, count, previousId, nextId } }` (server/store.js:1241-1253, :1453-1455). `votes` maps round id to `{ pick, blind }`, `pick` a member id or null for a tie. `stats` maps member id to one entry per round, `{ ms, inputTokens, outputTokens }`, each possibly null (index.js:604-619). `nodes` is left out. | 404 `Bench not found`. | The comparison (web/app.js:5290-5303, :5322-5331); reads `rounds[].id`, `content`, `branch`, `votes[roundId].blind`, `.pick`, `stats[memberId][index]` with `ms`, `inputTokens`, `outputTokens` (web/app.js:6318-6380, :3014-3035). A failure sets the details' `failed` state, shown in the comparison. |
| POST | `/api/benches/<id>/vote` | `{ roundId, pick }`: `pick` a member id, `"tie"`, or null to take the vote back | `{ votes }`, the bench's votes after the change (index.js:1009-1025). | 409 `Votes are cast while the models' names are hidden`; 400 `Unknown round`; 409 `Vote once every model has finished this round`; 409 `A vote needs at least two finished builds`; 400 `Pick a model that finished this round`. | Vote buttons (web/app.js:5305-5309, :6332); failure: showError. Test: test/server-turns.test.js:495. |
| POST | `/api/benches/<id>/reveal` | none | `{ ok: true }`, then a list push (index.js:1027-1031). | 404. | Reveal confirm (web/app.js:6698); failure: showError. |
| POST | `/api/benches/<id>/branch` | `{ roundId }` | `{ round }`, the 1-based index of that round on the new line (index.js:1034-1046). | 400 `Unknown round`; 409 `Stop the running models before switching branches`; 503 `The server is shutting down`. | Round branch arrows (web/app.js:5315-5323, :6348, :6356); failure: showError. Test: test/server-turns.test.js:718. |
| POST | `/api/benches/<id>/rounds` | message limit: `{ text, files, prompts, editOf? }` | `{ rounds }`, the line's length after the change (index.js:1052-1070). | 400 `Selections belong to one build, not a bench`; 409 `Wait for the round edit to finish`; 400 `Unknown round` for a bad `editOf`; 409 `Stop the running models before editing a round`; 503; the message and attachment 400s; 507. | Composer send in a bench (web/app.js:5184-5195); a failure puts the draft back and is thrown (showError). Tests: test/server-turns.test.js:247, :696. |
| POST | `/api/benches/<id>/retry` | `{ memberId }` | `{ ok: true }` (index.js:1073-1084). | 404 `This model is not in the bench`; 409 `Only a failed or stopped model can be retried` (both the check and the hold use this text). | Retry on a failed or stopped model (web/app.js:6384); failure: showError. Tests: test/server-turns.test.js:220, :411, :709. |
| GET | `/api/benches/<id>/share` | none | `{ url }` or `{ url: null }` (index.js:1089). | 404. | Share panel for a bench (web/app.js:4316-4331). |
| POST | `/api/benches/<id>/share` | none | NDJSON publish stream (index.js:1090-1099). | 400 `Set the GitHub repository for shared builds in Settings`; 400 `No model in this bench has built anything yet`; 409 `Wait for the bench's running turns before sharing`; 409 `This build is already being published`. | Share panel Publish (web/app.js:4336-4347). Tests: test/share-routes.test.js:234, :251. |
| DELETE | `/api/benches/<id>/share` | none | `{}` (index.js:1100-1103). | 404 `This build is not shared`; 409 `Wait for the build to finish publishing`. | Share panel Remove (web/app.js:4350-4359). |
| POST | `/api/benches/<id>/archive` | none | `{ ok: true }`, then a list push (index.js:1105-1115). | 409 `Stop the running models before archiving`. | Sidebar archive (web/app.js:5198-5202, :6287); failure: showError. Test: test/server-archive.test.js:173. |

### API routes: archive, search, leaderboard and shares

| Method | Path | Query or body | Answer | Errors | Page feature |
|---|---|---|---|---|---|
| GET | `/api/archive` | none | `{ conversations: [{ id, title, provider, model, archivedAt, expiresAt }], benches: [{ id, title, archivedAt, expiresAt, models }] }`, newest first, epoch ms, `expiresAt` 7 days on (server/store.js:1021-1036). Bench models' chats are not listed. | None. | The sidebar's archived section (web/app.js:4727-4739, web/archive.mjs) and a missing place opened by address (web/app.js:4882-4892); failure sets the section's failed state. Test: test/server-archive.test.js:216. |
| POST | `/api/conversations/<id>/restore` | none | `{ ok: true }`, then a list push (index.js:946-950). | 404 `Conversation not found`; 409 `A bench chat is changed only through its bench`; 404 `This chat is not archived` (server/store.js:1050-1066). | Restore (web/app.js:4749-4755, :6243, :6868); failure: showError. |
| POST | `/api/benches/<id>/restore` | none | `{ ok: true }`; the bench's queued rounds start and the list is pushed (index.js:952-956). | 404 `Bench not found`; 404 `This bench is not archived`. | Same. |
| DELETE | `/api/conversations/<id>` | none | `{ ok: true }`, then a list push (index.js:957-961). | 404 `Conversation not found`; 409 `A bench chat is changed only through its bench`; 409 `Archive it first` (server/store.js:1077-1080). | Delete for good (web/app.js:4774-4777, :6244). Test: test/server-archive.test.js:155. |
| DELETE | `/api/benches/<id>` | none | `{ ok: true }`, then a list push (index.js:962-966). | 404 `Bench not found`; 409 `Archive it first`. | Same. |
| GET | `/api/search` | `?scope=build` or `bench`, `&q=<text>` | `{ results: [{ id, snippet }] }`: chat ids for `build`, bench ids for `bench`; `snippet` is FTS text with `\u0001` and `\u0002` around matches and `…` at cuts, or null (server/store.js:1541-1570). | 400 `scope must be build or bench`; 400 `A search takes at most 16 different words`. | Sidebar search (web/app.js:5139-5149) and the command palette, both scopes (web/app.js:4936-4947); failure: showError. |
| GET | `/api/leaderboard` | none | `{ entries: [{ provider, model, effort, wins, votes, rating, benches }], votes }` (index.js:913-915, server/store.js:1410-1449): `rating` rounded Elo from 1000, `benches` a count, `votes` the total vote rows. | None. | The leaderboard place (web/app.js:5335-5338) reads `entries`; Settings' danger zone (web/app.js:679-681) reads `votes`. |
| DELETE | `/api/votes` | none | `{ ok: true }` (index.js:917-920). | None. | Danger zone Clear (web/app.js:7171-7178). |
| GET | `/api/shares` | none | `{ repo, builds }`; with no repository `{ repo: null, builds: [] }`. Each build is `{ shareId, title, kind, labels, url, publishedAt, conversationId, benchId, archived? }`: `kind` `chat` or `bench`, `publishedAt` ISO time of the last commit to its `build.json`, the source id null once deleted, `archived` present only while the source exists (index.js:971-987, server/share.js:341-364). | git or gh failures as 500, including `The shared build b/<id> was published in an older format: publish it again or remove it`. | Settings' published list, read each time Settings open (web/app.js:4229-4238, web/published.mjs:36-41, :79-83); a failure is shown in that list. Tests: test/share-routes.test.js:154, :285. |
| DELETE | `/api/shares/<shareId>` | none, `<shareId>` 12 hex digits | `{ ok: true }` (index.js:988-994). | 404 `This build is not shared` (also for a malformed id); 400 `Set the GitHub repository for shared builds in Settings`; 409 `Wait for the build to finish publishing`. | Remove in Settings' list (web/app.js:4245-4262); failure shown on that row. Test: test/share-routes.test.js:192. |
| GET | `/api/minecraft/status` | optional `?view=<key>`, a conversation id or `bench:<benchId>:<round>` | `{ minecraftVersion, yOffset, viewerUrl, mesher: { wasm, pack } \| null, viewerError, core, coreError, view }` (index.js:670-679, server/minecraft-live.js:71-82, server/core-wasm.js:71-77). `core` is the `/core/<version>-<hash>/` address or null; `coreError` is `The live world's engine could not be prepared: <last 600 chars>` or null. `view` is null without `?view`, else `{ key, version, mask, bounds, blockCount, layout, frame, syncError }` (server/live-views.js:63-70, :77-86): `layout` is null for a chat and the bench composer's layout for a round; `frame` a counter; `syncError` `Minecraft kept the previous build: <reason>` or null. | 404 `Conversation not found`, 404 `Bench not found`, 400 `Unknown round` for an unknown view (index.js:623-647); a build too tall with no earlier version throws its validation error. | The live world's poll, every 1000 ms, every 50 ms while catching up or woken (web/live-world.mjs:53-54, :272-291, :320-343); reads every field (web/live-world.mjs:194-221). A failure becomes the world's `pollError`, shown in the viewer state. The renderer also reads `mesher` and `viewerUrl` (web/viewer.js:631, :644). Test: test/live-views.test.js:24, :61, :79. |
| GET | `/api/minecraft/import` | `?view=<key>` and optional `&since=<version>` | `{ version, fresh, commands, blockCount }` (server/live-views.js:91-105). `commands` is text, one command per line ending in `\n`; `fresh` true means a placement from nothing (no `since`, or a version no longer kept, 16 kept per view); `fresh: false` with `commands: ""` means the page is current. | 400 `A view is needed`; the view errors above. | The live world's sync (web/live-world.mjs:417-427). Test: test/live-views.test.js:41. |

### The streams

There are four NDJSON streams and no server-sent events. Each line is one JSON object followed by `\n`. Every stream answers 200 with `content-type: application/x-ndjson`, `cache-control: no-cache`, `x-accel-buffering: no`.

#### The turn stream

Served by POST `/api/conversations/<id>/chat` (index.js:486, a new turn) and GET `/api/conversations/<id>/stream` (index.js:1288-1292, joining). Both are `TurnStream.attach` (server/turn-stream.js:22-32): it writes the head, then every line the turn has emitted so far, then each new line as it is emitted. The turn keeps all its lines in memory until it ends (server/turn-stream.js:34-41), and ends every attached response when its hold is released (server/turn-stream.js:43-49, :100-104). A client that disconnects does not stop the turn (index.js:121-125). Bench model turns use the same stream under the model's chat id (index.js:529-560).

| Event `type` | Fields | Emitted by | What the page does |
|---|---|---|---|
| `turn_snapshot` | `messages` (as GET `/api/conversations/<id>`), `messageTree` | index.js:317, first line of every turn | Replaces the items and tree, transcript `shown` (web/app.js:4642-4647). |
| `completion_retry` | `attempt` (1-based), `delayMs`, `error` (message) | server/completion-retry.js:24 via server/agent.js:633; API providers only, not harnesses | Drops live (uncommitted) items and `writing` tool calls (web/transcript.mjs:76-79). |
| `assistant_delta` | `text` | server/agent.js:651 | Appends to a live assistant item (web/transcript.mjs:80-81). |
| `reasoning_delta` | `text` | server/agent.js:652 | Appends to a live reasoning item (web/transcript.mjs:82-83). |
| `tool_call_delta` | `index`, `id`, `name`, `args` (the new argument text only) | server/agent.js:653; shapes from server/llm.js:452-459, :526, :548-555, server/codex.js:135, :140, server/harness.js:317, :416 | Creates or extends a `writing` tool item by `id`; streams `summary` and `program`/`command` out of the partial JSON (web/transcript.mjs:150-157, web/tool-stream.mjs:71-101). An event without `id` throws. |
| `harness_status` | `text`, a string or null to clear | server/agent.js:655, server/harness.js:323-326 (for example `Compacting context`, `Retrying after HTTP <n> (attempt <a> of <m>)`, `Summarizing context`) | Sets the ticker's status line (web/app.js:4648-4650). Not passed to `applyEvent`. |
| `notice` | `level` (`info`, `warning`, `error`), `text` | server/agent.js:656, server/harness.js:328-333 | Adds a notice item (web/transcript.mjs:115-116). |
| `plan` | `items: [{ text, status }]`, `status` `pending`, `active` or `done` (server/claude-code.js:136, server/codex-cli.js:411) | server/agent.js:657 | Replaces the turn's one plan item (web/transcript.mjs:117-122). |
| `reasoning` | `text`, the whole reasoning of one completion | server/agent.js:675-677 | Seals the live reasoning item with this text (web/transcript.mjs:86-87). |
| `assistant` | `text`, `final` | server/agent.js:680 (`final: true`, no tool calls), :685 (`final: false`, text before tool calls), index.js:341 (a stop) | Seals the live assistant item with this text and `final` (web/transcript.mjs:84-85, :140-145). |
| `tool_start` | `callId`, `name`, `summary`, `program` (the `program` or `command` argument, or the whole arguments for `test_build`, pretty-printed) | server/agent.js:714-720 | The `writing` item with that `callId` becomes `running`; none found throws `Tool call <id> started without being written` (web/transcript.mjs:90-96). |
| `tool_progress` | `callId`, `name`, `phase`, `message`, and by phase `tick`, `totalTicks`, `loadedColumns`, `totalColumns` | server/agent.js:725 from `test_build` (server/agent.js:451-454, server/minecraft-verify.js:306-563, :386-392); `message` joins each test's line as `Test <n>: <line>` separated by `; ` | Sets the running item's `progress` to `message` (web/transcript.mjs:97-99). |
| `tool_result` | `callId`, `name`, `ok`, `cancelled`, `error`, `diff`, `totalBlocks`, `output` (bash text; JSON text for `test_build` and `prompt_library`), `images` (array of `/api/images/...` URLs for a successful `see_build` or `view_image`) | server/agent.js:731-743; undefined fields are left out of the JSON | Sets `ok`, `failed` or `cancelled`, the diff, output and detail; appends a `shots` item for `images`; none running throws `Tool call <id> finished without running` (web/transcript.mjs:100-112). A successful `prompt_library` reloads the library; a successful `build` wakes the live world (web/app.js:4652-4653). |
| `done` | none | index.js:321 (finished), :342 (after a stop) | Clears the status; the page treats the turn as finished (web/app.js:4581, :4648-4650). |
| `error` | `message`, `messageId` (the stored error message's id; absent when saving the error failed) | index.js:344, :351 | Clears the status, adds an error item at once (not on the next frame), and the turn is finished (web/app.js:4648-4656, web/transcript.mjs:113-114). |

Any other `type` reaching `applyEvent` throws `Unknown turn event: <type>` (web/transcript.mjs:123-124). An event that throws, or a line that is not JSON, stops the page reading: the error goes to showError and the turn is `detached` until the list stops showing it running (web/app.js:4569-4595, :4615-4623).

A normal turn sends `turn_snapshot`, then for each model completion any `completion_retry` lines and the streamed deltas (`assistant_delta`, `reasoning_delta`, `tool_call_delta`, and for harnesses `harness_status`, `notice`, `plan`) in arrival order, then `reasoning` if the completion had reasoning. A completion with no tool calls is followed by `assistant` with `final: true`; one with tool calls by `assistant` with `final: false` if it had text, then for each call in order `tool_start`, any `tool_progress`, `tool_result`, and then the next completion. After the last completion the server saves the chat and sends `done`, then ends the response when the hold is released (index.js:316-321, :353-365, server/agent.js:631-799). A harness can stream the next call's `tool_call_delta` while an earlier call runs (web/transcript.mjs:147-149, server/harness.js:309-319).

A stopped turn (POST `/stop` aborts the turn's controller, index.js:1293-1296) sends whatever was under way, with any running tool's `tool_result` usually `ok: false, cancelled: true`, then `assistant` with `text: "(stopped by user)"` and `final: true`, then `done` (index.js:323-343). The stored message is `{ role: "assistant", content: "(stopped by user)", stopped: true, reply?, telemetry? }`, `reply` holding the text that had streamed (index.js:337-339). Test: test/server-turns.test.js:136, :331.

A failed turn sends what was under way, then `error` with `message` (the thrown error's text) and `messageId`, and no `done` (index.js:322-345). The stored message is `{ role: "error", content, telemetry? }`. If storing the end itself fails, the line is `error` with `message: "<message> (saving this failed: <reason>)"` and no `messageId` (index.js:346-352). A turn whose start is refused before acceptance has no stream; the POST answers JSON.

On reconnect or resume the client sends nothing but the GET (web/app.js:4549): no cursor, no last-seen line. The server replays every line of the turn from `turn_snapshot` on, then continues live (server/turn-stream.js:29-31). The snapshot is the transcript as it was when the turn started; the page rebuilds its items from it and re-applies every event (test/turn-stream.test.js:18 `refresh replays the snapshot, reasoning, and partial tool input exactly once`, :48 `replay captures the original baseline even as the stored transcript changes`, :61). A 404 `No turn is running` means the turn ended; the page reads GET `/api/conversations/<id>` (web/app.js:4554, :4599-4610). When the page's own stream drops without `done` or `error`, it waits 1 s and resyncs, and the list's `runningSince` makes it join again (web/app.js:4589, :4628-4637, :4060-4063). The page joins a turn when its list row has `runningSince` that is not the start of the turn it last saw end (web/transcript.mjs:46-49).

#### The chat list stream

GET `/api/conversations/stream` (index.js:884-890). The first line is sent at once; each later line is sent whole by `publishConversations` (index.js:189-193) after a turn starts (index.js:485) or is released (index.js:300-303), a title is set or fails (index.js:267, :274, :296), a chat is seen, archived, restored, deleted or forked, a bench is created, advanced, revealed, archived, restored or deleted, and after the daily purge (index.js:73). Every line is the same shape (index.js:179-187):

`{ conversations, benches, promptTitles, archived }`. `conversations` is every listed chat, newest first, non-empty and not archived, bench models included: `{ id, title, provider, model, effort, resolution, createdAt, benchId, benchSlot, unread, messageCount, prompts, runningSince }`, and for a bench model also `round` (prompts sent on its line) and `halted` (`stopped`, `failed` or null; null while it runs) (index.js:171-177, :521-524, server/store.js:1171-1187, :1280-1297). `createdAt` is ISO; `runningSince` epoch ms or null; `prompts` the library prompt ids any message came from. `benches` is every listed bench: `{ id, title, createdAt, head, resolution, rounds, prompts, blind, revealed, order }`, `rounds` a count (server/store.js:1572-1590). `promptTitles` maps a library prompt id to the title it was last sent under (server/store.js:1190-1204). `archived` is `{ conversations, benches }` counts (index.js:185).

The page holds one such stream (web/app.js:3904-3975). It lets go of it while the tab is hidden and reconnects when shown; after any end it waits 1 s and reconnects; the first line of each connection replaces everything. A failed request shows once per outage (`The chat list failed to load (HTTP <status>)`); a line that cannot be applied goes to showError and reading continues. On shutdown the server ends every list stream (index.js:1478). Tests: test/server-archive.test.js:247; test/server-turns.test.js:276.

#### The publish stream

POST `/api/conversations/<id>/share` and POST `/api/benches/<id>/share` (index.js:656-660, :1097, :1159). Lines are `{ state }` in this order: `exporting`, `publishing`, `waiting`, then `{ state: "live", url }`; or `{ state: "failed", error }` from any of the first three (server/share.js:84-104, :262-301). The head is written with the first line, which is sent before any work (server/share.js:93-97). The publish never rejects once it has started, so the stream always ends with `live` or `failed` unless the connection breaks. A publish continues if the page goes (index.js:1148-1149). The page applies each line to the share's state (web/share.mjs:61-66) and shows the step text `Preparing the build…`, `Uploading…`, `Waiting for GitHub Pages…` (web/share.mjs:84-93); a stream that ends early becomes `failed` with `The publish stopped before the page was live` (web/share.mjs:71-75). There is no resume: a second POST during a publish is 409 `This build is already being published`. Tests: test/share-routes.test.js:120 (exact lines `[{ state: 'exporting' }, { state: 'publishing' }, { state: 'waiting' }, { state: 'live', url }]`), :139, :251.

#### The world stream

POST `/api/conversations/<id>/world` (index.js:1185-1234), used by the renderer only. The client sends `known` (chunk key to the version it holds), `renderLod` (the one it holds), `clip` (`{ from, to }` or absent) and `tick` (absent for the current tick). A `renderLod` different from the world's discards `known`. The first line is `{ type: "meta", revision, blockCount, bounds, renderLod, removed, entities }`: `removed` the known keys the world no longer has, `renderLod` null or `{ axis, scale: 4 }` (server/world.js:543-550), `entities` the engine's entities at that tick, inside the clip, or null (server/shot-world.js:324-331). Then one `{ type: "chunk", chunk }` per chunk whose shown version differs, `chunk` being `{ key, origin, version, palette: [{ id, states, state }], blocks }` (server/world.js:559-643). The writer waits for `drain` between lines and stops when the client closes (index.js:1207-1232). The renderer keeps `known` between calls (web/shot.mjs:13-18, :79-121). There is no resume beyond sending `known` again.

### Workarounds, defects and ambiguities

A resumed turn replays the whole turn from its first line, deltas included, and the server holds every line of a running turn in memory until it ends (server/turn-stream.js:13, :37). There is no event id or offset, so a client cannot ask for "after line N".

A stop replaces what the page streamed. The stop's `assistant` event has `text: "(stopped by user)"`, and `finalize` overwrites the live assistant item's text with it (web/transcript.mjs:140-145). The streamed text is kept only in the stored message's `reply` field, which `itemsFromMessages` does not read (web/app.js:356-382), so neither the live view nor a reload shows it. The comment at index.js:334-336 says `reply` exists for a harness session's next turn.

A failed turn sends `error` and no `done`; a stopped turn sends `done`. The page treats either as the end (web/app.js:4581). An `error` without `messageId` (the save of the end failed) leaves an error item the page cannot dismiss on the server.

On an edit (`editOf`), the edited prompt is stored before `sandbox.reset` runs (index.js:476-480). If the reset throws, the POST answers a JSON error but the prompt stays stored with no turn after it; it is ended as failed with `How this turn ended was not saved` before the chat's next prompt (index.js:471, server/store.js:1324-1336).

POST `/stop` answers `{ ok: true }` for any id, running or not, existing or not (index.js:1293-1296). POST `/seen` answers `{ ok: true }` for any id (index.js:1173-1177).

The 409 on a second chat POST (`A turn is already running for this conversation`) is answered before the request body is read (index.js:440, :443).

Several validation failures are plain `Error`s and so answer 500 rather than 400: the settings save errors (server/settings.js:113, :140-141, :147), `favorites must be an array` (server/settings.js:152), the prompt generator's `Choose a model` and `Write something to improve first` (server/prompt-generation.js:17-19), and a failed logout command (server/provider-login.js:245).

The share removals differ in their answer: DELETE `/api/conversations/<id>/share` and `/api/benches/<id>/share` answer `{}` (index.js:1102, :1164), DELETE `/api/shares/<shareId>` answers `{ ok: true }` (index.js:993). Both chat and bench DELETE answer 404 `This build is not shared` when no repository is set, while `/api/shares/<shareId>` answers 400 `Set the GitHub repository for shared builds in Settings` (index.js:991).

GET `/api/provider-logins/<provider>` and GET `/api/cli-installs/<program>` answer the JSON literal `null` when nothing was started (server/provider-login.js:212, server/cli-install.js:54-57). The page stores it as the state (web/app.js:944-945).

POST `/api/benches/<id>/retry` uses `Only a failed or stopped model can be retried` both for a model that is not halted and for one that is held by another edit (index.js:1076-1077).

The PUT `/api/settings` body is the page's whole Settings draft (web/app.js:4405-4409), including read-only fields such as `summary`, `runtime`, `signIn`, `checkedAt` and `modelFavorites`; the server reads only `providers.<id>.{ apiKey, enabled, hiddenModels, modelOrder }`, `titleModel` and `shareRepo` (server/settings.js:124-135). A key field left empty keeps the saved key, so a key cannot be cleared to empty from this route.

The `/api/setup` comment at index.js:702-714 lists the steps `jdk, rust, minecraft, tables, blocks, viewer, engine, wasm, browser, shell`; the code's `STEPS` has `jdk, minecraft, tables, blocks, viewer, engine, browser, shell` (server/setup.js:25-34). The code is what is answered.

`/api/assets/<name>` answers any method, not only GET (index.js:822). `/api/minecraft/import` turns a non-numeric `since` into `NaN`, which matches no kept version, so the answer is a fresh placement (index.js:687, server/live-views.js:95).

The page reads every API answer with `res.json()` before checking the status (web/app.js:26-31), so the plain-text 403 from `foreignRequest` and the HTML 401 reach the user as a JSON parse error. The turn join (web/app.js:4556) and the live world (web/live-host.mjs:64-73) tolerate a non-JSON error body; `publishShare` (web/app.js:4340) and `send` (web/app.js:5534) do not.

The world stream, the `/api/assets/*` files, `/api/block-states` and `/api/block-tints` serve the renderer, not the page; the renderer authenticates with the bearer header (server/shot.js:79). `/api/activity` serves the hosted gateway and nothing in `web/` calls it.

Routes: 65 under `/api/` (plus the `/api/` fallback 404 `Not found`, index.js:1313) and 8 outside it, 73 in all. Streams: 4 (turn, chat list, publish, world), all NDJSON, none SSE.

## Checklist

| Area | Covered |
|---|---|
| `web/style.css` lines 1-664 (the first layer) | covered |
| `web/style.css` lines 666-2752 (the dark-first layer) | covered |
| `web/style.css` lines 2753-4195 (the T3 Code patterns and later additions) | covered |
| Every one of the 402 classes the stylesheet styles, indexed in 1.23 | covered |
| `web/theme.js`, `web/index.html` | covered |
| `web/viewer.js` | covered (the headless `see_build` renderer, not the pane; see 2.1) |
| `web/bluemap-bridge.js` | covered |
| `web/bluemap-view.mjs` | covered |
| `web/layers.mjs` | covered |
| `web/live-world.mjs` | covered |
| `web/live-host.mjs` | covered |
| `test/live-world-states.test.js`, 27 cases | covered |
| `server/index.js` routes: 73 (65 under `/api/` plus 8 outside it) and 4 NDJSON streams | covered |
