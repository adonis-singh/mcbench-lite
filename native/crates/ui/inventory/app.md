# Inventory: `web/app.js` from `App` (line 3406) to the end, and the page's state modules

This file records what the browser page does from `function App()` (web/app.js line 3406) to the end of web/app.js (line 7241), plus the modules `App` uses for its state and flows. Line numbers are web/app.js unless a file is named. Module line numbers are given as `file:line`. The components defined before line 3406 (pickers, settings panes, prompt library pieces, the composer input, share panel, viewer component) are covered by another agent's inventory; here only what `App` passes them and when they are shown is recorded. `style.css`, the viewer's own files and the server's routes are covered by a third agent.

Source revision: branch `s4-inventory` as of 2026-11-17 (web/app.js is 7,241 lines).

## 0. Page shell: `index.html`, `theme.js`, `page-errors.js`, the mount

`web/index.html` (19 lines) has the title `mcbench-lite`, the icon `/icons/block.svg`, the stylesheet `/style.css`, then two classic scripts in the head: `/theme.js` and `/page-errors.js`. The body holds `<div id="app">` with the text `Loading mcbench-lite…` (shown until the app mounts and replaces it), a `#page-error` banner (`role="alert"`, hidden, with a `.page-error-text` span and a `Reload` button) and the module script `/app.js`.

`web/theme.js` (3 lines) sets `document.documentElement.dataset.theme` to `localStorage['mcb-theme']` or `'dark'` before the first paint.

`web/page-errors.js` (25 lines) catches failures nothing else catches and writes them into `#page-error`, outside the app, so they show even if the app never rendered. Only the first failure is shown (`if (!sink.hidden) return`); later ones are ignored. The `Reload` button calls `location.reload()`. A window `error` event in the capture phase that is an `ErrorEvent` shows `The page hit an error: ${event.error?.message || event.message}`; one whose target is a `<script>` (a script or a module it imports failed to load) shows `The page could not load ${pathname} or a module it imports.`; other resources that fail (an image) are ignored. An `unhandledrejection` shows `The page hit an error: ${event.reason?.message || event.reason}`. `test/page-errors.test.js` states these.


## 1. State

Each row: name, type and initial value, what sets it, what reads it, and whether it survives a reload. "Ref" rows are `useRef`; "module" rows are module-level variables.

| Line | Name | Type, initial | Set by | Read by | Survives reload |
|---|---|---|---|---|---|
| 3399 | (module listener) `animationstart` on document | n/a | n/a | every running dot's `conv-pulse` animation is moved to `startTime = 0`, so all running dots pulse in step whenever each began | n/a |
| 3407 | `conversations` | array of chat rows, `[]` | `receiveConversations` (list stream), optimistic edits (rename, archive, create) | sidebar, header, bench grid, `exportName`, share blocking, `firstBuild` | server (list stream) |
| 3410 | `items` / `itemsRef` | array of transcript items, `[]` | `setItems` (immediate), `setItemsNextFrame` (stream events, batched to one draw per animation frame while events keep coming), `drawItems` (flushes a held change) | transcript, autoscroll effect | server (transcript fetch) |
| 3415 | `itemsFrameRef` | rAF id, `0` | `itemsFrame` | `setItemsNextFrame` | no |
| 3416 | `itemsHeldRef` | bool, `false` | `setItemsNextFrame`, `setItems` | `itemsFrame`, `drawItems` | no |
| 3446 | `messageTree` | `{ headId: null, messages: [] }` | `showTranscript`, turn stream | `branches` (branch navigation, line 3742) | server |
| 3447 | `provider` | string, `initialModelSelection.provider` (from `mcb-last-model`, defined before App) | `setModelChoice`, the picker | composer, send, `askModel`, `firstBuild` | `localStorage` `mcb-last-model` `{ provider, model }`, written only when chosen in the composer's picker (6468) |
| 3448 | `model` | string, `initialModelSelection.model` | same | same | same |
| 3449 | `effort` | string, `''` | `setModelChoice`, effort picker | send | per chat, server row |
| 3453 | `resolution` | resolution value, `storedResolution('mcb-last-resolution')` | `chooseResolution` (also writes `localStorage['mcb-last-resolution']` as JSON), `setModelChoice` | send, resolution picker | `localStorage` `mcb-last-resolution` (new chats); per chat from the server row |
| 3458 | `modelCaps` | `useModelInfo(provider, model)` | the model info cache | effort and image controls | no (fetched) |
| 3465 | `providers` | object, `{}` | `loadConfig`, `recheckSettings`, `saveSettings` | `noAI`, `firstPane`, pickers | server (`/api/config`) |
| 3468 | `config` | `{ phase: 'loading'|'loaded'|'failed', error }`, loading | `loadConfig` | `noAI`, `firstBuild`, empty screens | no |
| 3471 | `hosted` | bool, `false` | `loadConfig` (`data.hosted === true`) | Settings (an `/account` sign-out link for the hosted gateway) | server |
| 3474 | `noAI` (derived) | bool | `config.phase === 'loaded'` and no provider `usable` | empty screens, sidebar, pickers point to Connect an AI | derived |
| 3478 | `favoriteSyncRef` | `Favorites` instance (favorites.mjs) | created once; `write` PUTs `/api/model-favorites`; `onChange` sets `favorites`; `onError` shows `The star was not saved, and went back: ${message}` | `toggleFavorite`, `loadConfig` (`load(data.modelFavorites)`) | server |
| 3486 | `favorites` | array of keys, `[]` | Favorites `onChange` | pickers | server |
| 3488 | `turn` / `turnRef` | turn object (transcript.mjs), `IDLE` | `setTurn` (every change goes through `turnTo`'s phase table and throws on an illegal one) | `busy`, join effect, stream code | no |
| 3496 | `stopAskedRef` | AbortController or null | Stop pressed before the server took the send | send flow | no |
| 3499 | `hasText` | bool, `false` | `setComposer`, composer input | send button enablement | no |
| 3500 | `attachments` | array, `[]` | file picker, paste, drop, `applyDraft` | composer chips, send | drafts (in memory only, see `draftsRef`) |
| 3501 | `reading` | array of files still being read, `[]` | attachment reading, `applyDraft` | composer (reading state), send blocking | no |
| 3504 | `edit` | `{...}` or null: the sent prompt or round the draft is a new version of, with what the composer held before (draft.mjs `beginEdit`) | edit actions, `applyDraft`, `cancelEdit` | composer editing banner, Escape, send | no |
| 3505 | `copied` | message item or null | Copy action | the copied tick on that message | no |
| 3506 | `minecraftLive` / `minecraftLiveRef` | `{ status: 'idle', conversationId: null, address: null, error: null, syncError: null, viewerUrl: null, viewerRevision: null, mesher: null }` | `LiveWorld.subscribe` (line 4074) | viewer, share subject, live controls | no (rebuilt from the server's build) |
| 3510 | `lightbox` | image or null | image clicks | lightbox layer, Escape | no |
| 3513 | `command` | `{ query, active }` or null (Mod+K palette; `active` is the highlighted item's key, command.mjs) | palette open, typing, keys | palette, Escape | no |
| 3516 | `panel` / `panelRef` | Settings panel phase (settings-panel.mjs), `CLOSED`; phases closed, opening, open, saving | `setPanel` (through `panelTo`'s phase table; throws on an illegal change) | Settings, `settings` (the draft, `panelDraft(panel)`) | no |
| 3519 | `signInStart` | `{ id, method }` or null | Connect an AI asking for a sign-in | the provider pane, which clears it once started | no |
| 3531 | `shares` / `sharesRef` | object keyed by `shareKey(subject)`, `{}` | `setShare` | `shareOf(subject)` (default `SHARE_UNKNOWN`), header Share button, Share panel | no (read from server at first opening) |
| 3541 | `shareFor` | shareKey or null | `openShare`, `closeShare` | which Share panel is open | no |
| 3542 | `shareRepo` | string or `undefined` until the first read | `openShare` (reads `/api/settings` each opening) | Share panel | server |
| 3548 | `published` / `publishedRef` | published.mjs state, `PUBLISHED_UNREAD` | `loadPublished`, `removePublished`, Settings closing (`publishedClosed`) | Settings' published list | no |
| 3550 | `publishedReadsRef` | counter, `0` | each read and each close | only the latest read lands, none after close | no |
| 3557 | `checkRef` | counter, `0` | `recheckSettings`, `saveSettings`, `readSettings` | only the latest "Check again" lands | no |
| 3558 | `titleModelCaps` | `useModelInfo` of `settings.titleModel` | n/a | Settings' title model effort | no |
| 3559 | `theme` | `'dark'|'light'`, `localStorage['mcb-theme'] || 'dark'` | theme toggle | effect at 4145 sets `data-theme` and writes `mcb-theme` | `localStorage` `mcb-theme` |
| 3560 | `sidebarOpen` | bool, `localStorage['mcb-sidebar'] !== 'closed'` | `toggleSidebar` (wide), `startSidebarDrag` | layout | `localStorage` `mcb-sidebar` (`open`/`closed`) |
| 3563 | `narrow` | `useMedia(NARROW)` | window width media query | layout, drawer | no |
| 3564 | `drawerOpen` | bool, `false` | `toggleSidebar` (narrow), Escape, navigation | `drawer = narrow && drawerOpen` | no (never kept) |
| 3567 | `chatWidth` | number or null, `Number(localStorage['mcb-chat-width']) || null` | `startDividerDrag` | chat column width | `localStorage` `mcb-chat-width` |
| 3568 | `sidebarWidth` | number, `Number(localStorage['mcb-sidebar-width']) || SIDEBAR.width` | `startSidebarDrag` | sidebar width | `localStorage` `mcb-sidebar-width` |
| 3569 | `viewerFull` | bool, `false` | full-screen button, Escape | layout, Escape | no |
| 3576 | `place` / `placeRef` | place (place.mjs), `startPlace(addressPlace(location.pathname), localStorage['mcb-app-mode'])` | `setPlace` only (pushes or replaces history, see section 10) | everything: `placeKeys(place, benchRounds)` gives `appMode`, `activeId` (chat id), `viewKey` (world), `draftKey` | address, plus `localStorage` `mcb-app-mode` |
| 3580 | `leftRef` | `{ build: { kind: 'new-chat', id: null }, bench: { kind: 'new-bench' } }` | `setPlace` (each place not `missing` is remembered for its mode) | mode switch returns to the mode's last place | no |
| 3581 | `benches` / `benchesRef` | array, `[]` | `receiveBenches` | sidebar, bench views | server (list stream) |
| 3583 | `benchRounds` / `benchRoundsRef` | `{ benchId: round }`, `{}` | `showRound` | `placeKeys` (which round's world and draft) | no |
| 3591 | `replaceAddressRef` | bool, `false` | `replacingAddress(fn)` sets it around a navigation | `setPlace` chooses `replaceState` over `pushState` | no |
| 3614 | `benchDetails` / `benchDetailsRef` | `{ benchId: details }` (bench-details.mjs: loading, loaded or failed), `{}` | `setBenchDetails` | bench details view | no |
| 3621 | `detailsRef` | `{ next: 0, latest: {} }` | each details read is numbered | only the latest read per bench lands | no |
| 3624 | `archive` / `archiveRef` | archive.mjs state, `ARCHIVE_CLOSED` | archive open/read/close | sidebar archived section | no |
| 3626 | `archiveReadsRef` | counter | each read | only the latest lands, none after close | no |
| 3633 | `archivedCounts` | `{ conversations: 0, benches: 0 }` | list stream (`archived`) | archived section shows only for a mode with some | server |
| 3635 | `restoredRef` | restored item or null | Restore | `openRestored` opens it once the list stream brings it back | no |
| 3637 | `listedRef` | bool, `false` | first list | `openAddress` can run | no |
| 3641 | `binds` | shortcut map, `{ ...DEFAULT_BINDS, ...storedJson('mcb-binds', {}) }` (actions added since the save start on their defaults) | Settings' shortcuts pane | key handlers | `localStorage` `mcb-binds` (written by effect at 3642 on every change) |
| 3643 | `find` | string, `''` | sidebar search box | `found` effect | no |
| 3646 | `filters` | filters.mjs, `NO_FILTERS` | filter popover | sidebar lists | no |
| 3647 | `promptTitles` | `{}` | list stream (`promptTitles`) | filters (the title each prompt a listed row came from was last sent under) | server |
| 3648 | `filterButtonRef` | DOM ref | n/a | filter popover anchor | no |
| 3649 | `found` | server's ranked matches for the current mode, or null while nothing is typed | search effect | sidebar | no |
| 3650 | `findRef` | DOM ref of the search box | n/a | focus | no |
| 3651 | `openBenches` | Set of bench ids expanded in the sidebar, `new Set(storedJson('mcb-open-benches', []))` | expand/collapse, `receiveBenches` (drops gone ones; the first list drops any no longer listed) | sidebar | `localStorage` `mcb-open-benches` |
| 3652 | `benchModels` | array of model choices, `storedJson('mcb-bench-models', [])` | new-bench picker | new bench | `localStorage` `mcb-bench-models` |
| 3653 | `benchBlind` | bool, `storedJson('mcb-bench-blind', true)` | blind toggle | new bench | `localStorage` `mcb-bench-blind` (effect 3656) |
| 3655 | `benchResolution` | `storedResolution('mcb-bench-resolution')` | new-bench resolution picker | new bench | `localStorage` `mcb-bench-resolution` (effect 3657) |
| 3658 | `leaderboard` | null or the leaderboard data | leaderboard read | leaderboard view | no |
| 3659 | `viewerFocus` | null or a focus request | e.g. clicking a selection chip | viewer | no |
| 3660 | `selectToggle` | counter, `0` | the select shortcut | viewer toggles its selection tool on each bump | no |
| 3663 | `promptSyncRef` | `PromptSync` (prompt-sync.mjs): `read` GETs `/api/prompts`, `write` PUTs `/api/prompts` `{ prompts }`, `onChange` sets `promptLibrary`, `onError` shows the error | created once; `load()` on mount (effect 3680) | prompt library | server |
| 3672 | `promptLibrary` | `{ list, phase, ... }`, `promptSyncRef.current.snapshot()` | PromptSync | `prompts` (`list`), `promptsReady` (`promptsLoaded(phase)`); `setPrompts(edit)` throws before the library loaded | server |
| 3681 | `promptsActiveId` | id or null | prompt list clicks | `shownPromptId`: the active id if it is in `editorPrompts`, else the first prompt's id, else null | no |
| 3685 | `newPrompt` / `newPromptRef` | prompt or null: made with New prompt, open in the editor, not in the library until something is written in it; left untouched it is never saved | New prompt, `updatePrompt` | `editorPrompts = newPrompt ? [...prompts, newPrompt] : prompts` | no |
| 3692 | `messagesRef` | DOM ref of the transcript scroller | n/a | autoscroll | no |
| 3693 | `fileInputRef` | DOM ref of the hidden file input | n/a | attach button | no |
| 3696 | `navigationRef` | counter, `0` | bumped by every navigation (`leaveConversation`) | async work checks it after each await and lands nowhere it no longer belongs | no |
| 3697 | `conversationsRef` | mirror of `conversations` | `receiveConversations` | handlers | no |
| 3703 | `draftsRef` | `Drafts` (draft.mjs) | created once; parks and restores drafts by draft key | `arrive`, `leaveConversation`, list stream (drops drafts of gone chats and benches) | no (in memory only; a reload loses every unsent draft) |
| 3705 | `draftRef` | `{ attachments, selections, reading, choice: { provider, model, effort, resolution }, edit }`, rewritten every render | each render | `currentDraft()` adds `segments: segmentsRef.current` | no |
| 3706 | `selections` | blocks picked in the viewer for this message, `[]` | viewer selection, chips, `applyDraft` | composer chips, send | no |
| 3707 | `hoveredSelection` | null | chip hover | viewer highlights that selection | no |
| 3710 | `composerValue` | `{ segments: [], version: 0 }`; `version` changes only when the app rewrites the content | `setComposer` | composer input | no |
| 3711 | `segmentsRef` | segments, `[]` | `setComposer`, typing | `currentDraft`, send | no |
| 3731 | `transcriptsRef` | `Map` id to request promise, `.data` once it lands | `fetchTranscript`, `forgetError`, list stream (deletes stale entries) | `openConversation` shows a cached copy at once; hovering a row prefetches | no |
| 3733 | `listReady` | bool, `false` | first list | composer (the model is known) | no |
| 3736 | `transcriptState` / `transcriptStateRef` | transcript.mjs `LOADING`, `SHOWN` or `readFailed(error)` | `setTranscriptState` | transcript pane | no |
| 3742 | `branches` (memo) | `branchNavigation(messageTree)` | n/a | branch counters on messages | derived |
| 3747 | `notices` | `[{ id, text }]`, `[]` | `showError` (one per distinct text: an older notice with the same text is replaced by a new one at the end), dismiss | the notice stack at the top of the chat column | no |
| 3748 | `noticeIdRef` | counter | `showError` | notice ids | no |
| 3757 | `exportName` (derived) | string: `mcbench world` with no world; for a bench world `${bench title or 'Bench'} round ${round}`; else the chat's title or `mcbench world` | n/a | viewer's export file name | derived |
| 3793 | `itemsByTranscript` | `WeakMap` fetched transcript to items | `showTranscript` | items are built once per fetched transcript, so a chat shown again keeps its item objects | no |
| 3999 | `setup` | `/api/setup` view or null until the first read | setup poll | `preparing = setup && !setup.ready ? setupStatus(setup) : null`, `firstBuild`, held sends | server |
| 4015 | `held` | draft key or null: a send made before the app is ready | `submit` | effect 4016 sends it once ready, only if that draft is still on screen | no |
| 4025 | `askModel` | bool, `false` | effect 4028 | opens the chat model picker by itself | no |
| 4026 | `askedModelRef` | bool | picker | the picker open now was opened by itself | no |
| 4027 | `settingsWereOpenRef` | bool | effect 4028 | detects Settings closing | no |
| 4037 | `firstBuild` (derived) | `setup.ready && config loaded && !noAI && provider && model && conversations.length === 0 && place.kind === 'new-chat' && Settings closed` | n/a | effect 4039 focuses the composer whenever it becomes true | derived |
| 4045 | `activeRow` (derived) | the listed row of the chat on screen, or null | n/a | seen, busy, join | derived |
| 4046 | `pageVisible` | `document.visibilityState === 'visible'` | `visibilitychange` | mark seen | no |
| 4057 | `busy` (derived) | `turnRunning(turn, activeRow)`: the page holds a turn's stream or send, or the server runs one | n/a | composer (Stop vs Send), autoscroll | derived |
| 4069 | `liveWorldRef` | `new LiveWorld(browserHost())` (live-world.mjs, live-host.mjs): the tab's own engine in a worker; nothing shared with other tabs | created once | viewer status, `show(viewKey)` | no |
| 4092 | `prevScrollHeightRef` | number, `0` | autoscroll effect | autoscroll | no |
| 4104 | `settingsOpen` (derived) | `Boolean(settings)` | n/a | layers, published, clock | derived |
| 4108 | `shareSubject` (derived) | `{ kind: 'chat', id }` in Build when the world on show is viewable, belongs to this chat and has `blockCount > 0`; `{ kind: 'bench', id }` on a bench's comparison; else null | n/a | header Share button | derived |
| 4112 | `shareShown` (derived) | the subject's panel is the one open | n/a | layers | derived |
| 4114 | `shareBlocked` (derived) | a bench subject with any of its chats running (`runningSince`) | n/a | Share panel (cannot be shared yet) | derived |
| 4115 | `layers` / `layersRef` | `{ command, lightbox, settings, drawer, viewer, share }` booleans | each render | Escape (`topLayer`, layers.mjs) | derived |
| 4265 | settings clock | counter | `setInterval` 15000 ms while Settings are open | keeps `Checked … ago` current | no |
| 4277 | `openingRef` | the `readSettings()` promise an opening waits for | `openSettings` | `openSettings` | no |
| 4670 | `newChatRef` | `{ request, id }` or null: the new chat's server row, `id` once made | `createdChat`, `settleNewChat` | `showNewChat`, `send`, list stream | no (a reload makes another row on the next need; an empty row never shows in the list) |
| 4855 | `openAddressRef` | latest `openAddress` | each render | `popstate` | no |
| 4905 | `bindActionsRef` | the six bind actions, rewritten every render | each render | `runBind` | no |
| 4914 | `bindsRef` | mirror of `binds` | each render | `runBind` | no |
| 4932 | `commandFound` | `{ build, bench }` server results or null | palette search effect | palette | no |
| 4933 | `commandInputRef`, `commandListRef` | DOM refs | n/a | focus, scroll into view | no |
| 4951 | `commandShownRef` | the key highlighted as last rendered | `renderCommand` | effect keeping `command.active` on it | no |
| 4137 | `closeTopLayerRef` | latest `closeTopLayer` | each render | Escape listener | no |
| 5206 | `forking` | bool, `false` | `forkToBuild` | Fork to Build disabled | no |
| 5313 | `branching` | bool, `false` | `switchBranch`, `switchRoundBranch` | `changing`; branch arrows disabled | no |
| 5590 | `palette` | `{ query, index, slash, withSearch }` or null | slash typing, prompts button, keys | `PromptPalette` | no |
| 5591 | `composerRef`, `composerBoxRef` | DOM refs of the composer's editable root and its box | n/a | focus, reading, palette outside-click | no |
| 5593 | `caretRef` | the last caret range in the composer | `onCaret` | `insertPrompt` | no |
| 5597 | `caretAtEnd` | counter, `0` | edit and cancel | layout effect puts the caret at the end | no |
| 5822 | `entryVnodes` | caches of rendered entries | transcript memo | rendering speed only | no |
| 6119 | `typeAheadRef`, `typeKeyRef` | whether type-ahead applies; the handler | each render | window key listener, viewer `onType` | no |
| 6167 | `dotHistory` | `Map` dot key to `{ state, arrived }` | `dotClass` | the dot's arrival easing | no |
| IndexedDB | `mcbench-placements` (live-host.mjs) | each view's last fresh placement | `LiveWorld` | a provisional picture on the next page load | yes (IndexedDB, up to 256 MiB) |

## 2. Effects and streams

Index of every effect in the range (42: 40 `useEffect`, 2 `useLayoutEffect`), each described where the section column points.

| Line | Runs on | Does | Section |
|---|---|---|---|
| 3642 | `binds` | writes `mcb-binds` | 2.12 |
| 3656 | `benchBlind` | writes `mcb-bench-blind` | 2.12 |
| 3657 | `benchResolution` | writes `mcb-bench-resolution` | 2.12 |
| 3680 | mount | loads the prompt library | 8.7 |
| 3904 | mount | the list stream, with reconnect and visibility handling | 2.1 |
| 3994 | mount | reads the config | 2.2 |
| 4000 | mount | setup poll every 1000 ms until ready | 2.3 |
| 4016 | `held`, `setup.ready` | sends a held draft once ready | 2.4 |
| 4028 | `panel.phase` | opens the chat model picker after Settings close with no model | 2.5 |
| 4039 | `firstBuild` | focuses the composer | 2.6 |
| 4047 | mount | follows `visibilitychange` into `pageVisible` | 2.7 |
| 4053 | active row id, its `unread`, `pageVisible` | marks the chat seen | 2.7 |
| 4060 | active row id, its `runningSince`, `turn` | joins a turn the page does not read | 2.8 |
| 4072 | mount | subscribes to the live world, `begin`, `dispose` on unmount | 2.9 |
| 4084 | `viewKey` | `LiveWorld.show(viewKey)` | 2.9 |
| 4094 | `items`, `busy`, `appMode` | sticky autoscroll | 2.10, 5.8 |
| 4139 | mount | window Escape listener | 11.5 |
| 4145 | `theme` | sets `data-theme`, writes `mcb-theme` | 2.12 |
| 4150 | `appMode` | writes `mcb-app-mode` | 2.12 |
| 4154 | `sidebarOpen` | writes `mcb-sidebar` | 2.12 |
| 4158 | `sidebarWidth` | writes `mcb-sidebar-width` | 2.12 |
| 4222 | `settingsOpen` | lets go of the published list when Settings close | 2.13 |
| 4266 | `settingsOpen` | 15 s clock while Settings are open | 2.14 |
| 4621 | active row's `runningSince`, `turn` | ends a detached turn once the list stops showing it | 2.15 |
| 4858 | mount | `popstate`: Back and Forward | 10.1 |
| 4923 | mount | window key listener for the binds | 11.1 |
| 4936 | palette opening (layout effect) | focuses the palette input before paint | 4.2 |
| 4937 | palette query | server search after 90 ms | 4.2 |
| 4952 | every render | keeps `command.active` on the rendered highlight | 4.2 |
| 4955 | `command.active` | scrolls the highlighted item into view | 4.2 |
| 5140 | `find`, `appMode` | sidebar server search after 90 ms | 4.3 |
| 5152 | `appMode` | clears the search text and filters | 4.3 |
| 5331 | viewed bench id, rounds, head, revealed, members' state | reads the bench's details | 7.7 |
| 5335 | `place` | reads the leaderboard on the leaderboard | 7.7 |
| 5341 | `place`, members' unread, `pageVisible` | marks every unread member seen on the comparison | 7.7 |
| 5359 | `openBenches` | writes `mcb-open-benches` | 7.7 |
| 5360 | `benchModels` | writes `mcb-bench-models` | 7.7 |
| 5598 | `caretAtEnd` (layout effect) | focuses the composer, caret at the end | 6.5 |
| 5607 | palette open | `mousedown` outside the composer box closes the palette | 6.5 |
| 6102 | `place`, active prompt, Settings, palette | closes the narrow drawer | 3.3 |
| 6111 | `placeTitle` | sets `document.title` | 10.2 |
| 6135 | mount | window key listener for type-ahead | 11.3 |

Other subscriptions and timers: `requestAnimationFrame` batching of streamed items (3422 to 3441); the 1000 ms `setTimeout` in `lostStream` (4632); the 1200 ms copied tick (5561); the 1400 ms setting-row flash (4967); the 90 ms debounce timers above; the module-level `animationstart` listener (3399); the live world's own poll (9.3).

### 2.1 The list stream (lines 3904 to 3975)

One effect on mount reads `GET /api/conversations/stream`, a newline-delimited JSON stream (`readLines`). Each line is a whole set of lists `{ conversations, benches, promptTitles, archived }`, pushed by the server whenever something the sidebar shows changes (a turn starting or ending, a title, a chat seen or archived). There is no event type: every message replaces the lists. For each message (`receiveLists`, 3919): `receiveConversations(rows)`, `receiveBenches(benchRows, first)`, `setPromptTitles(titles)`, `receiveArchivedCounts(archived)`, `openRestored()`. On the first message only: `listedRef = true`, `listReady = true`, and if the user has not navigated yet (`navigationRef.current === 0`) the page opens what its address names, replacing the history entry (`openAddress(addressPlace(location.pathname))`).

A message that cannot be applied (an exception in `receiveLists`) is logged, shown as a notice, and the stream reads on. When the first message of a connection arrives and the transcript on screen is in the failed phase, the transcript is read again (`readAgain`): a read that failed while the server was away is retried once it is back.

Reconnect: a loop runs while the effect lives. A hidden page aborts its stream (`visibilitychange`; the comment says browsers allow few open requests per host) and waits until the page is visible before reconnecting, and gets the whole list again. A non-OK answer throws `The chat list failed to load (HTTP ${status})`. After any end or failure, if still visible, it waits 1000 ms and reconnects; there is no backoff and no retry limit. Notices: aborts and `TypeError` (dropped connection, server gone) are silent; any other failure shows one notice per outage (`failing` flag, cleared by the next message). Cleanup: stop the loop, remove the listener, resolve the visibility wait, abort.

`receiveConversations` (3844): for each row, if its `runningSince` changed, the cached transcript of that chat is dropped; else if its `messageCount` changed (a message stored outside a turn, such as a failed title), the cache is dropped and an open chat not running a turn is resynced. Each row's model info is warmed (`load(modelInfoUrl(provider, model))`, failures ignored). If the new chat's row (`newChatRef`) appears, `settleNewChat` runs: a send the page gave up on reached the server all the same. Chats that left the list (archived here or in another window) lose their cached transcript and their parked draft; if the chat on screen left, the view moves (replacing the history entry) to its successor among Build chats (rows without `benchId`): the next row below still listed, else the nearest above (`successor`, 3834), else the new chat.

`receiveBenches` (3880): sets the list; the first list drops from `openBenches` any bench not listed (archived while the page was away); later lists drop the gone ones. If Bench's place (on screen or remembered in `leftRef.bench`) was a bench that left the list, it moves to the successor bench's comparison, or the new bench; replacing the address if Bench is on screen, else only the remembered place changes. Gone benches' drafts are dropped. A bench whose round on screen is past its `rounds` (reverted elsewhere) is moved back to its last round (`showRound`).

### 2.2 Config read (lines 3978 to 3994)

On mount and on demand (`loadConfig`): `config` goes to loading, `GET /api/config`; on success `providers`, `hosted`, the favorites (`favoriteSyncRef.load(data.modelFavorites)`), `config` loaded, and every usable provider's model catalog is warmed (`load(modelsUrl(id))`, failures ignored) so pickers open already filled. On failure `config` is `{ phase: 'failed', error }` and a notice shows the error. No automatic retry; the empty screens offer a read again (see section 3).

### 2.3 Setup poll (lines 3999 to 4010)

On mount `GET /api/setup`; while `view.ready` is false it is read again every 1000 ms (`setTimeout`). A later start is ready at the first read and never shows the steps. A failed read shows a notice and stops polling (no retry). Cleanup clears the timer.

### 2.4 Held send (lines 4016 to 4020)

When `held` is set and `setup.ready` becomes true: `held` is cleared, and if the place on screen still has that draft key, `submit()` runs. A held draft not on screen when the app is ready stays unsent (no notice).

### 2.5 Model picker after Settings close (lines 4028 to 4032)

When Settings close (phase goes to closed after being open), with an AI connected, no chat model chosen (`!(provider && model)`) and Build on screen, `askModel` is set: the chat model picker opens by itself. Because the chosen model is kept (`mcb-last-model`), this happens only until one is chosen.

### 2.6 First build focus (lines 4039 to 4041)

Each time `firstBuild` becomes true the composer takes focus.

### 2.7 Visibility and seen (lines 4046 to 4055)

`pageVisible` follows `visibilitychange`. When the chat on screen is `unread` and the page is visible, `POST /api/conversations/${id}/seen` (failures ignored). The list stream then brings the row back without `unread`.

### 2.8 Joining a turn (lines 4060 to 4063)

On changes of the active row's id, its `runningSince` or `turn`: `turnToJoin(turn, activeRow)` (transcript.mjs) returns the start time of a turn the server runs that this page does not read (one begun in another window, or one whose stream broke); then `attachToRun(id, startedAt)` joins its stream.

### 2.9 Live world (lines 4072 to 4086)

On mount: subscribe to `LiveWorld` status (sets `minecraftLive`), `world.begin()`; cleanup unsubscribes and `dispose()`s. On each `viewKey` change: `liveWorldRef.current.show(viewKey)`; a failure shows a notice. See section 9.

### 2.10 Autoscroll (lines 4094 to 4100)

After each change of `items`, `busy` or `appMode`: the transcript is pinned if `scrollTop + clientHeight >= previous scrollHeight - 48` (the height recorded on the last update, not the current one); the new `scrollHeight` is recorded; a pinned transcript scrolls to the bottom. The comment says scroll events are too async to trust with deltas every ~40 ms.

### 2.11 Escape (lines 4139 to 4143)

A window `keydown` listener for `Escape` calls `closeTopLayer(evt.target)`. See section 11.

### 2.12 Stored preferences (lines 3642, 3656, 3657, 4145 to 4160)

Effects write `mcb-binds` (JSON), `mcb-bench-blind` (JSON), `mcb-bench-resolution` (JSON), `mcb-theme` (and set `data-theme` on `<html>`), `mcb-app-mode` (`build`/`bench`), `mcb-sidebar` (`open`/`closed`), `mcb-sidebar-width` on every change of their state.

### 2.13 Published list let go (lines 4222 to 4227)

When Settings close and the published list is not unread, the read counter is bumped (any read in flight lands nowhere) and the list becomes `publishedClosed`.

### 2.14 Settings clock (lines 4266 to 4270)

While Settings are open, a 15000 ms interval re-renders so `Checked … ago` stays current.

### 2.15 A chat's turn stream (lines 4544 to 4657)

Two ways in. A send this page makes reads the answer body of its own send request (see section 6, `send`). A turn the page joins (`attachToRun(id, startedAt)`, 4544, from the join effect 2.8) sets the turn to `{ phase: 'reading', id, controller, status: null }` and opens `GET /api/conversations/${id}/stream`, which replays the turn from its start. A fetch that throws (not aborted) is a lost stream. A 404 means no turn runs: it already ended (`finishTurn`). Any other non-OK answer shows a notice with the body's `error` or `HTTP ${status}` (the body may not be JSON) and detaches (the turn is left running, unread).

`readTurn` (4569) reads newline-delimited JSON events; events for a controller that is no longer the turn's are ignored. Each event goes to `receiveTurnEvent`. The stream ends in one of three ways. Finished: a `done` or `error` event came; the turn ends (`finishTurn`: abort the request, `turnEnded`: turn `{ phase: 'idle', ended: startedAt }`, mark the chat seen if the page is visible, and resync the transcript from the server). Lost: the stream ended or broke with neither; `lostStream` sets the turn to `{ phase: 'waiting' }` and after 1000 ms, if still waiting, sets it idle (`ended: null`) and resyncs; the join effect then rejoins if the list still shows the turn running. Broken: an event this page cannot apply (an exception in `receiveTurnEvent`, or a `SyntaxError` in a line): the error shows as a notice and the turn is detached. Before what the end does, any held events are drawn (`drawItems`). The turn's start time for a sent turn is the list row's `runningSince`.

`detachTurn` (4615) aborts the stream and sets `{ phase: 'detached', id, startedAt }`: the chat stays busy, so Stop works and nothing is sent into it, until the list stops showing the turn running (`turnOver`, effect 4621), when the turn ends as above.

Events handled by `receiveTurnEvent` (4641):

| Event `type` | What changes |
|---|---|
| `turn_snapshot` | The whole transcript is replaced from `event.messages` and `event.messageTree`, and shown (this is how a joined turn's stream starts). |
| `harness_status` | The turn's `status` becomes `event.text` or null (the harness's status line under the working indicator). Not passed to the items. |
| `done` | The turn's `status` clears; not passed to the items; ends the stream as finished. |
| `error` | The turn's `status` clears; the event is applied to the items at once (`setItems`, not batched); ends the stream as finished. |
| `tool_result` with `ok` and `name === 'prompt_library'` | The prompt library is reloaded from the server, then the event is applied to items. |
| `tool_result` with `ok` and `name === 'build'` | The live world is woken (`liveWorldRef.current.wake()`, it reads the new build), then applied to items. |
| every other event | Applied to items with `applyEvent` (transcript.mjs), batched to one draw per frame (`setItemsNextFrame`). |

The event types `applyEvent` understands are recorded in section 5.


## 3. Screens and layout

### 3.1 Sidebar resize and collapse (lines 4165 to 4197)

The sidebar's edge is a drag handle (left mouse button only). Movement under 3 px is not a drag. While dragging, the width is the pointer's x minus the layout's left edge; below `SIDEBAR.collapseBelow` the sidebar collapses (`sidebarOpen = false`), and pulled back out it opens again with the width clamped to `[SIDEBAR.min, SIDEBAR.max]` and rounded. On release: a click without movement on a collapsed sidebar opens it (collapsed, the window's left edge is the handle); a drag that ended collapsed restores the width it had at the start (collapsing keeps the width, not the minimum it passed). The body carries classes `dragging` and `sizing-sidebar` during the drag. The width persists in `mcb-sidebar-width` and open/closed in `mcb-sidebar`. The `SIDEBAR` constants (line 1229) are `width: 236` (the default), `min: 200`, `max: 420`, `collapseBelow: 140`. The narrow layout is the media query `(max-width: 980px)` (`NARROW`, line 1234).

### 3.2 The chat and world split (lines 4200 to 4218)

The divider between the chat column and the world drag-resizes the chat column: width = pointer x minus the chat's left edge, clamped to at least 320 px and at most `max(room - 360, 320)`, where `room` is the layout's right edge minus the chat's left edge minus the divider's width (the world keeps at least 360 px). Each move writes `mcb-chat-width` (rounded). No mouse-button check here (any button starts it). The body carries `dragging`. `chatWidth` null means the default `minmax(360px, 42%)` (line 6526). The divider has no double-click behaviour (7002 to 7007), and no way back to the default width other than clearing `mcb-chat-width`. Double-clicking the viewer (not the divider) toggles full screen.

### 3.3 Derived flags used by the layout (lines 6096 to 6112)

`benchMode` is Bench. `memberRow` is the member's row on a member place. `transcriptShown`: in Build on any place but `missing`, and in Bench only on a member's chat. `sidebarLabel` (the sidebar's own toggle button): `Collapse sidebar` in a wide window; when narrow, `Close the list` while the drawer is open, else `Benches and prompts` in Bench or `Chats` in Build. The drawer closes on every change of place, of the active prompt, of Settings opening or closing and of the palette opening or closing (effect 6102). `composerShown` is true when the place has a draft key (a chat, the new chat, a bench place with a composer).

### 3.4 The layout tree (lines 6523 to the end of `App`'s return)

The root is a `layout` element with classes `sidebar-closed` (wide window with the sidebar closed), `prompts-mode` (on the prompt library), `drawer-open` (narrow with the drawer open), and two CSS variables: `--chat-col` (the dragged chat width in px, or `minmax(360px, 42%)` by default) and `--sidebar-w` (the sidebar width). Its children in order:

| Order | Part | Shown when |
|---|---|---|
| 1 | `sidebar-reveal`: an invisible strip at the window's left edge titled `Show sidebar`; mouse down starts the sidebar drag (a click opens it, a drag opens it sized) | wide window, sidebar closed |
| 2 | `drawer-scrim`: a scrim; a click closes the drawer | the drawer is open |
| 3 | the sidebar (`aside`, section 4.8) | always (narrow: as the drawer) |
| 4 | the chat column (`section.chat`): header, notices, then the place's content and the composer (3.5) | always |
| 5 and on | the divider, the viewer, the overlays: recorded in 3.6 onward | see there |

### 3.5 The chat column (lines 6652 to 6895)

Header, left to right. When the sidebar is closed in a wide window: an `Open sidebar` button and a `New bench` / `New chat` button (the same action as the sidebar's). The title: on a missing place its place title (`Archived` or `Not found`); in Build the chat's title or `New build`; in Bench `New bench`, the bench's title on the comparison, `Leaderboard`, on a member's chat a breadcrumb (a button with the bench's title, back to its comparison, then `/`, then the member's name or letter), and on the prompt library a breadcrumb `Prompts` `/` the shown prompt's title or `Untitled prompt`.

Then, on a comparison of a bench with more than one round, a round switch (group label `Round shown in the world`): one numbered button per round (`1`, `2`…, title `Round ${n}`), the one on screen pressed; a click shows that round. On a comparison or a member's chat of a bench that hides names, a `ConfirmButton` (component before App; a first click arms it, a second confirms) with icon `eye`, label `Reveal models`, confirm label `Reveal for good`, title `Show which model made each build. Voting ends here: the votes cast stay as they are.`, confirming `POST /api/benches/${id}/reveal` (notice on failure; the list stream and details then show names). On a member's chat: while names are hidden, a `ConfirmButton` with icon `fork`, label `Fork to Build`, confirm label `Fork and reveal models`, title `Build shows the model, so forking reveals this bench`; otherwise a plain `Fork to Build` button titled `Continue this model's build as a new chat in Build`; both disabled while a fork runs or when the member has no finished round (its last finished round, `round - 1` while running else `round`, is below 1), both calling `forkToBuild`. On the prompt library with a prompt shown, `Run bench`, disabled while the prompt's text is blank, which runs `benchPrompt` (the new bench with that prompt in the composer).

In Build only, model warnings from the model's capabilities: `unknown model` (title `This model is not in the provider's catalog`) when `modelCaps.known === false`; `no tools` (title `This model cannot call tools — the builder needs tool calling`) when the model is known and has no tool calling.

Then the Share button when there is a share subject (section 1, `shareSubject`): `Share` with a share icon, pressed while its panel is open, disabled while a bench member runs (title `Wait for the bench's running turns before sharing`), else titled `Publish this bench's builds to a public page` or `Publish this build's 3D view to a public page`; a click closes an open panel or opens it (`openShare`, notice on failure). The `SharePanel` (component before App) shows anchored under it, given `subject`, `hiddenNames` (a bench that hides names), `share` (this subject's share state or `SHARE_UNKNOWN`), `repo` (`shareRepo`), `onPublish`, `onRemove` (each with notices on failure), `onSettings` (closes the panel and opens Settings on General, where the share repository is set), `onClose: closeShare`, `onError: showError`.

Notices: under the header, the stack of notices (`role="alert"`), one row per notice with its text and a `Dismiss error` button that removes that notice only. Notices are kept until dismissed; there is no timeout.

Then the place's content:

| Content | Shown when | What |
|---|---|---|
| `PromptEditor` (component before App) | on the prompt library once loaded | given `prompts: editorPrompts` (with the unsaved new prompt), `activeId: shownPromptId`, `providers`, `favorites`, `onSetup` (Settings on a view), `onToggleFavorite`, `onCreate: createPrompt`, `onUpdate: updatePrompt`, `onError: showError`. While the library is loading the prompt place shows nothing in the chat column. |
| the transcript (`messages`, scroller with `messagesRef`) | `transcriptShown`: Build on any place but missing, or a bench member's chat | see below |
| the new bench (`messages bench-new`) | the new bench | see section 7.9 |
| the comparison | a comparison whose bench is listed | section 7.8 |
| the missing screen | a missing place | see below |
| the leaderboard | the leaderboard | section 7.10 |
| the composer | `composerShown` | section 6.7 |

The transcript scroller holds, in order: in Build, when the transcript is shown and empty and no turn runs: while setup is preparing, `SetupStatus` (component before App) given `status: preparing`, `noAI`, `onConnect: connectAI`, `onRetry` (`POST /api/setup/retry`, whose answer becomes the setup view; notice on failure); otherwise the empty hint: the grass block at 56 px, `What should we build?`, and under it the connect hint while no AI is connected (`Connect an AI to start building`) or `Or have several models build it in` with a `Bench` link (`benchFromBuild`). Then the transcript's entries (section 5.3). Then, when the read failed: `This chat could not be read: ${error}` with a `Read again` link. Then, while busy and the row shows a running turn, the working indicator (`Ticker`, component before App, the timer) given `startedAt: runningSince` and `status`: `Still working, shown when it ends` while the turn is detached, else the harness's status line (`turn.status`, or null). A member's chat in Bench also shows this scroller (read only), without the empty hint (Build only) and without a composer (a member place has no draft key). The Bench composer button `Send to every model` therefore shows only on a comparison.

The missing screen: the grass block, then `This bench is archived` / `This chat is archived` when the archive read found it, else `There is no bench at this address` / `There is no chat at this address` (also while the read is pending), then `Looking in the archive…` while pending, `Restore it to open it here. What is archived is deleted 7 days after it was archived.` when archived, or `It was deleted, or the address was mistyped.`; then actions: `Restore` (only when archived; `restoreArchived`, opens it once listed) and `New bench` / `New chat`.

### 3.6 After the chat column (lines 7002 to 7218)

The divider and the viewer (section 9.2; both hidden on the prompt library, where the chat column takes the room: class `prompts-mode`). Then the overlays, each only while open: the palette (4.2); the lightbox (a full-window layer showing the image; a click anywhere on it closes it; Escape closes it); Settings (8.5). The narrow layout differs only by the drawer: the sidebar becomes a drawer opened by the sidebar toggle (`Chats` / `Benches and prompts`), with a scrim behind it that closes it on a click, closing also on any navigation, prompt change, Settings or the palette opening, and on a row click inside it. In a narrow window the sidebar's own open/closed and width are ignored (not changed, not shown) and the left-edge reveal strip and the header's `Open sidebar` button do not show. How the narrow layout stacks the chat and viewer is in style.css (the third agent's).

Full-screen viewer: the viewer section takes class `full`; the rest of the layout stays rendered. Escape and double-click leave it.

The conditions in summary, by place (Build/Bench is `appMode`):

| Condition | Chat column content | Composer | Viewer |
|---|---|---|---|
| setup not finished, Build, empty transcript | `SetupStatus` steps | shown; a send is held, with the held line | `The world shows here once the app is ready.` until a world is shown |
| Build, no chat open (`new-chat`), empty | empty hint `What should we build?` | shown | the new chat's world once its row exists, else `Open a chat to view its world.` |
| Build, a chat open | transcript, working indicator while running | shown | that chat's world |
| missing address | missing screen | not shown (no draft key) | per the viewer's empty state |
| Bench, new bench | new bench line-up | shown, Bench controls and Blind | Bench's empty text is null |
| Bench, a bench's comparison | rounds and results | shown, `Send to every model` | the round's world with labels |
| Bench, a member's chat | the member's transcript (read only), working indicator while it runs | none (a member place has no draft key) | that member's own world (`viewKey` is the member's chat id) |
| Bench, leaderboard | the leaderboard | none | no world (`viewKey` null; Bench's empty text is null) |
| Bench, prompt library | `PromptEditor` | none | hidden |
| Settings open | unchanged beneath | unchanged | unchanged |


## 4. The sidebar

### 4.1 New chat, archive, restore, delete (lines 4659 to 4776)

The new chat has one server row, made once when New chat or a send first needs it (`createdChat`, 4671: `POST /api/conversations` with `{ provider, model }`), and it stays the new chat's for as long as it is new, however often the view leaves and comes back. When the row's id lands and the new chat is on screen, the place takes the row (`withRow`) so the live world opens on it (the address stays `/new` or the like until the first message; see section 10). A failed create forgets the request so the next need tries again. The list shows a chat only once it has a message; when the new chat's row appears in the list, `settleNewChat` makes it the chat `id` at the chat's address (replacing the history entry), and the next new chat gets another row.

`newConversation` (4695), the New chat action: without a chosen model it throws `Choose a model before starting a chat` (a notice). On an untouched new chat already (shown, no items, not busy) it only focuses the composer. Otherwise it shows the new chat and creates its row.

`archiveConversation(id, evt)` (4708): `POST /api/conversations/${id}/archive`; the chat leaves the list when the server archives it, and the view moves with it (2.1). The archived list is read again if open. Errors propagate to the caller.

The archived section (archive.mjs): `toggleArchive(mode)` opens it for a mode (reads `GET /api/archive`, numbered; only the latest read lands) or closes it if it is open for that mode. Opening it for the other mode reads it for that mode. It is read again after every archive, restore and delete (`readArchiveAgain`). When the list stream's counts say the open section's mode has no archived items left, it closes (`receiveArchivedCounts`, 4718). `restoreArchived(mode, id)` (4749): `POST /api/{conversations|benches}/${id}/restore`, read the archive again, then open the restored item once the list stream has it (before or after the restore's answer), unless the user navigated meanwhile: a bench opens on its comparison, a chat opens. `deleteArchived(mode, id)` (4773): `DELETE /api/{conversations|benches}/${id}`, then read the archive again. The 7-day rule is in archive.mjs (section 4, archive).

### 4.2 Search everything, the palette (lines 4929 to 5137)

The palette opens and closes with the `search` bind (default recorded in section 11) and closes with Escape (it is a layer), a press on the dark overlay outside the dialog (`mousedown` whose target is the overlay), or running an item. State `command = { query, active }`. The input is focused before the panel paints (`useLayoutEffect`), so no key typed right after the shortcut is lost.

Server search: on each query change, `commandFound` resets to null; with a non-empty trimmed query, after 90 ms of no typing, `GET /api/search?scope=build&q=…` and `GET /api/search?scope=bench&q=…` run together; their `results` land only if the query has not changed (newest wins); failure is a notice.

Highlight: `active` is the highlighted item's key; `highlighted(keys, active)` (command.mjs) resolves it against the items as rendered: `active` while it is listed, else the first item (or none). An effect copies the rendered highlight back into state, so the highlight belongs to an item by its key, not to a position, and stays on that item wherever groups arriving later (the server's matches) put it. `stepped(keys, active, step)` moves by `step` from the highlighted item and wraps round at both ends. `test/command.test.js` states these. The highlighted item is scrolled into view (`block: 'nearest'`) whenever it changes. Moving the mouse over an item highlights it. Keys in the input: ArrowDown and ArrowUp move the highlight by one, wrapping round (`stepped`, command.mjs), when there are items; Enter runs the highlighted item. Typing resets `active` to null. Running an item closes the palette first, then runs it.

Matching (`matchScore`, 4978): the query is split into lowercase words on whitespace; every word must match somewhere, in any order, or the item is left out. Each word scores its best field: in the title 2, elsewhere 1, doubled when the match is at the start of a word (index 0 or after a character that is not `a-z0-9`). The item's score is the sum; items sort by score, highest first (stable for ties). Matched words are marked (`<mark>`) in titles and string details. Server results come with a `snippet`, shown marked (`marked(snippet)`, defined before App).

Groups, in order, each shown only if it has items, each with a limit:

| Group title (searching / not) | Items | Limit searching / not |
|---|---|---|
| `Builds` / `Recent builds` | searching: the server's build results that are listed Build chats (no `benchId`), in server order, detail the snippet; not searching: every Build chat in list order. Icon `build`, title the chat's title; runs `goBuild(id)` | 6 / 4 |
| `Benches` / `Recent benches` | same for benches; runs the bench's comparison | 5 / 3 |
| `Prompts` (only searching) | the library's prompts ranked here by title, text (`search`) and tags (`keywords`, searched not shown); title `prompt.title || 'Untitled prompt'`; detail an excerpt: the text around the first query word in it (from the previous space at least 30 characters before, when the word is more than 40 characters in, with a leading `…`, and 200 characters after the word's start), or its first line; runs `openPrompt(id)` | 4 |
| `Actions` | ranked when searching, else all in this order (table below) | 5 / all 7 |
| `Settings` (only searching) | setting rows and provider rows ranked together | 5 |

Actions:

| Key | Title | Keywords (searched, not shown) | Hint (shown shortcut) | Runs |
|---|---|---|---|---|
| `new-chat` | `New chat` | `build create start` | the `new` bind, in Build only | in Build `newConversation()`; in Bench `goBuild(null)` (the new chat) |
| `new-bench` | `New bench` | `compare models create start` | the `new` bind, in Bench only | the new bench |
| `switch` | `Switch to Bench` in Build, `Switch to Build` in Bench | `mode build bench` | the other mode's bind | `switchMode(other)` |
| `leaderboard` | `Leaderboard` | `votes ranking ratings blind` | none | Bench's leaderboard |
| `sidebar` | `Show the list` when narrow, else `Hide the sidebar` or `Show the sidebar` | `panel toggle drawer list` | the `sidebar` bind | `toggleSidebar` |
| `theme` | `Light theme` in dark (icon `sun`), `Dark theme` in light (icon `moon`) | `appearance colors dark light` | none | flips the theme |
| `settings` | `Settings` | `preferences options` | none | `openSettings()` (default pane) |

Setting rows: `Title model` (section `General`, keywords `chat names naming`), `Theme` (`General`, `appearance colors dark light`), `Clear leaderboard` (`Danger zone`, `votes reset delete`), and one per rebindable action from `BIND_ACTIONS` with its label (section `Shortcuts`, keywords `keybind bind key hotkey`). Each shows detail `Settings · ${section}`, searches `Settings ${section}` too, and runs `openSetting('general', title)`: Settings open on the General pane, and after two frames the row with `data-setting="${title}"` is scrolled to the centre and flashes (class `flash` for 1400 ms). Note that every setting row, including the `Danger zone` and `Shortcuts` ones, opens the pane `general`; the rows must therefore live on that pane (this is the code's assumption; the panes are the other agent's). Provider rows: one per provider in `providers`, with its logo, title `provider.label`, detail `Settings · Harnesses` for a harness or `Settings · Providers` otherwise, keywords `api key login account`, runs `openSetting(id, null)` (Settings open on that provider's pane).

Empty states: with no items, `Searching…` while the server results have not landed, else `Nothing matches “${needle}”` (curly quotes, `needle` is the trimmed query). With an empty query the Actions group is always there, so an empty state shows only for a query. The input's placeholder is `Search builds, benches, prompts and settings`, its label and the dialog's label `Search everything`; an `esc` key cap shows at the input's end; an item with a hint shows the bind formatted (`formatBind`, defined before App) at its right.

### 4.3 Sidebar search and filters state (lines 5139 to 5155)

The sidebar search text `find` is searched on the server's full-text index for the current mode: after 90 ms, `GET /api/search?scope=${appMode}&q=…`; `found` becomes `data.results`; newest wins; an empty query sets `found` to null at once; failure is a notice. Switching mode clears the search text and the filters (`NO_FILTERS`): both are the mode's own and do not survive a switch.

### 4.4 Status dots (lines 6147, 6165 to 6185, 6253 to 6311)

The dot labels (`DOT_LABEL`, read by screen readers as the dot's `aria-label`): `running` `Working`, `unread` `Finished, not opened yet`, `halted` `Stopped or failed`, `idle` `Idle`. `dotClass(key, state)` (6168) gives `conv-dot ${state}` plus `arrived` when a dot that was `running` becomes `unread` (and keeps `arrived` while it stays unread): a dot that was working and is now waiting eases from grey into blue; one already waiting when it first appears simply is blue. The history is kept per dot key for the page's life. All running dots pulse in step (3399). A chat row's state and a bench member's state come from `memberDot(row, activeId)` and similar helpers defined before App (other agent); the bench row's dot is the first of `running`, `unread` found among its members' states.

The mode switch's dot (6177): for the other mode, `unread` if any of its items is finished and not looked at (a bench counts as unread only once none of its models is running and some member is unread; a running bench is `running`), else `running` if any is running, else none. `otherWaiting` is the count of unread items in the other mode (shown with the dot where the switch is rendered).

### 4.5 Filters and search results in the lists (lines 6187 to 6217)

`filterView` (6190): the Build chats (no `benchId`), each bench's members, and what the filters offer (`offered(...)`, filters.mjs) for the current mode: for Build each chat's facets (`chatFacets`), for Bench each bench's facets (`benchFacets(bench, members)`), with the library's prompts once loaded and `promptTitles`; `labels` are `filterLabels(offer, providers)`. `shownFilters` are the filters in effect (only values still offered, `inEffect`) when any are set. `narrowed` is true when there is a search result set or a filter in effect. `buildRows` and `benchRows` are `listed(rows, found, filters, facetsOf)` (filters.mjs): the rows in search rank order when searching, narrowed by the filters. `toggleFilter(group, value)` toggles one value.

A search result's snippet (from the server, with matches between `\u0001` and `\u0002`) is shown under the row's title, with the matches marked; when the first match is more than 18 characters in, the snippet starts at the space at least 12 characters before it with a leading `…` (the sidebar is narrow). The palette uses the same `marked`.

### 4.6 Archived section (lines 6219 to 6251)

Below a mode's list, only when that mode has archived items (the count from the list stream). A toggle button `Show archived` / `Hide archived` (with a chevron, `aria-expanded`). Open: `Loading…` while the first read runs (rows already read stay on show during later reads); on failure `Archived items failed to load.` with a `Try again` link that reads again; then one `ArchivedRow` (component before App) per archived item with `title`, `meta` (for a bench `${n} model · ` / `${n} models · ` then `timeLeft(row.expiresAt, now)`; for a chat only the time left; `timeLeft` is archive.mjs), `onRestore` (`restoreArchived`) and `onDelete` (`deleteArchived`), both reporting failure as a notice. The comment says each item is restorable until it expires, or deleted now on a second click (the two-click delete is inside `ArchivedRow`).

### 4.7 A bench's row and its members in the sidebar (lines 6253 to 6311)

Members are the chats with that `benchId`, in the bench's `order`. The bench row is active on that bench's comparison; clicking it opens the comparison. A caret button (`Show models` / `Hide models`, `aria-expanded`) expands or collapses the members without opening the bench (`openBenches`, kept in `mcb-open-benches`). Then the bench's title (with its search snippet), its `ResolutionTag` (component before App), the dot: collapsed, the bench's dot if any member is running or unread; expanded, or with no such state, an idle placeholder dot that holds its place (so the resolution tag does not move when the bench opens, closes, starts or stops). Then the archive button (title `Archive bench (it can be restored for 7 days)`, label `Archive bench`), disabled while any member runs. Expanded, each member is a row with its dot (`memberDot`) and its name (`nameOf`: the model name, or `Build A` etc. while names are hidden; the hover title is the model id unless names are hidden); active when it is the chat on screen; hovering a member that is not running and not on screen prefetches its transcript; clicking opens the member's chat.

### 4.8 The sidebar, top to bottom (lines 6530 to 6650)

The sidebar is an `aside` in the layout. In the narrow drawer, a click on a chat row, bench row, member row or prompt row (`.conv-item, .pe-item`) that is not on a button inside it closes the drawer.

Brand row: the app mark (a grass block, 28 px), the `ModeSwitcher` (component before App) given `mode`, `binds`, `waiting` (the other mode's unread count), `dot` (the other mode's dot, `unread` or `running`, with the same arrival easing, keyed `switch:${otherMode}`) and `onChange: switchMode`. Then two buttons: `New bench` / `New chat` (by mode: the new bench, or `newConversation()` with a notice when no model), and the sidebar toggle labelled `sidebarLabel` (3.3), with `aria-expanded` only when narrow, showing an `x` icon while the drawer is open and a panel icon otherwise; narrow it toggles the drawer, wide it closes the sidebar.

Search row: an input with placeholder `Search`, label `Search benches` in Bench or `Search chats` in Build; typing sets `find` (4.3); Escape in it clears the search and leaves the field (blur), and also reaches the window's Escape handler (11.5). Beside it `FindFilter` (component before App) given `offer`, `labels`, `filters` (the ones in effect), `emptyPrompts` (`No bench was sent from a library prompt yet` in Bench, `No chat was sent from a library prompt yet` in Build), `buttonRef` and `onToggle`. When a filter is in effect, `FilterChips` (component before App) under it with `labels`, `filters`, `buttonRef`, `onToggle` and `onClear` (clears all filters).

Build's list: when searching or filtering left nothing, `No matches`. Otherwise one row per Build chat (`buildRows`, in the order the list stream sends them, or by search rank when searching; the landing treats the first Build row as the newest chat). The row's state: `running` while `runningSince` is set (the dot pulses), `unread` when finished and not looked at (solid), else `idle` (hollow); the open chat is always looked at, so it is never `unread`. The row shows the dot (with its label), the title with any search snippet, the chat's `ResolutionTag`, and an archive button (`Archive build (it can be restored for 7 days)`, label `Archive build`) disabled while running. The row is active when it is the chat on screen. Hovering a row that is not running and not on screen prefetches its transcript (failures ignored). Clicking opens the chat (or retries a failed read of the chat on screen). Then Build's archived section (4.6).

Bench's list: when not searching or filtering, a `Leaderboard` row first (trophy icon), active on the leaderboard. When searching or filtering left no bench, `No matches`. Then the bench rows (4.7) in `benchRows` order, then Bench's archived section, then once the prompt library has loaded the `PromptsSection` (component before App) given `prompts`, `onCreate: createPrompt`, `activeId` (the shown prompt on the prompt place, else null), `onSwitch: openPrompt`, `onReorder: reorderPrompts`, `onToggleFavorite: togglePromptFavorite`, `onDelete: deletePrompt`. Note that the prompts section is not narrowed by the sidebar search or filters. When the library failed to load: `Prompts failed to load.` with `Try again` (loads it again).

In either mode, when the config read failed: `Models failed to load.` with `Try again` (`loadConfig`).

Foot: a `Settings` button (opens Settings on the default pane). The sidebar's edge: a handle titled `Drag to resize · double-click to collapse`; mouse down starts the resize (3.1); double-click collapses the sidebar (`sidebarOpen = false`).

There is no rename action anywhere in `App`'s range: chat and bench titles come from the server (the title model or the chat's own model) and `App` renders no rename control. If the sidebar has rename, it is not in this range (no handler for a rename route exists in `App`).

### 4.9 archive.mjs: the archived section's states and the 7-day rule

One value for one mode at a time: `closed`; `loading` (`data` is an earlier read of the same mode, still shown, or null); `loaded` (`data` is the server's `{ conversations, benches }`); `failed` (`error`, can be read again). Transitions: closed to loading; loading to loading, loaded, failed or closed; loaded and failed to loading or closed (others throw). A read for the other mode starts with no data. `archivedRows` throws on a closed archive and gives null before the first read. `timeLeft(expiresAt, now)`: the server deletes an archived item at `expiresAt`; the row says `less than a day left` under 24 hours, else whole days rounded up: `1 day left` or `${n} days left` (one archived a moment ago shows `7 days left`, matching the Archive buttons' `it can be restored for 7 days`). `test/archive.test.js` states these; `test/server-archive.test.js` the server side.

### 4.10 filters.mjs: the sidebar's filters

Three groups, `size`, `prompt`, `model`; `NO_FILTERS` has none chosen. A row is listed when it matches every group with a choice, and within a group any chosen value. Facets: a chat's `size` is its current resolution, `prompt` the ids of the library prompts any of its messages was sent from (`row.prompts`), `model` its current model as `provider::model`; a bench's size is its resolution, prompt its rounds' prompts, model any of its members' models. Filtering takes no request: only the text is searched on the server and its results are narrowed here. `listed`: the search's results in rank order (or every row), then filtered. `offered`, what the menu offers, from every row the mode lists (not only those shown, so one choice never takes the others away): every resolution stop (resolution.mjs `RESOLUTIONS`); the prompts the rows came from in the library's order, then those deleted from the library by their last title (`promptTitles`) alphabetically, marked deleted; the rows' models sorted by model id then provider. `inEffect`: a chosen prompt or model no listed row has any more is no choice and its chip goes; it comes back with a row that has it. The prompt facet uses the library only once it has loaded (`promptsReady ? prompts : []`), so before that every used prompt is offered as deleted. `test/filters.test.js` states these.


## 5. The transcript

### 5.1 Edit and branch (lines 5237 to 5269)

`editPrompt(of, { text, attachments, selections })`: the sent prompt or round `of` comes into the composer to be changed; nothing else changes, on screen or on the server, until it is sent, and the send makes the branch. The draft becomes `beginEdit(currentDraft(), of, { segments: text ? [one text segment] : [], selections, reading: [{ name, size }...] })` (draft.mjs; it saves what the composer held), the caret goes to the end (`setCaretAtEnd`), and each file is fetched back from its stored `url` and read as a data URL (`withData`). A file that cannot be read throws `Could not load "${name}" (HTTP ${status})`: the whole batch finishes with no files (`finishRead(batch, [])`) and the error shows as a notice; so one unreadable file leaves out all files of that edit, not only that one (the comment says "A file that cannot be read back is left out, and says so"; the code drops the batch). `cancelEdit()` (Cancel or Escape) restores what the composer held before (`endEdit`) and puts the caret at the end.

### 5.2 Copy and branch switching (lines 5558 to 5585)

`copyMessage(item)`: writes the message to the clipboard (`writeMessageClipboard`, clipboard.mjs: text plus its attachments, so a paste into this app brings both back), marks it `copied` for 1200 ms (the copy button shows its copied state), then clears it if it is still that item. `switchBranch(messageId)`: only with a chat on screen and not busy; `branching` while it waits; `POST /api/conversations/${id}/branch` `{ messageId }`; the live world is woken and the cached transcript dropped; if the user has not navigated, the transcript is pinned to the bottom and replaced from the answer's `messages` and `messageTree`. Failure propagates (notice).

`activeTitle` (5730) is the chat's title or `New build`. `idea` (5733) is one idea per chat view (`pickIdea`, defined before App), stable across renders, re-picked when the chat changes; `prompted` is whether the transcript has a user message.

### 5.3 How the transcript is rendered (lines 5808 to 6094)

`usageMeta` (5810) is the latest `usage` item whose `telemetry.failed` is not set: the context meter and the limit chip use it; a stopped or failed request reports no usage, so the meter keeps the last real one. `usage` items are never drawn as rows (filtered out at 5824).

`readOnly` (5813): a bench model's chat (the row has a `benchId`) reads and never edits: no branch arrows, no Edit and branch, no error dismiss. `changing` (5816): a branch switch is waiting or an edit is in the composer; no second change starts (branch arrows and Edit and branch are disabled). `editedId` is the prompt or round being edited; its user block carries the class `editing`.

The items are grouped by `groupItems(items without usage, busy)` (transcript.mjs, section 5.6) into entries: single items, `activity` groups and `tool-stack` groups. Each entry's rendering is cached until something it shows changes (an implementation detail for speed: a streamed token re-renders only the entry it lands in). Entries are keyed by `entryKeys(entries)`.

User message (`kind: 'user'`, 5827): the message text as plain text (not markdown); then its attachments: an image (`type` starting `image/`) is a button with a thumbnail (`file.data || file.url`) and its name, opening the lightbox on click; any other file is a link to `file.url` (or `#`) opening in a new tab, with a file icon and its name. Then its selections as `SelectionChip`s (component before App), each lighting its box in the viewer on hover (`onHover = setHoveredSelection`). Under it the message tools: the branch picker when this message has branches (not read-only): `Previous branch` button, `${index + 1} / ${count}`, `Next branch` button, each disabled while busy, while changing, or with no branch that way, each calling `switchBranch`; the `Copy message` button (icon turns to a check for 1200 ms after copying); and `Edit and branch` (only for a stored message, when the transcript has more than one item, not busy, not read-only; disabled while changing), which calls `editPrompt(messageId, item)`.

Assistant text (`kind: 'assistant'`): markdown (`Markdown`, component before App) in a `msg assistant` block. Reasoning (`kind: 'reasoning'`): a fold row titled `Show reasoning`, label `Reasoning…` while `live`, else `Reasoning`, body the markdown text. `plan`, `notice` and `shots` at the top level render as log items (below).

Tool stack (`kind: 'tool-stack'`, a live turn's run of tool calls): the latest call is shown in full (its tool row and its outputs); the earlier ones fold under `+${n} previous tool call` / `+${n} previous tool calls` (title `Show previous tool calls`), expanded to show each.

Activity (`kind: 'activity'`, a finished stretch of work): a fold whose summary is `Worked · ${n} step` / `Worked · ${n} steps` (n is the number of `tool` children) or `Worked` with no tool; consecutive reasoning children merge into one `Reasoning` row (texts joined with a blank line); the body renders each child as a log item.

Error (`kind: 'error'`): its text in a `msg error` block; when not read-only and not busy, a dismiss button (`Dismiss error`) removes it from the screen, and for a stored error (it has a `messageId`) dismisses it in the cached transcript (`forgetError`) and on the server (`POST /api/conversations/${id}/dismiss-error` `{ messageId }`, notice on failure), since persisted errors would otherwise come back on the next resync.

Log items (`renderLogItem`, 5963), inside folds everything drops a register: assistant text is a `log-note` in markdown; `plan` is a `PlanCard` with `items`; `notice` is a `NoticeLine` with `level` and `text` (components before App); `reasoning` a reasoning row labelled `Reasoning`; `shots` is a row of thumbnails, each opening the lightbox (these are the images from `see_build`). A tool row:

| `item.tool` | Action text: running (`writing` or `running`) / failed / done (`cancelled` where noted) | Detail text | Icon |
|---|---|---|---|
| `bash` | `Running command` / `Command failed` / `Ran command` | the summary without a leading `$ `, in a monospace span | `terminal` |
| `build`, summary (after removing a leading `build`, `building` or `built`) starting with `inspect`, `check`, `verify`, `dump`, `read` or `examine` | `Inspecting` / `Inspection failed` / `Inspected` | the rest after that word | `hammer` |
| `build`, other | `Building` / `Build failed` / `Built` | the summary without a leading `build`/`building`/`built` | `hammer` |
| `see_build` | `Inspecting build` / `Inspection failed` / `Inspected build` | the summary without a leading `Look at build` | `eye` |
| `test_build` | `Testing in Minecraft` / `Minecraft test failed` / `Minecraft test completed`; `Minecraft test cancelled` when `status === 'cancelled'` | while running, `item.progress` if any; else `${n} tests: input and block-state observations` when the summary has `(n tests)`, else `Input and block-state observations` | `wrench` |
| `view_image` | `Viewing image` / `Image view failed` / `Viewed image` | the summary without a leading `Look at` | `eye` |
| any other | the tool's name, or `Tool` | the summary, or the tool name, or `…` | `wrench` |

A detail equal to the action, or blank, is not shown. The action text is styled active while running. The detail carries a hover title of itself. While the tool's input is still being written (`status === 'writing'`) and some program has arrived, `+${n} lines` shows (lines of the program so far). An `item.diff` shows a `DiffStat` (component before App). A failed tool shows an `x` mark at the row's end, and its `item.detail` under the row as `log-err` text. A row with a program, or still being written, is a fold titled `Show code` whose body is the program as highlighted code (`Code`, component before App), or `Streaming tool input…` while writing with nothing yet, plus `item.output` as terminal text when present; while writing, the code box sticks to its bottom if it was within 40 px of it. Other rows are plain.

### 5.4 Stored messages to items (`itemsFromMessages`, app.js 336 to 401, before App; recorded here because every transcript `App` shows goes through it)

Stored messages are in OpenAI format. A `user` message with array content gives a `user` item with its text parts joined by newlines, its `attachment` parts and its `selection` parts, and `messageId`; except a user message marked `seeBuild` (the images the app fed back after `see_build`), which gives a `shots` item with its `image_url` parts. A `user` message with string content gives a user item with no files. An `error` message gives an `error` item unless `dismissed`. An `assistant` message gives, in order: a `usage` item when it has `telemetry` (with `rateLimit`), a `notice` item per entry of `notices`, a `plan` item when it has `plan`, a `reasoning` item when it has `reasoning`, an `assistant` item when it has content (`final` when the message has no tool calls), then one `tool` item per tool call with `status: 'ok'`, its `program` (the `program` or `command` argument, or for `test_build` the whole arguments as indented JSON), and its summary: the `summary` argument, or for `see_build` `Look at build (${views})` (each shot's `view`, or `camera` for a shot with `eye`, else `iso_se`; `default` with none), for `bash` `$ ${first line of the command, 80 characters}`, for `test_build` `Test build in Minecraft` plus ` (${n} tests)` when more than one, for `view_image` `Look at /work/${path}`, else the tool's name (`build` when the arguments do not parse). A `tool` message completes its call's item: `status` `cancelled`, `ok` or `failed`; `diff` when ok; `output` when a string (for `test_build`, the whole result as indented JSON); `detail` the error, or `failed` when there is neither error nor output; a `bash` result that is not JSON is its terminal text as `output`.

### 5.5 Stream events to items (transcript.mjs `applyEvent`)

| Event | Effect on items |
|---|---|
| `completion_retry` | drops only the interrupted response's uncommitted stream: every live item (streaming text or reasoning) and every tool still `writing`; completed tool results and messages remain; no tool is run again by a retry |
| `assistant_delta` | appends `text` to a trailing live assistant item, or starts one |
| `reasoning_delta` | the same for reasoning (shown `Reasoning…` while live) |
| `assistant` | seals the latest live assistant item with the full `text` and `final`; with none live, appends one if the text is not empty |
| `reasoning` | seals the latest live reasoning item, or appends one if not empty |
| `tool_call_delta` | needs `id` (else throws `Tool call ${index} streamed without its id`); merges the argument text into the `writing` tool item with that call id, or adds one (`tool` the event's name or `build`, status `writing`); arguments are parsed as they stream (tool-stream.mjs) for `summary` and `program` (`program` or `command`) |
| `tool_start` | the writing call with that `callId` becomes `running` with the event's `summary`, `name` and `program` (kept if absent); a start with no written call throws `Tool call ${callId} started without being written` |
| `tool_progress` | sets `progress` (the event's `message`) on the running call (shown as the test_build detail) |
| `tool_result` | the running call becomes `cancelled`, `ok` or `failed`; `diff` when ok; `output` when a string; `detail` the error, or `failed` with neither error nor output; with `images`, a `shots` item follows; a result with no running call throws `Tool call ${callId} finished without running` |
| `error` | appends an error item (`message`, `messageId`) |
| `notice` | appends a notice item (`level`, `text`) |
| `plan` | one live task list per turn: replaces the plan item after the last user message, or appends one |
| any other | throws `Unknown turn event: ${type}` (the turn stream then counts as broken: notice and detach) |

`harness_status`, `done` and `turn_snapshot` are handled in `App` and never reach `applyEvent` (2.15). The harness's status line (the `status`, e.g. `Retrying (attempt 2 of 10)` per transcript.mjs's comment) shows in the working indicator as `${status} · ${elapsed}`; without a status it reads `Working for ${elapsed}` (the `Ticker` component, app.js 304 to 309, updating each second). Retries are therefore visible as that status line plus the `completion_retry` clean-up.

tool-stream.mjs: `parseToolArgs(raw)` parses the arguments when they are whole JSON (unwrapping up to three levels of nested `arguments` or `function.arguments`), else extracts the top-level string fields `summary`, `program` (or `command`) and `path` from the partial JSON as they stream, including a value still open (a trailing lone backslash or partial `\u` escape is dropped). `mergeToolCallDelta` keeps the last decoded fields on the item, so rendering never depends on reparsing one fragment. `test/tool-stream.test.js` states these.

### 5.6 Grouping (transcript.mjs `groupItems`, `entryKeys`)

Consecutive items of the kinds `reasoning`, `tool`, `shots`, `assistant`, `notice` and `plan` form a stretch of work; `user`, `error` and `usage` items break it (usage is filtered out before grouping in `App`). Within a stretch only the latest `plan` is kept. The last stretch, while busy and before it has a final assistant message, is live: tool calls in a row compact into a `tool-stack` (each call with the `shots` that follow it as outputs); other items show as they are (assistant text visible, reasoning in its own fold). Any other stretch is settled: everything except the final assistant message (the last with `final: true`), the plan and non-`info` notices folds into one `activity` entry; then the plan and warnings (notices whose level is not `info`) stay visible after it; then the final message. So after a turn the transcript reads: user message, `Worked · n steps` fold, plan card, warnings, final answer. A turn whose prompt has not shown yet leaves the previous turn's fold as it is.

Keys (`entryKeys`): `${turn}:user` for the n-th user message, `${turn}:${ordinal}:${kind}` after it, so a fold keeps its open state while entries around it change, and the settled fold is a different entry from the live one that stood in its place (an open live stack does not leave the settled fold open).

Branch navigation (`branchNavigation`, app.js 403, before App): among `branchable` messages of the tree, siblings with the same parent, sorted by `position`, give each message `{ index, count, previousId, nextId }` when there are at least two.

Tests: `test/transcript.test.js` (turn phases and transitions, `turnRunning`, `turnToJoin`, `turnOver`, `applyEvent`, `groupItems`, `entryKeys`), `test/tool-stream.test.js`, `test/turn-stream.test.js` and `test/turn-lifecycle.test.js` (server side of the same stream).

### 5.7 The turn's phases (transcript.mjs)

| Phase | Meaning | Can go to |
|---|---|---|
| `idle` | no stream held; `ended` is a turn start the page saw end, so a list still showing it running is stale | idle, sending, reading |
| `sending` | a message on its way (the chat being created, or the POST before the server takes it), with its controller | reading, idle |
| `reading` | the page reads the turn's events; `status` is the harness's line | reading, waiting, detached, idle |
| `waiting` | the stream broke mid-turn; the page joins again shortly | idle |
| `detached` | the server runs the turn (chat `id`, begun `startedAt`) and the page stopped reading it | idle |

Sending and reading, and only they, hold a controller (`turnTo` throws otherwise). `turnRunning(turn, row)`: busy when the phase is not idle, or the row has a `runningSince` that is not the one the page saw end. `turnToJoin`: idle, and the row runs a turn not seen ending. `turnOver`: detached, and the row no longer shows that start running.

Turn end states as the user sees them: finished turns show their items, settled, with any error item (the server's error text, `event.message`, dismissable); a stopped turn's items as the server stored them after the resync (the server decides what a stop stores); a failed send restores the transcript and shows a notice. The texts of server errors come from the server (routes agent).

### 5.8 Scrolling rules

The transcript (and the comparison, which uses the same scroller ref) follows the newest content only if the user was within 48 px of the bottom of the previous content (2.10). Opening any place (`leaveConversation`), sending (`send`) and a branch switch reset the recorded height to 0, so the next update always scrolls to the bottom: a newly opened chat starts at its newest message, a send always jumps to the live tail. Streaming keeps the view at the bottom while pinned; scrolled up, it stays where the user is. The streaming code box inside a tool fold sticks to its own bottom within 40 px (5.3). There is no jump-to-bottom button anywhere in `App`'s range. There is no scroll restoration per chat: returning to a chat starts at the bottom.

### 5.9 Lightbox

Image attachments in user messages and round prompts, and `shots` thumbnails (see_build images and tool result images), open the lightbox: a full-window layer with the image (`file.data || file.url`, or the shot's src); a click anywhere on it or Escape closes it. Image attachments in the composer's tiles do not open it.


## 6. The composer as `App` drives it

### 6.1 Submit and the held send (lines 5399 to 5411)

`submit()` is every send (Enter, the send button). While the app is preparing (setup not ready), a send is held: if the composer has text or attachments, `held` becomes the place's draft key and the send goes once setup is ready (2.4); nothing else happens and the composer keeps its content. The exception: a Build send with no model chosen goes on, so that it says so (see `send`). When ready: Build goes to `send()`, the new bench to `runBench()`, a bench comparison to `sendRound(benchId)`.

### 6.2 Attachments: picker, paste, reading (lines 5413 to 5469)

`addFiles(files)` (5416), from the file picker or a drop: `checkAttachmentLimits(draftFiles(draft), files)` throws first if the files would exceed the draft's limits (the limits and texts are in draft.mjs, recorded in 6.8); files count toward the limits at once, from the pick. The draft becomes `reading` the batch (`{ name, size }` each) immediately, so a send in between sees it and is refused (the send does nothing while `reading` is non-empty). Each file is read in turn (`readUpload`, defined before App: images are prepared, see upload-image.mjs, other agent). A failed read ends the batch with no files and throws (notice). `finishRead(batch, files)` (5436) puts the files where the batch is held: the composer's draft if it holds it, and the parked drafts (`draftsRef.endRead`) wherever the user is by now; a draft that is gone (its chat archived, another prompt put in its place) took the batch with it.

`onPaste(evt)` (5446): first a message copied from this app (`readMessageClipboard`, clipboard.mjs) is recognised: its text is inserted as plain text and its attachments join the draft (after the limit check; a limit error shows as a notice and nothing is pasted). Otherwise any files on the clipboard go through `addFiles` (notice on failure); with no files only the clipboard's `text/plain` is inserted, never pasted formatting. Pasting files does not insert the clipboard's text.

### 6.3 Stop (lines 5471 to 5480)

`stop()`: while the turn is `sending` (the message is still being sent), the Stop is kept for `send` (`stopAskedRef`): before the POST it cancels the send and puts the message back; after it the server's turn is stopped. Otherwise, with a chat on screen, `POST /api/conversations/${activeId}/stop` (failure is a notice). The turn's end comes through the stream as usual.

### 6.4 Send (lines 5482 to 5556)

`send()`: does nothing while a turn runs on this chat (`turnRunning`) or files are being read, or when there is no composed message (an empty fill-in box takes the focus and flashes instead), or when there is neither text nor attachments. Without a model it shows a notice: `Connect an AI in Settings first` when no AI is connected, else `Choose a model before sending`, and keeps the draft. Otherwise: the turn becomes `sending` at once (so a second Enter, or New chat's creation, never makes a second chat); the composer is cleared as after a send (`afterSend`); the transcript is pinned to the live tail; the user's message is shown at once as an item `{ kind: 'user', text, attachments, selections }`. For an edit, the items from the edited prompt onward are dropped first (the edit's prompt takes its place on screen as on the server).

Then the chat id is the place's, or for the new chat its row (`createdChat`, awaited; aborted if the user navigated). A Stop asked meanwhile puts the message back, restores the transcript and ends the turn. `POST /api/conversations/${id}/chat` with `{ text, prompts, provider, model, files: attachments, selections, effort, resolution, editOf? }`; `effort` is `effortFor(modelCaps, effort)` when the model's capabilities are known, else the raw choice. A non-OK answer throws its JSON `error` or `HTTP ${status}`. Any failure before the server took the message: the message goes back into its draft (`putBack`); if aborted (the user navigated) nothing else; otherwise a notice with the error, the transcript as it was before, and the turn ends (or goes idle when there was no chat id). Once taken: the new chat becomes the chat (`settleNewChat`); for an edit the cached transcript is dropped and the live world woken (the edit went back to the world before the prompt it edits); the turn becomes `reading`; a Stop asked meanwhile is sent to the server (`POST …/stop`); and the answer body is read as the turn's stream (`readTurn`, 2.15).

### 6.5 Composer typing, palette and fill-ins (lines 5587 to 5728)

The prompt palette over the composer (`palette`, 5590): null when closed, else `{ query, index, slash, withSearch }`; `slash` is the `/query` that opened it (`composer.slashAtCaret`) or null when the button opened it. Any rewrite of the composer closes it (`setComposer`). A `mousedown` anywhere outside the composer box closes it (effect 5607).

`caretAtEnd` (5597): bumped when an edit or its cancel swaps the composer's draft; a layout effect then focuses the composer with the caret at the end.

`onComposerChange({ text, segments })` (5616), on every keystroke: files that came with a prompt token (`fromPrompt`) are dropped when that token is deleted; segments and `hasText` update (no re-render unless `hasText` changes); a `/query` at the caret opens or updates the palette (`{ query, index: 0, slash, withSearch: false }`), and moving off a slash closes a palette that a slash opened.

`composedMessage()` (5631): reads the composer (`composer.read`); if a fill-in box is empty, that box flashes (class `missing` removed and re-added on its parent, restarting the animation), takes the focus, and the send does not happen; else `{ text: trimmed, segments, prompts: composer.promptLinks(segments) }` (the library prompts the message was inserted from, which the server records).

`insertPrompt(prompt)` (5646): the picked prompt replaces the `/query` that summoned it, or lands at the last caret (`caretRef`, if still in the composer) or at the end; it is inserted as a token or as its text with boxes for its fill-ins (`composer.promptSegments`, composer.mjs), and the first box takes the focus. The prompt's attachments join the draft marked `fromPrompt: prompt.id`. The prompt's `usedAt` is set to now in the library (a library write). The palette closes.

`saveDraftAsPrompt()` (5665): a new library prompt from the composer: `id` a random UUID, title the text's first line (cut to 47 characters plus `…` when longer than 48), the text, the draft's attachments, no tags; the palette closes and the prompt opens in the library editor (`openPrompt`).

Palette keys (`paletteKey`, 5681), while it is open, before any other composer key: ArrowDown and ArrowUp move through the flattened sections (`promptSections(prompts, query)`, defined before App) and wrap around; Enter without Shift, or Tab, inserts the highlighted prompt (when there is one); Escape closes the palette, stops the key from reaching the page (so no layer closes and no edit is cancelled) and focuses the composer.

Composer keys (`onKeyDown`, 5706): with the palette handling first; Escape while editing cancels the edit (stopped from reaching the page); Tab and Shift+Tab walk through the fill-in boxes cyclically when there are any (otherwise Tab is the browser's); Enter without Shift and not during IME composition submits (`submit()`); Shift+Enter is a new line.

### 6.6 The composer's controls (lines 6430 to 6521)

`benchReady` is at least 2 bench models. `connectHint` is the link `Connect an AI to start building` (opens Settings on Connect an AI), shown under an empty screen's question while no AI is connected. `fitBenchPicker` sets `--below` on the bench line-up row (window height minus the row's bottom minus 16 px) before its models' popover opens, capping the popover's height.

The resolution picker (`ResolutionPicker`, component before App) shows in a chat, the new chat and the new bench, and on a comparison only when the bench's resolution is above Standard (`> 1`), read only there (the models' chats keep it). Its value: the new bench's `benchResolution`, the comparison's bench resolution, else the chat's `resolution`. Its change sets `benchResolution` in the new bench, else `chooseResolution` (also stored as `mcb-last-resolution`).

Build's controls, in order: when the latest usage is for the current provider and model, a `LimitChip` with `usageMeta.rateLimit` and, when both the context tokens and the model's context window are known, a `ContextMeter` with `used` and `window`; the resolution picker; once the list is ready, the chat model picker (`ModelPicker`, given `providers`, `value` `{ provider, model }` or null, `favorites`, `onToggleFavorite`, `requireTools: true`, `label: 'Chat model'`, `openNow: askModel`, `onOpened` (clears `askModel`, marks the picker as opened by itself), `footer: { label: 'Compare several in Bench', icon: 'bench', onSelect: benchFromBuild }`, `onSetup(view)` opening Settings on a view); its change stores `mcb-last-model` `{ provider, model }` (a fresh page starts with it), sets the choice with effort reset to `''` and the current resolution kept, and if the picker had opened by itself the composer takes focus next frame. Then, once the list is ready, when the model reasons and has efforts, the `EffortPicker` (`caps`, `value: effort`, `onChange: setEffort`). Then, while busy, a Stop button (`Stop generating`), else a Send button (`Send message`), disabled when there is neither text nor attachments, no model, or files are being read.

Bench's controls: the resolution picker, then a send button titled `Run on ${n} models` on the new bench or `Send to every model` on a comparison (or member chat), disabled when there is neither text nor attachments, on the new bench with fewer than 2 models, or while files are read. There is no Stop button in Bench's composer (stopping is per model, in the comparison's rows).

### 6.7 The composer block (lines 6895 onward)

Shown when the place has a draft key. From the top: when this place's send is held and setup is still preparing, a status line (`role="status"`): animated working dots unless setup failed, `heldLine(preparing)` (setup-status.mjs, the exact texts are in 6.9), and a `Don't send` link that drops the hold (the draft stays in the composer). When an edit is in the composer, a status line with a compose icon and `Editing a round: sending it runs the new version on every model` in Bench or `Editing a message: sending it starts a new branch` in Build, and a `Cancel` link (label `Cancel edit`, title `Cancel edit (Esc)`) that cancels the edit. Then the composer box (class `busy` while busy), continued in 6.8.

### 6.8 The composer box (lines 6910 to 6999)

Inside the box, from the top: the prompt palette when open (`PromptPalette`, component before App), given `prompts`, `query`, `index`, `withSearch` (true when opened by the button, which shows its own search field), `onQuery` (sets the query, index back to 0), `onIndex`, `onPick: insertPrompt`, `onEdit` (closes the palette and opens the prompt in the library), `onToggleFavorite: togglePromptFavorite`, `canSaveDraft: hasText`, `onSaveDraft` (`saveDraftAsPrompt`, errors as a notice; it throws before the library loaded), `onRetry` (`loadPrompts` when the library failed to load, else null), `onKey: paletteKey`.

In Build, when the draft has selections: one `SelectionChip` per selection (`draft: true`, `onHover` lights its box, `onRemove` removes it). Bench never shows selections (and the viewer's `onSelect` is null in Bench).

When the draft has attachments or files being read: one tile per attachment, an image's thumbnail (`file.data || file.url`) or a file icon, its name (hover title the name) and its kind (the part of its MIME type after `/`, or `file`), and a `Remove attachment` button; then one tile per file still being read, with working dots, its name and `Reading…` (`role="status"`). Removing an attachment that came from a prompt token does not remove the token.

Then the input (`ComposerInput`, component before App), given `inputRef: composerRef`, `value: composerValue`, `placeholder` (`Follow up with every model` on a comparison; empty once the chat has a user message; otherwise the chat view's idea, `pickIdea`), `onChange: onComposerChange`, `onKeyDown`, `onPaste`, `onCaret` (keeps the last caret range for inserting prompts).

The footer: an `Attach file` button (a plus) that clicks the hidden file input (multiple files, any type); choosing files runs `addFiles` (notice on failure) and clears the input so the same file can be picked again. A prompts button (title `Prompts (type / in the message)`, label `Insert a saved prompt`, active while the palette was opened with search) toggling the palette with its search. On the new bench, a `Blind` toggle (pressed while on; icon `eye-off` when on, `eye` when off; title `Blind: models go by letters until you reveal them` when on, `Names shown: turn on to judge without knowing which model is which` when off). Then the mode's controls (6.6).

There is no drag-and-drop of files anywhere in web/app.js (no `drop` or `dragover` handler); files come only from the picker and from paste. This is a fact of the current page; the brief's mention of drag and drop has nothing to match here.

### 6.9 Held-send line texts (setup-status.mjs)

`Not sent yet: a step failed. Try it again above.` when a needed setup step failed (no working dots), else `Sends when the app is ready. ${done} of ${total} done.` (with working dots). Recorded with the rest of setup-status.mjs in 8.10.

### 6.10 Drafts per place (draft.mjs)

A draft is `{ segments, attachments, selections, reading, choice, edit }` (`EMPTY_DRAFT` all empty, `choice` and `edit` null). `reading` holds `{ name, size }` per file still being read, added and removed a batch (one pick) at a time; a draft that is reading is not sent. `edit` is null or `{ of, before }`: the prompt or round being re-written and what the composer held before. `draftEmpty`: no edit, no attachments, no selections, nothing reading, and no segment other than blank text (a prompt token or a fill-in makes it non-empty). `draftFiles` is attachments plus reading: what the limits count. `beginRead`/`endRead` add a batch and replace it with the files read (none when it failed), in the draft or in what its edit put aside; `endRead` throws when the draft does not hold the batch. `beginEdit` (throws when already an edit) puts the prompt in the composer with its files as a reading batch and the old content aside; the model choice stays. `endEdit` (throws when not an edit) brings back what was aside, keeping the model chosen since. `afterSend`: empty, or after an edit what was held before it. `sentBack(sent, now)`: a message that did not go out comes back unless something new was written there since (then `now` stays and the failed message is lost from the composer; it is not merged); an edit always comes back, with `now` put aside again (throws if `now` is itself an edit).

`Drafts` keeps the drafts of places not on screen by draft key; a draft is parked or in the composer, never both (`park` throws if already parked). `take` removes and returns it (or `EMPTY_DRAFT`); `peek` reads; `put` places a prompt there from elsewhere keeping that place's model choice; `putBack` applies `sentBack`; `drop` forgets a gone place's draft; `endRead` finishes a batch in whichever parked draft holds it. Drafts live only in memory: a reload loses every unsent draft. `test/draft.test.js` states these.

Draft keys: `chat:new` (the new chat, the same before and after its row exists), `chat:<id>`, `bench:new`, `bench:<benchId>` (place.mjs).

### 6.11 Composer segments, prompt tokens and fill-ins (composer.mjs)

The composer's content is plain text with two kinds of inline atoms, serialised as segments: `{ type: 'text', text }`, `{ type: 'prompt', id, title, text }` (a prompt token, shown as `/` plus the prompt's title or `Untitled prompt`, hover title the prompt's text, with a remove button labelled `Remove /${title}` that is not reachable by Tab), and `{ type: 'field', name, value, from }` (a fill-in box from a prompt's `{{name}}`, an input whose placeholder is the name, sized to the longer of its value and placeholder, at least 2, plus one character; `from` is the prompt `{ id, title }`). `promptSegments(prompt)`: a prompt without `{{…}}` (the pattern `\{\{([^{}\n]*)\}\}`) is one token; one with fill-ins arrives as its text with a box in place of each (names trimmed), and the text around the boxes is plain editable text. `promptLinks(segments)`: the library prompts the message came from, in order of appearance, each once; a prompt counts while its token or any of its boxes is still in the message. `read(root)` gives the segments, the text a send carries (tokens expanded to their text, boxes to their values, `<br>` as newline, a block `div`/`p` starting a new line, zero-width spaces removed) and the first box left blank (`emptyField`), which blocks a send. `slashAtCaret(root)`: a `/query` just before a collapsed caret, at the text's start or after whitespace, where the query has no whitespace or `/`. `insert(root, segments, range)` replaces the range with the segments, followed by a zero-width space so the caret has a place after a trailing atom, leaves the caret after them, and returns the first box. `fields(root)` lists the boxes for Tab. `test/composer.test.js` states these.

### 6.12 Attachment limits

`checkAttachmentLimits` is defined before App (other agent's range, with `readUpload` and upload-image.mjs). `App` calls it with every file of the draft (`draftFiles`, read or still reading) and the new files, for the picker and for a paste of a copied message; it throws with its message (shown as a notice) and nothing is added.


## 7. Bench

### 7.1 Bench places and moving between modes (lines 4780 to 4842)

`roundFor(benchId)` is the round on screen for the bench, else the bench's `rounds` (its last), else 1. `showBenchView(view)` (4785) is every Bench navigation: a `member` view (one model's chat in a bench) opens that chat as a bench place (`openConversation(view.memberId, view)`); any other leaves, and for a `compare` view sets the round shown (`roundFor`) and, if the bench's details failed to load before, reads them again (clicking the bench retries); then arrives and clears the transcript. `showRound(benchId, round)` sets the round a comparison shows, and so the world its viewer shows and its draft key.

`benchFromBuild` (4810), going to the new bench with what Build's composer holds: the prompt (text and files, not selections, which belong to one build, and not the model choice) moves to the new bench and Build's draft is emptied, but only if Build's draft is not empty and the new bench's draft is empty; a prompt already typed for the new bench stays and Build's stays in Build. The composer takes focus on the next frame. `buildFromBench` (4822) is the reverse into the new chat (the bench keeps its models and Blind setting), then `showNewChat`, focus next frame.

### 7.2 Starting a bench and sending rounds (lines 5157 to 5194)

`takeMessage` (5159) takes what the composer would send for a bench out of it: nothing (null, the send does nothing and shows nothing) when there is no composed message, when it has neither text nor attachments, or while files are still being read. Otherwise the composer is cleared as after a send (`afterSend`, draft.mjs) and the message `{ text, prompts, draft, key, navigation }` returned.

`runBench` (5168): does nothing with fewer than 2 bench models. `POST /api/benches` with `{ text, prompts, files: attachments, members: benchModels, blind: benchBlind, resolution: benchResolution }`. On failure the message goes back into its draft (`putBack`) and the error propagates (notice). On success the new bench is expanded in the sidebar (`openBenches`), and if the user has not navigated meanwhile its comparison opens (pushing an address). The upper limit of 8 models is not checked here; it is in the picker (other agent) and recorded in section 7 where the new bench is rendered.

`sendRound(benchId)` (5183): `POST /api/benches/${benchId}/rounds` with `{ text, prompts, files }`, plus `editOf: edit.of` when the draft is an edit of a round (the send makes a branch). Failure puts the message back and propagates. Success shows the returned round number (`rounds`).

`archiveBench(benchId, evt)` (5198): `POST /api/benches/${benchId}/archive`; the bench leaves the list and the view moves on (2.1); the archive is read again if open.

### 7.3 Forking a bench model's chat to Build (lines 5204 to 5218)

`forkToBuild(member)`: one fork at a time (`forking` is true while the request runs; its button waits). `POST /api/conversations/${member.id}/fork` with `{ round }`: the member's last finished round, which is `member.round - 1` while the member is still running, else `member.round`. The returned row is put at the top of the chat list at once, and if the user has not navigated, Build opens on it. A failure propagates (notice).

### 7.4 Focusing builds in the viewer (lines 5220 to 5235)

`focusBuild(memberId)`: when the world on show is this place's and its `layout` has an entry for the member, the viewer is asked to frame that member's box (`min`/`max` with `yOffset` added to y; `at: Date.now()` makes each request distinct). `showWholeRound(benchId, round)`, clicking a round's prompt: ignored while text is selected on the page (selecting the prompt's text); a round other than the one on screen is shown; the round on screen frames the whole world's `bounds`.

### 7.5 Names, details, votes (lines 5281 to 5309)

`nameOf(row)`: a bench model's name, or `Build ${letter}` (`benchLetter`) while the bench hides names (`hidesNames(bench)`, i.e. blind and not yet revealed, defined before App). `loadBenchDetails(benchId)`: numbered per bench (a later read supersedes one on its way, so an older answer never undoes a newer vote); `detailsReading` keeps what was read before on show while the read runs and after a failure, which the comparison says and can retry; `GET /api/benches/${benchId}`. `voteRound(benchId, roundId, pick)`: `POST /api/benches/${benchId}/vote` `{ roundId, pick }`; the returned `votes` apply at once (`detailsVotes`), then the details are read again. Failure propagates.

### 7.6 Editing a round

`editRound(round)` (5272) puts the round's text (its text parts joined by newlines) and its attachments into the composer as an edit of `round.id`, with no selections, through `editPrompt` (section 5).

### 7.7 Round branches, the bench on screen, leaderboard (lines 5311 to 5360)

`branching` (5313) is true while a branch switch (of a chat or of a bench's rounds) waits for its answer; the arrows wait, so no second switch meets the first on the server. `switchRoundBranch(benchId, roundId)`: `POST /api/benches/${benchId}/branch` `{ roundId }`, then the returned `round` is shown. Failure propagates.

`viewedBench` (5325) is the bench of the place in Bench mode (null elsewhere); `viewedMembers` are its chats (rows with that `benchId`) sorted by the bench's `order`; `viewedDetails` the loaded details' data; `viewedDetailsError` the error when the details failed. Effect 5331: the details are read again whenever the viewed bench changes, or its `rounds`, `head` or `revealed` change, or any member's `round`, `runningSince` or `halted` changes (a model starts or ends a turn). Effect 5335: on every change of `place` while it is the leaderboard, `GET /api/leaderboard` sets `leaderboard` to `data.entries` (failure is a notice; the previous entries stay). Effect 5341: on the comparison, while the page is visible, every unread member is marked seen (looking at the comparison is looking at every model's result). Effects 5359 and 5360 write `mcb-open-benches` and `mcb-bench-models`.

### 7.8 The comparison (lines 6313 to 6428)

The comparison is the chat column's scroller (it takes `messagesRef`, so autoscroll applies). For each round of the bench's details, in order, a section (class `shown` on the round on screen):

The round's prompt as a user block (class `editing` when the round is being edited): its text parts joined by newlines and its files (`renderFiles`: images open the lightbox, others are links, as in a user message). Clicking the prompt shows that round, or on the round on screen frames every build (`showWholeRound`; ignored while text is selected). Its tools: when the round has more than one branch, a branch picker (label `Round branch`, `Previous branch`, `${index + 1} / ${count}`, `Next branch`), disabled while any model runs, while changing, or with no branch that way; and `Edit and branch` when no model is running (disabled while changing), which puts the round into the composer (`editRound`).

Then a list (label `Round ${n}`) of one `BenchResultRow` per member (component before App), given: `member` (the row), `letter` (the member's letter while names are hidden, else null), `picked` (the blind vote's pick is this member), `onVote` (only when the round is votable and this member finished the round: votes for it, or takes the vote back if it is already the pick), `state` (`roundState(member, n)`, defined before App: e.g. `done`, `running`, `queued`), `stats` (the details' `stats[memberId][roundIndex]`), `onFocus` (on the round on screen frames that build, else shows the round), `onOpen` (opens the member's chat), `onStop` (`POST /api/conversations/${memberId}/stop`, notice on failure), `onRetry` (`POST /api/benches/${benchId}/retry` `{ memberId }`, notice on failure). Retry one model is therefore this row's action; its visibility rule is inside `BenchResultRow`.

Voting rules: a round is votable only while the bench hides its models' names (blind, not revealed), no member is still `running` or `queued` on it, and at least two members finished it (`done`). Only a blind vote is shown (`votes[roundId].blind`); a vote kept from a bench that showed its names never counted and is not shown. A vote sends `pick` as the member's id, or `'tie'` for a tie; clicking the current pick takes it back by sending `pick: null`. A stored tie has `pick === null` (with the vote present), which is how a tie is told from no vote (`vote` undefined). When votable, a tie row follows: `Tie` with the note `no build is better`, and a button titled `Call this round a tie` (label `Call this round a tie`), or when the tie is the pick, titled `Your call for this round. Click to take it back` (label `Take back the tie`, pressed), showing `Tie`. After the reveal, a blind tie stays as a mark: `Tie` with `no build was better` and a mark labelled `Your blind call for this round: a tie`.

After the rounds, when the details failed: `This bench could not be read: ${error}` with a `Read again` link. What was read before stays on show above it.

### 7.9 The new bench (lines 6812 to 6857)

The grass block at 56 px, `What should they build?`, then the line-up: one `BenchModelChip` (component before App) per chosen model, keyed by provider, model and effort, given `entry`, `taken` (the efforts already chosen for the same provider and model), `onEffort(value)` (sets that entry's effort), `onCopy(value, shown)` (only while fewer than 8: replaces the entry by two copies, the first with effort `shown`, the second with `value`; at 8 nothing happens), `onRemove`. Then, once the list is ready, the bench models picker (`ModelPicker` in multi-select: `value: null`, `selected: benchModels`, `onToggle` removes a chosen provider and model (every effort of it) or adds it with effort `''` while fewer than 8 are chosen, ignoring the add at 8; `favorites`, `onToggleFavorite`, `requireTools: true`, `onSetup`, `label: 'Bench models'`, `emptyLabel` `Add` when some are chosen else `Models`); a click on it first sets the popover's height cap (`fitBenchPicker`). Then, while no AI is connected, the connect hint; otherwise `Choose at least two models` while fewer than two are chosen, and `Or have one model build it in` with a `Build` link (`buildFromBench`). The chosen models persist in `mcb-bench-models`; Blind in `mcb-bench-blind` (default on; its toggle is in the composer's footer, 6.8); the resolution in `mcb-bench-resolution`. The 2 to 8 rule: send is disabled below 2 (and `runBench` returns), the picker and copy refuse beyond 8.

### 7.10 The leaderboard (lines 6874 to 6894)

While `leaderboard` is null (not read yet) nothing shows. Empty: `No blind votes yet` and `Vote on a blind bench's rounds and the models are ranked here.`. Otherwise a table (label `Models ranked by blind votes`) with the headers `#`, `Model`, `Rating`, `Picked`, `Benches`, one row per entry in the server's order: the rank (index + 1), the vendor logo (`vendorIcon(provider, model)`) with `modelLabel(model)` and the effort when there is one, the `rating`, `${wins} / ${votes}` titled `Picked as best in ${wins} of the ${votes} votes it was in`, and `benches`. The entries are keyed by model and effort.

### 7.11 How a bench's chats are shown, status and ordering

A bench's models are never shown side by side as separate panes: the comparison lists, per round, one result row per model in the bench's `order`, and the viewer shows one world for the round (`bench:${benchId}:${round}`) with every model's build laid out in it (`layout`), each labelled with its model's name or letter (9.1). Pressing `1` to `9` (type-ahead) or a row's focus action frames one build; clicking the round's prompt frames them all. The round switch in the header (more than one round) and clicking another round's prompt or row change the round, and so the world. One model's own chat opens as a member place (its row's open action, or its row in the sidebar), at `/bench/<id>/<memberId>`, read only, with its own world, a breadcrumb back to the comparison, Reveal and Fork to Build in the header. Benches are listed in the server's order (the list stream's `benches`); the landing in Bench opens `benches[0]`, the newest. A bench's sidebar dot is `running` while any model runs, `unread` while some model finished unseen and none runs; opening the comparison marks every member seen. A bench's resolution is fixed at creation and shown read only on its comparison above Standard.


## 8. Prompts library, share panel, published list, settings and setup

### 8.1 Settings open, read, save, check (lines 4272 to 4426)

`openSettings(view = firstPane(providers))` opens Settings on a pane: by default Connect an AI while no AI is connected, General once one is. If Settings are already open, it only turns to `view`, keeping the unsaved draft. If they are being saved it throws `Settings are being saved` (shown as a notice by callers that `.catch(showError)`). Otherwise the panel goes to `{ phase: 'opening', view }`, and if it was closed `readSettings()` starts; an opening already in flight is reused (it will open on the newer view, since `readSettings` reads `panelRef.current.view` when it lands).

`readSettings` (4287): `GET /api/settings`; if the saved `titleModel` is set, its model info is loaded first (failures ignored) so the panel opens complete (null `titleModel` means titles are written by each chat's own model). A failed read closes the panel and throws (the caller shows a notice). On success: bump `checkRef`, start `loadPublished()`, and open with `draft = { ...saved, providers: each connection made editable }`, `error: ''`. `editable(connection)` clears `apiKey` to `''` for a connection whose key comes from the environment (`envKey`).

`closeSettings` (4309) closes only open Settings, dropping the draft. Settings being saved close or not with the save's answer; Escape does nothing to them.

`recheckSettings` (4378), the "Check again" action: bump `checkRef`, `GET /api/settings?refresh=1` (re-reads logins, keys and CLI versions); a check that a later check, save or opening came after lands nowhere. Otherwise `providers` takes the fresh state at once (pickers update), and if Settings are open the draft takes `checkedAt` and each provider's fresh fields while keeping the draft's `apiKey`, `enabled`, `hiddenModels` and `modelOrder`. Its failure propagates to the caller.

The panel's phases (settings-panel.mjs): closed to opening; opening to opening (another pane asked for meanwhile), open, or closed (the read failed); open to open, saving or closed; saving to open (failed, with its error) or closed (others throw). Only open and saving hold a draft; any edit clears the last save's error. `withShareRepo` trims the typed repository and saves an empty field as null (sharing off). `withTitleModel` sets `{ provider, model, effort: '' }` or null. `usable(provider)` is connected and enabled.

`saveSettings` (4399): the panel goes to `saving` (no edits, no second save, no closing), bump `checkRef`, `PUT /api/settings` with the whole draft as JSON. On failure Settings reopen with the draft and `error` set to the message (shown inside Settings). On success `providers = result.providers` and Settings close.

`editSettings(change)` applies a draft change through `panelEdit`, which throws unless Settings are open. `showSettingsPane(view)` changes pane without counting as an edit (a save's error stays). `updateConnection(id, field, value)` edits one provider field in the draft. `toggleFavorite(key)` stars a model; before the favorites loaded it throws and the notice shows why (favorites.mjs).

### 8.2 Published list (lines 4229 to 4262)

`loadPublished` numbers the read, sets `publishedReading`, `GET /api/shares`; only the latest read lands (`publishedRead` or `publishedFailed`). `removePublished(build)` (Settings' Remove on a published build): marks the row removing; if this page holds the chat or bench of that build as shared (`publishedSubject(build)` and its share in phase `shared`), its share goes through the same removing moves so its header reads right; `DELETE /api/shares/${encodeURIComponent(shareId)}`. On failure the row is kept with the error (`publishedKept`) and the share back to shared (`shareKept`). On success the share is `shareRemoved`, the row `publishedRemoved`, and the list is read again. If Settings closed meanwhile, the list's row is gone and nothing is updated in it (`shown()`).

### 8.3 Share panel (lines 4313 to 4359)

`openShare(subject)`: sets `shareFor`; reads `GET /api/settings` for `shareRepo` at each opening (Settings may have changed it). The share itself is read only the first time (phase `unknown`): `shareReading`, `GET shareUrl(subject)`; failure sets `shareUnread` and throws; success `shareRead`. `publishShare(subject)`: `sharePublishing`, `POST shareUrl(subject)`; a non-OK answer throws its JSON `error` (a refusal: 400, or 409 while a turn runs); the body is a stream read by `readShareStream`, each message applied with `shareStep`; any failure `shareFailed` (shown in the panel, not as a notice). A stream that ends without its final message is `shareStoppedEarly`. `removeShare(subject)`: `shareRemoving`, `DELETE shareUrl(subject)`; failure `shareKept` and throws (one that fails stays shared and says why); success `shareRemoved`. Shares are kept per subject (`shareKey`), so a publish that runs on while another place is on show lands on its own subject.

### 8.4 Prompt library actions (lines 5739 to 5806)

`createPrompt()` (New prompt): a new prompt `{ id: random UUID, title: '', text: '', attachments: [], tags: [] }` held as `newPrompt` (not in the library) and opened. `openPrompt(id)`: another prompt opened lets go of a new one left empty; sets the active prompt and goes to Bench's `prompt` place (the library lives in Bench). `benchPrompt(prompt)` (Bench this prompt): the new bench opens with the composer holding the prompt's segments (`composer.promptSegments`) and its attachments marked `fromPrompt`, replacing whatever the new bench's draft held; the prompt's `usedAt` is set; the composer takes focus next frame. `updatePrompt(id, patch)` (`patch` is fields or a function of the prompt): the first edit of the new prompt adds it to the library, edited; otherwise the prompt is patched in the list (a prompt gone from the list, e.g. changed by a builder via the `prompt_library` tool, is left be). `togglePromptFavorite(id)` flips `favorite`. `deletePrompt(id)` removes it; if it was the active one, the active becomes the library's first prompt (or none). There is no confirmation in this function (any confirmation would be in the component). `reorderPrompts(from, to)`: indices into the unfiltered list; the prompt moves to where the one at `to` is, by ids (after it when moving down, before it when moving up); the open prompt stays open.

### 8.5 The Settings panel as `App` renders it (lines 7046 to 7218)

Settings render inline in `App` while the panel has a draft (phases open and saving; nothing renders while opening). An overlay; a click on the overlay outside the panel closes Settings (dropping the draft; ignored while saving). While saving, the panel's body is `inert` (no interaction).

Navigation (label `Settings sections`): the heading `Settings`; `Connect an AI` (plug icon) and `General` (wrench icon), each active on its pane; then `Providers` with one `SettingsNavItem` (component before App) per non-harness provider in the draft, and `Harnesses` with one per harness, each given `id`, `connection`, `active`, `onSelect: showSettingsPane`, `onToggle(target, on)` setting the draft's `enabled`.

The main area: a close button (`Close settings`); the pane; under it the save error when there is one (`panel.error`, with a `Dismiss error` button that clears it by a no-op edit, since any edit clears the error); and the actions `Cancel` (closes, dropping the draft) and `Save changes` (`Saving…` while saving; both disabled while saving).

General pane: heading `General`. On a hosted instance a section `Account` with the row `Your account` (help `Sign out of the website, or delete this workspace and everything in it.`) and a `Manage account` link to `/account`. Section `Conversations`, row `Title model` (tip `Names each new chat from its first message`): a `ModelPicker` over the draft's providers with `value: titleModel`, `label: 'Title model'`, `emptyLabel: "The chat's own model"`, `onSetup: showSettingsPane`, favorites, changing it through `withTitleModel` (settings-panel.mjs); when the title model reasons and has efforts, an `EffortPicker` labelled `Title model reasoning effort` editing `titleModel.effort`; when a title model is set, a `Use the chat's own model` link setting it to null. Section `Appearance`, row `Theme` (tip `Applies right away on this browser`): a group (`Color theme`) of `Light` and `Dark` buttons, the current one pressed; this changes the theme at once, outside the draft (Cancel does not undo it). Section `Sharing`, row `GitHub repository for shared builds` (help `A public repository with GitHub Pages on. Needs git and gh signed in on this machine.`): an input `#share-repo`, monospace, placeholder `owner/name`, editing the draft through `withShareRepo`. Then `PublishedBuilds` (component before App) given `published`, `onRetry: loadPublished`, `onRemove: removePublished` (notice on failure), `onGoTo(subject)` (closes Settings, then opens the bench's comparison or the chat), `onError`. Then `BindsSection` given `binds`, `onChange(action, combo)` (sets that action's combination and unbinds (null) any other action that had the same combination), `onReset` (all defaults); binds save at once to `mcb-binds`, outside the draft. Then `DangerZone` (component before App) with `onClearVotes`: `DELETE /api/votes`, then every in-flight details read is disowned (`detailsRef.latest = {}`, since reads on their way hold the old votes), every loaded bench's details get empty votes, and the leaderboard becomes empty; errors through `onError`.

Connect an AI pane: `ConnectPane` (component before App) given the draft's `providers`, `onSelect: showSettingsPane`, `onConnect(id)` (records `signInStart = { id, method: first sign-in method }` and turns to that provider's pane).

A provider's pane: `ProviderPane` (component before App) given `id`, `connection` (the draft's), `favorites`, `onToggleFavorite`, `onChange(field, value)` (`updateConnection`), `checkedAt`, `onRecheck` (`recheckSettings`, notice on failure), `startWith` (the pending sign-in when it is for this provider), `onStarted` (clears it), `onError`.

### 8.6 Starred models (favorites.mjs)

The starred list lives on the server, read once with the config (`load`; a second load throws). Phases: `unloaded` (nothing can be starred: toggling throws `The favorites have not loaded, so they cannot be changed yet`, shown as a notice), `idle`, `saving`. A star shows at once; toggles are saved one at a time, in order, each as the server's list with that one toggle made (`PUT /api/model-favorites`); the list shown is the server's list with the unsent toggles applied. A save that fails drops only its own toggle (its star goes back) and shows `The star was not saved, and went back: …`; the next toggle is then saved. `test/favorites.test.js` states these.

### 8.7 The prompt library's sync (prompt-sync.mjs)

The library lives on the server and the builders can change it too (the `prompt_library` tool). Local edits show at once and reach the server shortly after. Phases: `unloaded`, `loading` (the first read; no list to edit), `failed` (`error`; `load()` reads again), `ready`, `saving`, `reloading`. Transitions: unloaded to loading; loading to ready or failed; failed to loading; ready to saving or reloading; saving and reloading to ready (others throw). Editable (`promptsLoaded`) in ready, saving and reloading; an edit before throws `The prompt library has not loaded, so it cannot be changed yet` (a notice). Each edit is a function of the list, applied at once and kept in `unsent` until a save that carried it succeeds. In ready with nothing on its way: a stale list (the server's changed: `reload()`, called when a turn's `prompt_library` tool result succeeds) is read again first, and the unsent edits are made again on the server's new list, so neither side's change is lost; otherwise unsent edits are saved 300 ms after the last edit (`PUT /api/prompts` with the whole list). One request at a time. A save that fails keeps its edits unsent and is not retried by itself: the next edit, or a reload, sends them (a notice shows the error). A reread that fails leaves the list stale, so the next edit reads again before saving. A failed first load shows `Prompts failed to load.` with `Try again` in Bench's sidebar, and the palette's `onRetry`. `test/prompt-sync.test.js` states these.

### 8.8 The share of a subject (share.mjs)

A subject is `{ kind: 'chat' | 'bench', id }`; its key `${kind}:${id}` (another kind throws); its route `/api/conversations/${id}/share` or `/api/benches/${id}/share`. States: `unknown` (not read), `loading`, `none` (not shared), `shared` (live at `url`), `exporting`, `publishing`, `waiting` (a publish on its way, with the step the server last said; `url` the old one or null), `failed` (`error`, the server's message; `url` kept), `removing`. Transitions: unknown to loading; loading to none, shared or unknown (a failed read: read again at the next opening); none to exporting; shared to exporting or removing; exporting to exporting, publishing or failed; publishing to waiting or failed; waiting to shared or failed; failed to exporting; removing to none or shared (others throw). The publish stream is NDJSON; each message's `state` is `live` (shared at `message.url`), `failed` (with `error`), or a step name. A stream that ends while still publishing is failed with `The publish stopped before the page was live`. Step texts shown by the panel: `Preparing the build…` (exporting), `Uploading…` (publishing), `Waiting for GitHub Pages…` (waiting). `postUrl(url)` is the X post link `https://x.com/intent/post?url=…`. `readShareStream` parses complete lines only; a last line without a trailing newline is not read. `test/share.test.js` states these.

### 8.9 The published list (published.mjs)

States: `unread` (not read since Settings last opened), `loading` (`data` an earlier read's, or null), `loaded` (`data` is `{ repo, builds }`), `failed` (`error`), each with `removing` (shareIds whose removal is on its way) and `errors` (shareId to why its last removal failed). Transitions: unread to loading; loading to loading, loaded, failed or unread; loaded and failed to loading or unread. What the list shows (`publishedView`): `loading` before the first read lands, `failed`, `no-repo` when no repository is set, `empty`, or `list`. A removal starts only for a build on show and not already being removed (else throws), clearing its last error; a removed build leaves the list before the re-read; a failed one stays with its error. `publishedSubject(build)`: the chat or bench it was made from, or null when that was deleted (a build of another kind throws). `publishedAgo(iso, now)`: `${n} ${unit} ago` / `${n} ${unit}s ago` in the largest whole unit among year (365 days), month (30 days), day, hour, minute; else `just now`. `test/published.test.js` states these. The texts of the list itself are the other agent's (`PublishedBuilds`).

### 8.10 Setup steps (setup-status.mjs)

The server's view (`GET /api/setup`, from server/setup.js) is read as it is: each step's state is `waiting`, `running`, `done`, `failed` or `unavailable` (any other throws `A setup step cannot be ${state}`). A step's row: label the step's `doing` while running, else its `title`; value `Waiting`, the download's progress while running (`${done} of ${total} MB`, or `${done} MB` with no total; MB is bytes / 10^6 rounded) or empty, `Done`, `Failed`, `Not available`; note the step's `detail` while running, else its `error`, or none. `setupStatus`: the rows, and among the steps the app needs (not `optional`) how many are done, the total, and whether one failed. `heldLine`: `Not sent yet: a step failed. Try it again above.` when a needed step failed, else `Sends when the app is ready. ${done} of ${total} done.`. `downloadSize(bytes)` is `${n} MB`. `linkParts(text)` splits a note into text and web addresses (an address ends at a space or a closing bracket; a trailing full stop or comma belongs to the sentence), so the setup screen links them. `test/setup-status.test.js` states these.

The held line's `Try it again above.` refers to the `SetupStatus` steps, which `App` shows only in Build on an empty transcript. A send held in a chat that already has messages, or on a bench, shows this line with no steps above it (no retry control visible in that view). This reads as a gap in the current page.

### 8.11 Shared server data (shared.mjs)

`Shared` caches server reads that several views use (model catalogs, model capabilities) by URL: fetched once and shared, so a view mounted after it landed renders it at once; a reload (`fresh`) keeps the old value on show until the new one arrives; only the entry's current request settles it (a superseded request still answers its caller but writes nothing); a failed read clears the request so the next load tries again. `App` uses it through `load(modelInfoUrl(...))`, `load(modelsUrl(id))` and `useModelInfo` (defined before App) to warm the pickers. `test/shared.test.js` states this.

### 8.12 Bench details (bench-details.mjs)

Per bench: `loading` (with the earlier read's `data` or null), `loaded`, `failed` (earlier data kept, `error`). Loading can follow anything; loaded and failed only follow loading (a land or failure without a begun read throws). `detailsVotes` replaces the held data's votes at once (a vote cast, all votes cleared), whatever read is on its way; it throws when the bench was never read. `test/bench-details.test.js` states these.


## 9. The viewer as `App` drives it

### 9.1 Labels and selection boxes given to the viewer (lines 5346 to 5397)

`viewerLabels`: only for a bench world (`viewKey` starting `bench:`) that is the one on show and has a `layout`: one label per laid-out member, text `nameOf(row)` (the model's name, or `Build A`… while blind; empty if the row is unknown), placed at the centre of its box in x and z (`(min + max + 1) / 2`) and 2 blocks above its top plus `yOffset`.

`addSelection(box)` (5364), a box picked in the viewer: its corners are converted to the build's coordinates (y minus `yOffset`), `POST /api/conversations/${chatId}/selection` `{ min, max }` answers `{ blocks, block, label }` (what it mostly holds and where it sits); if the user has not navigated, it joins the draft's selections as the next free letter `A` to `Z`; with all 26 used, the box is silently not added. Failure propagates to the caller. `removeSelection(letter)` drops one.

`viewerSelections`: only in Build with a chat and a known `yOffset`. Each draft selection is drawn with its letter label, its block's icon (`blockIconUrl(block)`), lifted by `yOffset`, lit while its chip is hovered, removable from the viewer. While a sent message's chip is hovered, that selection's box alone is drawn lit with no label (key `sent`), not removable.

### 9.2 The viewer's place and what `App` gives it (lines 7002 to 7036)

The divider (`strata`, title `Drag to resize`) sits between the chat column and the viewer; mouse down starts the split drag (3.2). It has no double-click handler: the split itself has no double-click behaviour. Both the divider and the viewer are hidden on the prompt library.

The viewer section (class `full` in full screen) is titled `Double-click or press Esc to exit fullscreen` in full screen, else `Double-click to expand`; a double-click anywhere on it toggles full screen. Escape (as a layer) leaves full screen. Full screen is not kept across reloads.

`BlueMapViewer` (component before App, the viewer agent's) is given:

| Prop | Value |
|---|---|
| `live` | `minecraftLive`: the live world's status (status, conversationId, address, error, syncError, viewerUrl, viewerRevision, mesher, blockCount, layout, bounds, yOffset, and the live controls' fields) |
| `world` | the `LiveWorld` instance (play, pause, step, restart, export go through it) |
| `exportName` | the saved world's name (the chat's title, `${bench} round ${n}`, or `mcbench world`) |
| `onError` | `showError` (notices) |
| `activeId` | `viewKey`: the world on show (a chat id, or `bench:${benchId}:${round}`) |
| `labels` | `viewerLabels` (bench model names over their builds) |
| `focus` | `viewerFocus` (`{ min, max, at }` to frame) |
| `emptyText` | in Bench null; in Build `The world shows here once the app is ready.` while setup is preparing, else `Open a chat to view its world.` |
| `selections` | `viewerSelections` |
| `onSelect` | in Build `addSelection(box)` (notice on failure); null in Bench (no selection tool) |
| `onDeselect(key)` | removes the draft selection whose key is `draft-${letter}` |
| `binds` | `binds` |
| `onBind` | `runBind`: the viewer passes on key presses that may be the app's shortcuts |
| `typeAhead` | `typeAhead && !viewerFull`: whether the viewer should pass on printable keys |
| `onType(key)` | the type-ahead handler (11.3) |
| `full`, `onFull` | full screen state and setter |
| `onEscape` | `closeTopLayer`: Escape pressed in the viewer closes the top layer |
| `selectToggle` | the counter bumped by the `select` shortcut |

The live controls (play, pause, step, restart, tick counter; the component just before App at lines 3380 to 3392) are rendered by the viewer, not by `App`: their titles are `Pause` / `Play` (disabled when the world is stopped), `Step one tick` (disabled while playing or stopped), `Restart from the build`, and the counter `${n} ticks` titled `Ticks since the build was placed`.

What happens while a turn runs and when the world changes: the viewer shows `viewKey`'s world; `LiveWorld.show(viewKey)` runs whenever the place's world changes (2.9); a `build` tool result during a turn wakes the live world so it reads the new build (2.15); an edit's send, a branch switch and a round branch also wake it (the world went back to an earlier build). Exports (`.schem`, world), player mode, the selection tool and block clicks are inside the viewer and the live world (another agent's files); `App`'s part is the props above. See section 9.3 for live-world.mjs and live-host.mjs as far as the page's state is concerned.

### 9.3 The live world as far as the page's state goes (live-world.mjs, live-host.mjs)

Each tab runs the world of the build on show in an engine of its own (the Rust engine as WebAssembly in a Web Worker, at Minecraft's 20 ticks a second), nothing shared with other tabs; the server only says what to show. `LiveWorld.begin()` polls `GET /api/minecraft/status` (with `?view=${viewKey}` when a world is on show) every 1000 ms, or every 50 ms while it is catching up (a sync in flight, or the engine module not known yet), until `dispose`. `wake()` looks at once (and a wake during a look makes another look right after); `App` wakes it after a successful `build` tool result, after an edit's send, after a chat branch switch, and the effect on `viewKey` calls `show(key)`, which lets go of the previous world and its engine at once and starts the next view in a spare engine started ahead. A failed poll sets `pollError`, which becomes the status `error` (shown by the viewer); the poll goes on. A sync reads `GET /api/minecraft/import?view=…` (with `&since=${version}` for an edit on the world held) and places the commands, afresh or as an edit (an edit keeps water flowing and clocks ticking). The last fresh placement of each view is kept in IndexedDB (`mcbench-placements`, stores `kept` and `bytes`, up to 256 MiB, least recently kept let go first) and shown at once on a later page load while the engine places it again (a provisional base: no input, control, tick or player reaches it until it is confirmed byte for byte). So the viewer's world survives a reload through IndexedDB as a provisional picture, not as the running state.

The status `App` holds as `minecraftLive` (`snapshot()`): `status` is `idle` with no view, else `error` when the failed sync, the engine's stop reason or the poll error is set, `starting` before a base, `syncing` while syncing, else `running`; `conversationId` (the view key), `revision`, `address: null`, `error` (that error, or the server's `coreError`), `syncError` (the view's), `progress: null`, `phase` (`placing` while syncing), `syncFailed` (only a restart or a new version tries again), `frame`, `minecraftVersion`, `yOffset` (default 64), `viewerUrl`, `mesher`, `viewerError`, `viewerRevision` (the base number), `viewerBlueprint`, `viewerMask`, `bounds`, `blockCount`, `layout` (a bench round's members' boxes), `previewView: false`, and `live` (`{ playing, stopped, ticks }` once a real base is placed, else null). The tick count is emitted at most 4 times a second (every 250 ms), the last count always. `App` reads from it `conversationId`, `blockCount` (Share), `layout`, `bounds`, `yOffset` (labels, selections, focus) and passes the whole to the viewer.

Live controls (`control(action)`): `play`, `pause`, `step` (one tick while paused; a stopped world throws `The live world cannot step while stopped`), `restart` (the build placed afresh after any sync in flight; after a failed sync it tries that version again). A stopped world (the engine stopped it: `The live world stopped: ${message}`, e.g. an `unported:` block) neither plays nor steps until placed afresh; the first reason stands. A world placed afresh starts at tick 0 and plays (a paused one stays paused). A build with no blocks shows an empty world. Block clicks (`input(x, y, z, yaw)`) go to the engine only for blocks that take input (lever, button, door, trapdoor, fence gate, repeater, comparator, note block, daylight detector, decorated pot, redstone dust). Player mode (`spawnPlayer`, `setPlayerInput`, `removePlayer`, `onPlayerGone`) starts the player beside the build on the side the camera looked from (`besideBuild`, `standBeside`), facing the build's middle; a fresh placement respawns the player where it stood. Export as a world save (`exportWorld(name)`, a zip whose spawn is above the build's middle, at most y 959) uses `exportName` as the name. The `.schem` export, the selection tool and player-mode controls are drawn by the viewer (other agent). `test/live-world.test.js`, `test/live-world-states.test.js` and `test/live-views.test.js` state these.

`browserHost()` (live-host.mjs) is what the page gives the world: `json(path, { method, body })` (fetch, rejecting with the server's JSON `error` or `HTTP ${status}`), `startEngine` (`CoreEngine.start`), blob URLs for blueprints, `sleep`, SHA-256 `digest`, and the IndexedDB placement store.


## 10. Addresses and history

### 10.1 Navigation mechanics (lines 3592 to 3611, 4437 to 4538, 4783 to 4897)

Every change of place goes through `setPlace(next)` (3592): a place other than `missing` is remembered as its mode's last place (`leftRef`), and if the place's address (`placeAddress`, place.mjs) differs from `location.pathname` it is pushed as a new history entry, or replaces the current one when the navigation runs inside `replacingAddress` (3600). Replacing is used when the page only renames the place on screen or stands in for a place that is gone: the first open from the address, a chat or bench that left the list moving to its neighbour, the new chat taking its row's address after its first message (`settleNewChat`), and (see later) other cases noted where they occur.

Every navigation calls `leaveConversation` (4491): park the composer's draft under the place's draft key, abort the turn's stream (never the turn itself, which the server keeps running), set the turn idle, bump `navigationRef` (async work started before checks it and lands nowhere), close the slash palette, and reset the autoscroll height so the new view starts pinned to the newest message. Then `arrive(next)` (4503) sets the place and takes that place's parked draft into the composer (a place with no composer gets `EMPTY_DRAFT`).

`parkDraft` (4514): the draft keeps its model choice only for the new chat (always) or for a chat while the choice differs from the chat's own row (provider, model, effort or resolution); otherwise the choice is dropped, so returning to the chat shows the chat's own model. `putBack(key, draft)` (4527) returns a message that did not go out to where it was written: into the composer if that place is on screen (`sentBack(draft, now)`, draft.mjs, merges with what is there now), else into the parked draft.

`openConversation(id, next = { kind: 'chat', id })` (4441): opening the chat already on screen only reads its transcript again if it had failed (clicking its row retries). Otherwise leave, arrive, and for a Build chat set the picker to the draft's kept choice or the row's model, effort and resolution (a bench model's chat never sets the picker). If the row is running a turn, the transcript is cleared and the join effect reads the turn, whose stream replays it from its start. Otherwise a cached copy shows at once (or the transcript clears to loading), and a fresh copy is fetched (forced fresh when a cached copy was shown); it replaces the shown copy only if still on that chat, the navigation is the same and no turn began, and only if it differs (`sameTranscript`: same message count, last message id, head id, tree size and dismissed count). A failed read shows the failed transcript pane.

`showNewChat` (4478): leave, arrive at `{ kind: 'new-chat', id: newChatRef.id ?? null }`, empty transcript shown (not loading). If the new chat's draft has a kept model choice it is applied; otherwise only the resolution resets to the last chosen one (`mcb-last-resolution`), not the one of the chat just left; the provider, model and effort stay what they were.

`openAddress(target)` (4847): `landing` opens the landing; otherwise `listedPlace(target, pathname, conversations, benches)` (place.mjs) gives the place; a `missing` one shows the missing screen, a chat opens, the new chat shows, and the rest (bench places) go through `showBenchView`. A chat or bench the lists do not have shows as missing, never another one in its place.

`openLanding` (4867), the address `/`: in Bench, the newest bench's comparison if the composer is empty (a prompt typed for the new bench before the list came stays on the new bench), else the new bench; in Build, the newest chat without a `benchId` (and what was typed into the new chat before the list came moves into that chat's composer), else the new chat.

`showMissing(missing)` (4883): leave, arrive at `{ ...missing, archived: null }`, clear the transcript, then `GET /api/archive`; if still on that navigation, the place gets `archived: true|false` according to whether the id is among the archived chats (or benches for Bench mode). The missing screen's texts are recorded where it is rendered.

Back and Forward (`popstate`, 4858): once the first list has come, the address is opened with `openAddress`; before it, the first list opens the address. No history entry is pushed by `popstate` itself because the place's address already equals `location.pathname`.

`switchMode(mode)` (4834): no-op for the current mode; Bench goes to Bench's remembered place (`goBench`); Build goes to the remembered Build chat id (`goBuild(leftRef.build.id)`).

### 10.2 Document title (lines 6103 to 6111)

`document.title` is `${placeTitle} · mcbench-lite`, or `mcbench-lite` when the place title is empty. The place title: on a missing place `Archived` (if the archive read found it archived) or `Not found` (including while `archived` is still null); in Build the chat's title or `New build`; in Bench `New bench`, the bench's title on its comparison, `Leaderboard`, `${bench title} / ${model name or Build letter}` on a member's chat, and `Prompts` on the prompt library. A comparison whose bench is not yet known gives `undefined`, so the title is `mcbench-lite` (falsy check).

### 10.3 Address forms (address.mjs) and places (place.mjs)

| Address | Place | Opens | Mode | `chatId` | `viewKey` (world) | `draftKey` (composer) |
|---|---|---|---|---|---|---|
| `/` | `{ kind: 'landing' }` (not a place) | the landing (10.1 `openLanding`) | the mode last used (`mcb-app-mode`) | n/a | n/a | n/a |
| `/new` | `{ kind: 'new-chat', id }` (`id` null until the row exists; the address stays `/new` after) | Build's new chat | build | `id` | `id` | `chat:new` |
| `/chat/<id>` | `{ kind: 'chat', id }` | a Build chat the list has | build | id | id | `chat:<id>` |
| `/bench` | `{ kind: 'new-bench' }` | a new bench | bench | null | null | `bench:new` |
| `/bench/<id>` | `{ kind: 'compare', benchId }` | a bench's comparison | bench | null | `bench:<benchId>:<round>` | `bench:<benchId>` |
| `/bench/<id>/<memberId>` | `{ kind: 'member', benchId, memberId }` | one model's chat in a bench | bench | memberId | memberId (that model's own world) | none: no composer |
| `/leaderboard` | `{ kind: 'leaderboard' }` | the leaderboard | bench | null | null | none |
| `/prompts` | `{ kind: 'prompt' }` | the prompt library | bench | null | null | none |
| the address it came from | `{ kind: 'missing', mode, id, address, archived }` | the missing screen | `mode` | null | null | none |

Ids are URI-encoded in addresses and decoded when read. `addressPlace` returns null for any other path, for a path with an empty segment (e.g. a trailing slash), or for a malformed escape. The server serves the page only at page addresses (`isPageAddress`), so the page never starts on another path; a null target reaching `startPlace` or `openAddress` would throw (no handling). Settings, the search panel and the archived section open over a place and have no address. The sign-in link keeps the path it was opened on (`?token=` on `/chat/<id>` lands on that chat; server side, recorded by the routes agent).

`startPlace(target, mode)`: while the lists load, the page shows what the address names when it needs no list (`/new`, `/bench`, `/leaderboard`, `/prompts`), Build's new chat while a `/chat/<id>` is looked up, a new bench while a `/bench/<id>` or member address is looked up, and on `/` the new bench if the last mode was Bench else the new chat. These interim places do not push history (the address equals or is replaced once the first list arrives).

`listedPlace(target, address, chats, benches)`: a chat address is missing unless the list has that chat as a Build chat (no `benchId`); a bench address is missing unless the bench is listed; a member address is missing (as a bench, with the bench's id) unless both the bench and the member chat in that bench are listed. A missing place keeps its own address. The landing throws here (it names no place).

`placeKeys` throws for a comparison whose round is not set (`The comparison of bench … has no round to show`): `showBenchView` always sets the round first. `withRow` and `asChat` throw on misuse.

Tests: `test/address.test.js` and `test/place.test.js` state these mappings; `test/page-addresses.test.js` states which paths the server serves as the page.

What pushes and what replaces history: a navigation the user makes (clicking a row, New chat, a palette item, a mode switch) pushes, while showing another round of a comparison never changes history (the round is not in the address); the first open from the address, moving off a chat or bench that left the list, and the new chat becoming its chat replace. `showMissing` sets the place again with `archived` known; its address is unchanged, so no history change. Back and Forward reopen the address (10.1).


## 11. Keyboard

### 11.1 Rebindable shortcuts (lines 4904 to 4927)

A window `keydown` listener turns each key press into a combination (`comboOf(evt)`, defined before App) and runs the action bound to it (`runBind`); a press that matches a bind has its default prevented, even when the action is held back. The viewer passes on its own key presses to `runBind` too (it is passed to the viewer). An action runs only if `bindRuns(action, layers)` (layers.mjs) allows it with the layers open; one held back by an open layer still counts as the app's key (default prevented). The binds are `binds` (`mcb-binds` in `localStorage`, merged over `DEFAULT_BINDS`); the defaults and labels are `DEFAULT_BINDS` and `BIND_ACTIONS`, defined before App (recorded in section 11.4 below after reading them).

| Action id | Does |
|---|---|
| `build` | `switchMode('build')` |
| `bench` | `switchMode('bench')` |
| `search` | toggles the palette: opens it empty, or closes it if open |
| `new` | in Bench the new bench; in Build `newConversation()` (notice if no model) |
| `sidebar` | `toggleSidebar`: the drawer when narrow, else the sidebar |
| `select` | bumps `selectToggle`: the viewer toggles its selection tool |

### 11.2 Palette keys (lines 5101 to 5105)

ArrowDown, ArrowUp and Enter in the palette's input, as recorded in 4.2. Escape closes it through the layer order.

### 11.3 Typing anywhere goes to the composer (lines 6114 to 6144)

`typeAhead` is true when the place has a composer and no modal layer is open (`modalOpen(layers)`, layers.mjs). A window `keydown` with a single printable character (`key.length === 1`), not a space, without Meta, Ctrl or Alt (Shift is allowed), not during IME composition, and whose target is not an input, textarea, select or editable element, is taken (default prevented): on a bench comparison the digits `1` to `9` frame that model's build in the viewer (the n-th member in the bench's order, if there is one); any other character goes into the composer, which takes the focus. A caret already in the composer stays where it was; from anywhere else the character goes at the end of the draft. The viewer passes on keys pressed in it (the `onTypeKey`-style callback given to the viewer, recorded in the layout section), except in full screen where they move the camera. Space is not taken (it would scroll or press buttons).

### 11.4 Defaults of the rebindable shortcuts (defined at lines 2616 to 2646, before App; listed here because `App` runs them)

`Mod` is Cmd on a Mac (`navigator.platform` matching `Mac|iPhone|iPad`) and Ctrl elsewhere. A combination needs a modifier (Mod, Alt, or Ctrl on a Mac) or is an F key, so typing never triggers one. Letter and digit keys are read from the physical key (`evt.code` `KeyX`, `DigitN`), so a layout or Shift does not change the combination's key.

| Action id | Label (Settings, palette) | Default |
|---|---|---|
| `build` | `Switch to Build` | `Mod+1` |
| `bench` | `Switch to Bench` | `Mod+2` |
| `search` | `Search everything` | `Mod+K` |
| `new` | `New chat or bench` | `Mod+Shift+O` |
| `sidebar` | `Show or hide the sidebar` | `Mod+B` |
| `select` | `Select blocks in the viewer` | `Alt+S` |

A bind shows as `⌘`, `⌃`, `⌥`, `⇧` joined without separators on a Mac, or `Ctrl`, `Alt`, `Shift` joined with `+` elsewhere; an unset bind shows `Not set`. Settings' recording of binds is the other agent's (`BindsSection`). `App` passes Settings `binds` and an `onChange`/`onReset` recorded in section 8 where Settings is rendered.

### 11.5 Escape's precedence (lines 4102 to 4143; layers.mjs)

The layers, topmost first (style.css stacks them in this order): `command` (the palette, modal), `lightbox` (modal), `settings` (modal), `drawer` (the narrow list drawer, modal), `viewer` (full screen, not modal), `share` (the Share panel, not modal). A window-level Escape, wherever the focus is, closes the topmost open layer only (`topLayer`), reading the layers as last rendered (so an Escape right after a layer opens closes that one). Closing Settings this way is `closeSettings`, which does nothing while saving (and the Escape is then spent: no lower layer closes). With no layer open, Escape cancels an edit in the composer's draft, unless it was pressed in an input, textarea, select or editable element (the composer is editable, and handles its own Escape first: see below). The viewer, whose frame keeps its own keys, passes Escape on through `onEscape` (`closeTopLayer`).

Escapes that are handled before the window listener and stop there: in the composer, Escape with the prompt palette open closes the palette and refocuses the composer (`stopPropagation`); Escape while editing cancels the edit (`stopPropagation`). Popovers stop their own Escape (the comment at 4103; inside the other agent's components). In the sidebar search, Escape clears the search and blurs the field, without stopping it: the window listener also runs, closing the top layer if one is open (in the narrow drawer that is the drawer itself), and with none open doing nothing (the target is an input, so an edit is not cancelled).

Modal layers also hold back binds: `bindRuns(action, layers)` lets `search` run always (it opens and closes the palette from anywhere), lets `sidebar` run when the drawer is the top layer (closing it), and holds back every other bind while a modal layer is open. Type-ahead (11.3) is off while a modal layer is open. `test/layers.test.js` states the order and the binds.

### 11.6 Every key handler in this range, in one table

| Line | Where it listens | Keys and conditions | Does |
|---|---|---|---|
| 4140 | window `keydown` | `Escape`, any focus | closes the top layer, else cancels an edit (not from another field) (11.5) |
| 4924 | window `keydown` | any combination with Mod, Alt or (Mac) Ctrl, or an F key, equal to a bind | runs the bind if `bindRuns` allows; default prevented either way (11.1) |
| 6136 | window `keydown` | one printable character other than space, no Meta/Ctrl/Alt, not composing, target not a field, a composer on the place, no modal layer | on a comparison `1` to `9` frame that model's build; else the character goes into the composer, which takes the focus (11.3) |
| 5101 | the palette's input | `ArrowDown`, `ArrowUp` (with items), `Enter` (with a highlighted item) | move the highlight with wrap-round; run the item (4.2) |
| 5681 | the composer, while the prompt palette is open | `ArrowDown`, `ArrowUp` (wrap round), `Enter` without Shift or `Tab` (with a prompt), `Escape` | move; insert the prompt; close the palette and refocus (6.5) |
| 5706 | the composer | `Escape` while editing; `Tab` / `Shift+Tab` with fill-in boxes; `Enter` without Shift and not composing | cancel the edit; cycle the boxes; submit (6.5) |
| 6572 | the sidebar search input | `Escape` | clear the search and blur |
| viewer | `BlueMapViewer` (passed `onBind`, `onType`, `onEscape`) | its own keys; passes binds, printable keys (not in full screen) and Escape on | as the three handlers above |

Mouse gestures with keyboard-like effect: double-click on the sidebar edge collapses the sidebar; double-click on the viewer toggles full screen.


## 12. Errors

### 12.1 The error boundary and the mount (lines 7223 to 7241)

`ErrorBoundary` wraps `App`. A render or an effect that throws replaces the whole page with `The page stopped on an error` (heading), the error's message in a preformatted block, and a `Reload` button (`location.reload()`); the error is logged to the console. Notices live inside `App`'s tree, so they could not say it. The mount clears `#app` (removing `Loading mcbench-lite…`) and renders `ErrorBoundary(App)` into it.

### 12.2 The page-error banner

See section 0: failures nothing else catches (uncaught exceptions, unhandled rejections, a script that failed to load) show in `#page-error` outside the app, first failure only, with `Reload`.

### 12.3 Notices

`showError(err)` (3749) adds a notice with `String(err.message || err)`; an existing notice with the same text is removed and the new one goes to the end; notices stay until dismissed one by one. Almost every user action's failure ends here via `.catch(showError)`. Failures shown elsewhere instead: a transcript read (in the transcript, `This chat could not be read: …`, `Read again`), bench details (`This bench could not be read: …`, `Read again`), the archive (`Archived items failed to load.`, `Try again`), the prompt library (`Prompts failed to load.`, `Try again`, plus a notice), the config (`Models failed to load.`, `Try again`, plus a notice), Settings' save (inside Settings), the published list and the Share panel (inside them).

### 12.4 Offline and server gone

The list stream treats a dropped connection (`TypeError`) silently and reconnects every 1000 ms while visible; only other failures show (one notice per outage, e.g. `The chat list failed to load (HTTP 502)`). A turn stream that breaks waits 1000 ms then resyncs; the join effect rejoins if the list still shows the turn running. A transcript read that failed while the server was away is read again when the list stream reconnects. The setup poll stops on its first failure (notice), and does not restart. Every other request fails with a notice carrying the fetch error's message (in Chromium, `Failed to fetch`); the request helper `api` (defined before App) decides the message for non-OK answers (other agent's range).


## 13. Server routes `App` calls

| Method | Path | Body | Line | When | Answer used for | Failure |
|---|---|---|---|---|---|---|
| PUT | `/api/model-favorites` | `{ favorites: [keys] }` | 3481 | a star toggled (Favorites write) | `data.favorites` becomes the list | the star goes back; notice `The star was not saved, and went back: …` |
| GET | `/api/prompts` | none | 3666 | mount (`loadPrompts`), `reloadPrompts` | `data.prompts` (PromptSync) | PromptSync phase failed; notice with the error |
| PUT | `/api/prompts` | `{ prompts: [list] }` | 3667 | after a library edit (PromptSync write) | PromptSync | notice; PromptSync state (see section 8) |
| GET | `/api/conversations/${id}` | none | 3769 | opening a chat, prefetch on hover, resync, read again | transcript `{ messages, messageTree, ... }`; cached per id | cache entry dropped; transcript pane failed (`readFailed`) if still on that chat |
| GET | `/api/conversations/stream` | none | 3941 | mount, reconnect 1000 ms after each end, on page visible | every line replaces the lists (section 2.1) | non-OK `The chat list failed to load (HTTP n)`, one notice per outage; dropped connections silent |
| GET | `/api/config` | none | 3980 | mount, read again from empty screens | providers, `hosted`, `modelFavorites` | `config.phase = 'failed'`, notice |
| GET | `/api/setup` | none | 4003 | mount, every 1000 ms until `ready` | setup steps (setup-status.mjs) | notice; polling stops |
| POST | `/api/conversations/${id}/seen` | none | 4052 | chat on screen is unread and page visible | nothing (list stream updates the row) | ignored |
| GET | `/api/shares` | none | 4233 | Settings open, after a removal | published list | `publishedFailed` shown in Settings |
| DELETE | `/api/shares/${encodeURIComponent(shareId)}` | none | 4252 | Remove in Settings' published list | row removed, list re-read | row kept with the error (`publishedKept`) |
| GET | `/api/settings` | none | 4290 | Settings opening | the draft | Settings close; notice |
| GET | `/api/settings` | none | 4318 | each Share panel opening | `shareRepo` | rejection from `openShare` (caller shows notice) |
| GET | `shareUrl(subject)` (share.mjs) | none | 4323 | first Share panel opening for the subject | `shareRead` | `shareUnread`; thrown |
| POST | `shareUrl(subject)` | none | 4339 | Publish in the Share panel | streamed steps (`readShareStream`) | non-OK: JSON `error` (400, 409 while a turn runs); panel shows `shareFailed` |
| DELETE | `shareUrl(subject)` | none | 4353 | Remove in the Share panel | `shareRemoved` | `shareKept`; thrown |
| GET | `/api/settings?refresh=1` | none | 4381 | Check again | `providers`, draft's `checkedAt` and provider fields | thrown to the caller |
| PUT | `/api/settings` | the whole draft | 4405 | Save | `providers`; Settings close | Settings reopen with `error` |
| GET | `modelInfoUrl(provider, model)`, `modelsUrl(id)` (via `load`, defined before App) | none | 3857, 3987, 4292 | warming the model info and catalogs | cached for pickers | ignored |
| GET | `/api/conversations/${id}/stream` | none | 4549 | joining a turn the server runs | NDJSON turn events, starting with `turn_snapshot` | 404: turn already over (finish); other non-OK: notice `error` or `HTTP n`, detach; network error: lost, retry after 1000 ms |
| POST | `/api/conversations` | `{ provider, model }` | 4660 | New chat, or a send in the new chat without a row | `data.id`: the new chat's row | request forgotten; caller sees the rejection |
| POST | `/api/conversations/${id}/archive` | none | 4710 | Archive on a chat row | nothing; the list stream removes the row | rejection to the caller (notice) |
| GET | `/api/archive` | none | 4731 | archived section opened, after each change | `{ conversations, benches }` archived rows | `archiveFailed` shown in the section |
| POST | `/api/conversations/${id}/restore`, `/api/benches/${id}/restore` | none | 4751 | Restore in the archived section | archive re-read; item opened when listed | rejection to the caller |
| DELETE | `/api/conversations/${id}`, `/api/benches/${id}` | none | 4774 | Delete in the archived section | archive re-read | rejection to the caller |
| GET | `/api/archive` | none | 4888 | the missing screen | whether the missing id is archived | notice |
| GET | `/api/search?scope=build&q=…`, `/api/search?scope=bench&q=…` | none | 4943 | palette query, 90 ms debounce | `results` (`id`, `snippet`) | notice |
| GET | `/api/search?scope=${appMode}&q=…` | none | 5145 | sidebar search, 90 ms debounce | `found` | notice |
| POST | `/api/benches` | `{ text, prompts, files, members, blind, resolution }` | 5174 | Run in the new bench | `id`; the comparison opens | message put back; notice |
| POST | `/api/benches/${id}/rounds` | `{ text, prompts, files, editOf? }` | 5188 | send in a bench's comparison | `rounds`: the round shown | message put back; notice |
| POST | `/api/benches/${id}/archive` | none | 5200 | Archive on a bench row | list stream removes it | notice |
| POST | `/api/conversations/${id}/fork` | `{ round }` | 5212 | fork a bench model's chat to Build | the new row; Build opens on it | notice |
| GET | an attachment's `file.url` | none | 5239 | edit and branch reads files back | bytes as a data URL | `Could not load "name" (HTTP n)`; edit has no files |
| GET | `/api/benches/${id}` | none | 5298 | bench details | `detailsRead` | `detailsFailed`, shown in the comparison |
| POST | `/api/benches/${id}/vote` | `{ roundId, pick }` | 5306 | a vote | `votes`, then details re-read | notice |
| POST | `/api/benches/${id}/branch` | `{ roundId }` | 5318 | bench round branch arrows | `round` shown | notice |
| GET | `/api/leaderboard` | none | 5337 | each change of place while on the leaderboard | `entries` | notice |
| POST | `/api/conversations/${id}/selection` | `{ min, max }` (build coordinates) | 5370 | a box picked in the viewer | `{ blocks, block, label }`: a selection chip | rejection to the caller |
| POST | `/api/conversations/${id}/stop` | none | 5479, 5553 | Stop | nothing; the stream ends the turn | notice |
| POST | `/api/conversations/${id}/chat` | `{ text, prompts, provider, model, files, selections, effort, resolution, editOf? }` | 5525 | send in Build | NDJSON turn events read as the turn | message put back; notice with JSON `error` or `HTTP n`; transcript restored |
| POST | `/api/conversations/${id}/branch` | `{ messageId }` | 5571 | branch arrows on a message | `{ messages, messageTree }` replaces the transcript | notice |
| POST | `/api/conversations/${id}/dismiss-error` | `{ messageId }` | 5930 | dismissing a stored error | nothing | notice |
| POST | `/api/conversations/${memberId}/stop` | none | 6383 | Stop on a model's row in the comparison | nothing | notice |
| POST | `/api/benches/${id}/retry` | `{ memberId }` | 6384 | Retry on a model's row | nothing; the list stream shows it running | notice |
| POST | `/api/benches/${id}/reveal` | none | 6698 | Reveal models, confirmed | nothing; list stream and details show names | notice |
| POST | `/api/setup/retry` | none | 6795 | Retry in the setup steps | the new setup view | notice |
| DELETE | `/api/votes` | none | 7172 | Danger zone: clear the leaderboard | every bench's votes emptied, leaderboard empty | `onError` (notice) |
| GET | `/account` (a link, not a fetch) | none | 7084 | hosted instances: Manage account | navigates the tab | n/a |
| GET | `/api/minecraft/status?view=${viewKey}` (live-world.mjs:323, through `LiveWorld`) | none | 4078 (`begin`) | every 1000 ms, 50 ms while catching up, at once on `wake` | assets, engine module, the view's version | `pollError` in the status; polling goes on |
| GET | `/api/minecraft/import?view=…&since=…` (live-world.mjs:421) | none | via `LiveWorld` | a sync | commands placed in the tab's engine | sync failed; only a restart or a new version retries |

## 14. User-visible strings

| Line | String | Where it appears |
|---|---|---|
| index.html:6 | `mcbench-lite` | the document title before the app sets one |
| index.html:15 | `Loading mcbench-lite…` | the page body until the app mounts |
| index.html:16 | `Reload` | the page-error banner's button |
| page-errors.js:20 | `The page hit an error: ${message}` | page-error banner (uncaught exception) |
| page-errors.js:21 | `The page could not load ${pathname} or a module it imports.` | page-error banner (script load failure) |
| page-errors.js:24 | `The page hit an error: ${reason.message or reason}` | page-error banner (unhandled rejection) |
| 3390 | `${n} ticks` (n with `toLocaleString`), title `Ticks since the build was placed` | live controls (component before App, rendered by App) |
| 3483 | `The star was not saved, and went back: ${message}` | notice |
| 3758 | `mcbench world` | default export file name |
| 3761 | `${bench title or 'Bench'} round ${round}` | export file name of a bench world |
| 3942 | `The chat list failed to load (HTTP ${status})` | notice |
| 4281 | `Settings are being saved` | notice (opening Settings while a save runs) |
| 4558 | `${body.error}` or `HTTP ${status}` | notice (joining a turn refused) |
| 4696 | `Choose a model before starting a chat` | notice |
| 5025 | `Builds`, `Recent builds` | palette group headings |
| 5031 | `Benches`, `Recent benches` | palette group headings |
| 5037 | `Prompts` | palette group heading |
| 5038 | `Untitled prompt` | palette item title |
| 5006 | `…` | palette prompt excerpt lead |
| 5046 | `New chat` | palette action |
| 5048 | `New bench` | palette action |
| 5049 | `Switch to Build`, `Switch to Bench` | palette action |
| 5050 | `Leaderboard` | palette action |
| 5051 | `Show the list`, `Hide the sidebar`, `Show the sidebar` | palette action |
| 5052 | `Light theme`, `Dark theme` | palette action |
| 5053 | `Settings` | palette action |
| 5055 | `Actions` | palette group heading |
| 5059 | `Title model`, `Theme`, `Clear leaderboard`, `General`, `Danger zone`, `Shortcuts` | palette setting rows and their sections |
| 5064 | `Settings · ${section}` | palette setting and provider row detail |
| 5068 | `Harnesses`, `Providers` | provider row section |
| 5074 | `Settings` | palette group heading |
| 5092 | `Search everything` | palette dialog and input label |
| 5098 | `Search builds, benches, prompts and settings` | palette placeholder |
| 5107 | `esc` | palette key cap |
| 5110 | `Nothing matches “${needle}”`, `Searching…` | palette empty states |
| 5240 | `Could not load "${name}" (HTTP ${status})` | notice (edit read-back) |
| 5284 | `Build ${letter}` | bench model's name while hidden |
| 5494 | `Connect an AI in Settings first`, `Choose a model before sending` | notice (send without a model) |
| 5534 | `${error}` or `HTTP ${status}` | notice (send refused) |
| 5670 | `${first 47 characters}…` | title of a prompt saved from the draft |
| 5730 | `New build` | the title of a chat without one (header, document title) |
| 5850 | `Conversation branch` (label), `Previous branch`, `Next branch`, `${i} / ${n}` | branch picker on a user message |
| 5868 | `Copy message` | message tool title |
| 5874 | `Edit and branch` | message tool title and label |
| 5887 | `Reasoning…`, `Reasoning` | reasoning row label |
| 5906 | `Show previous tool calls`, `+${n} previous tool call`, `+${n} previous tool calls` | tool stack fold |
| 5922 | `Dismiss error` | error dismiss button title and label |
| 5948 | `Show reasoning` | reasoning fold title |
| 5975 | `…` | tool row detail with no summary |
| 5978 | `Tool` | tool row action for an unnamed tool |
| 5981 | `Running command`, `Command failed`, `Ran command` | bash row |
| 5987 | `Inspecting`, `Inspection failed`, `Inspected` | build inspection row |
| 5990 | `Building`, `Build failed`, `Built` | build row |
| 5993 | `Inspecting build`, `Inspection failed`, `Inspected build` | see_build row |
| 5996 | `Testing in Minecraft`, `Minecraft test cancelled`, `Minecraft test failed`, `Minecraft test completed` | test_build row |
| 5999 | `${n} tests: input and block-state observations`, `Input and block-state observations` | test_build detail |
| 6001 | `Viewing image`, `Image view failed`, `Viewed image` | view_image row |
| 6014 | `+${n} lines` | tool row while its input streams |
| 6029 | `Show code` | tool fold title |
| 6043 | `Streaming tool input…` | tool fold body while input streams |
| 6072 | `Worked · ${n} step`, `Worked · ${n} steps`, `Worked` | activity fold summary |
| 6100 | `Collapse sidebar`, `Close the list`, `Benches and prompts`, `Chats` | the sidebar toggle's label |
| 6104 | `Archived`, `Not found`, `New bench`, `Leaderboard`, `${bench} / ${name}`, `Prompts` | place titles (document title) |
| 6111 | `${placeTitle} · mcbench-lite`, `mcbench-lite` | document title |
| 6147 | `Working`, `Finished, not opened yet`, `Stopped or failed`, `Idle` | status dot labels |
| 6207 | `…` | snippet lead in the sidebar and palette |
| 6230 | `Hide archived`, `Show archived` | archived section toggle |
| 6234 | `Loading…` | archived section while reading |
| 6236 | `Archived items failed to load.`, `Try again` | archived section failure |
| 6242 | `${n} model · `, `${n} models · ` | archived bench row meta prefix |
| 6271 | `Hide models`, `Show models` | bench row caret label |
| 6287 | `Archive bench (it can be restored for 7 days)`, `Archive bench` | bench row archive button |
| 6342 | `Round branch` | comparison round branch picker label |
| 6371 | `Round ${n}` | comparison results list label |
| 6391 | `Tie`, `no build is better` | comparison tie row |
| 6397 | `Your call for this round. Click to take it back`, `Call this round a tie`, `Take back the tie` | tie button title and label |
| 6409 | `no build was better`, `Your blind call for this round: a tie` | revealed tie mark |
| 6422 | `This bench could not be read: ${error}`, `Read again` | comparison failure |
| 6432 | `Connect an AI to start building` | empty screens while no AI is connected |
| 6477 | `Chat model` | model picker label |
| 6480 | `Compare several in Bench` | model picker footer |
| 6493 | `Stop generating` | Stop button title and label |
| 6499 | `Send message` | Send button title and label |
| 6508 | `Run on ${n} models`, `Send to every model` | Bench send button title and label |
| 6546 | `New bench`, `New chat` | sidebar brand button title and label (also 6660 in the header) |
| 6569 | `Search` | sidebar search placeholder |
| 6570 | `Search benches`, `Search chats` | sidebar search label |
| 6579 | `No bench was sent from a library prompt yet`, `No chat was sent from a library prompt yet` | filter popover's empty prompts text |
| 6588 | `No matches` | sidebar list when search or filters leave nothing (also 6618) |
| 6603 | `Archive build (it can be restored for 7 days)`, `Archive build` | chat row archive button |
| 6616 | `Leaderboard` | Bench sidebar row |
| 6633 | `Prompts failed to load.`, `Try again` | Bench sidebar |
| 6637 | `Models failed to load.`, `Try again` | sidebar |
| 6640 | `Settings` | sidebar foot button |
| 6646 | `Drag to resize · double-click to collapse` | sidebar edge title |
| 6655 | `Open sidebar` | header button |
| 6528 | `Show sidebar` | left-edge reveal strip title |
| 6676 | `Prompts`, `/`, `Untitled prompt` | header breadcrumb |
| 6679 | `Round shown in the world`, `Round ${n}` | round switch |
| 6695 | `Reveal models`, `Reveal for good`, `Show which model made each build. Voting ends here: the votes cast stay as they are.` | reveal button |
| 6705 | `Fork to Build`, `Fork and reveal models`, `Build shows the model, so forking reveals this bench` | fork button while blind |
| 6716 | `Continue this model's build as a new chat in Build`, `Fork to Build` | fork button |
| 6726 | `Run bench` | header on the prompt library |
| 6729 | `unknown model`, `This model is not in the provider's catalog` | header model warning |
| 6732 | `no tools`, `This model cannot call tools — the builder needs tool calling` | header model warning |
| 6742 | `Wait for the bench's running turns before sharing`, `Publish this bench's builds to a public page`, `Publish this build's 3D view to a public page`, `Share` | Share button |
| 6771 | `Dismiss error` | notice dismiss |
| 6799 | `What should we build?` | Build empty hint |
| 6800 | `Or have several models build it in`, `Bench` | Build empty hint |
| 6806 | `This chat could not be read: ${error}`, `Read again` | transcript failure |
| 6810 | `Still working, shown when it ends` | working indicator status while detached |
| 6816 | `What should they build?` | new bench |
| 6845 | `Bench models`, `Add`, `Models` | bench models picker |
| 6852 | `Choose at least two models` | new bench |
| 6853 | `Or have one model build it in`, `Build` | new bench |
| 6863 | `This bench is archived`, `This chat is archived`, `There is no bench at this address`, `There is no chat at this address` | missing screen |
| 6864 | `Looking in the archive…`, `Restore it to open it here. What is archived is deleted 7 days after it was archived.`, `It was deleted, or the address was mistyped.` | missing screen |
| 6868 | `Restore`, `New bench`, `New chat` | missing screen actions |
| 6876 | `No blind votes yet`, `Vote on a blind bench's rounds and the models are ranked here.` | leaderboard empty |
| 6878 | `Models ranked by blind votes`, `#`, `Model`, `Rating`, `Picked`, `Benches` | leaderboard table |
| 6887 | `${wins} / ${votes}`, `Picked as best in ${wins} of the ${votes} votes it was in` | leaderboard cell |
| 6900 | `Don't send` | held send line |
| 6906 | `Editing a round: sending it runs the new version on every model`, `Editing a message: sending it starts a new branch` | editing line |
| 6907 | `Cancel`, `Cancel edit`, `Cancel edit (Esc)` | editing line |
| 2621 | `Switch to Build`, `Switch to Bench`, `Search everything`, `New chat or bench`, `Show or hide the sidebar`, `Select blocks in the viewer` | bind labels (before App; shown in Settings and the palette) |
| 2643 | `Not set` | an unset bind (before App) |
| 6940 | `file` | attachment kind when the MIME type has none |
| 6942 | `Remove attachment` | attachment tile button |
| 6952 | `Reading…` | attachment tile while reading |
| 6961 | `Follow up with every model` | composer placeholder on a comparison (else the idea, or empty once prompted) |
| 6968 | `Attach file` | composer button |
| 6983 | `Prompts (type / in the message)`, `Insert a saved prompt` | composer prompts button title and label |
| 6992 | `Blind: models go by letters until you reveal them`, `Names shown: turn on to judge without knowing which model is which`, `Blind` | new bench Blind toggle |
| 7005 | `Drag to resize` | split divider title |
| 7012 | `Double-click or press Esc to exit fullscreen`, `Double-click to expand` | viewer title |
| 7023 | `The world shows here once the app is ready.`, `Open a chat to view its world.` | viewer empty text (Build) |
| 7050 | `Settings sections`, `Settings` | Settings nav label and heading |
| 7054 | `Connect an AI` | Settings nav |
| 7058 | `General` | Settings nav (and the pane heading at 7080) |
| 7060 | `Providers`, `Harnesses` | Settings nav group labels |
| 7075 | `Close settings` | Settings close button |
| 7082 | `Account`, `Your account`, `Sign out of the website, or delete this workspace and everything in it.`, `Manage account` | General, hosted only |
| 7088 | `Conversations`, `Title model`, `Names each new chat from its first message` | General |
| 7098 | `The chat's own model` | title model picker's empty label |
| 7105 | `Title model reasoning effort` | effort picker label |
| 7117 | `Use the chat's own model` | General link |
| 7122 | `Appearance`, `Theme`, `Applies right away on this browser`, `Color theme`, `Light`, `Dark` | General |
| 7140 | `Sharing`, `GitHub repository for shared builds`, `A public repository with GitHub Pages on. Needs git and gh signed in on this machine.`, `owner/name` | General |
| 7205 | `Dismiss error` | Settings save error |
| 7211 | `Cancel`, `Save changes`, `Saving…` | Settings actions |
| 7231 | `The page stopped on an error`, `Reload` | error boundary |

## 15. Workarounds, TODOs and known defects noted in the code

There are no `TODO` or `FIXME` comments in this range or in the listed modules. What reads as a workaround, a browser-specific measure, a gap or a possible defect:

| Where | What |
|---|---|
| 3399 | All running dots are re-timed to start at time zero on every `animationstart`, so they pulse in step: a browser animation workaround. |
| 3908 to 3913 | The list stream is dropped while the page is hidden because browsers allow few open requests per host; a hidden tab gets no list updates and catches up on return. |
| 3966 | The list stream reconnects every 1000 ms with no backoff and no limit. |
| 4088 to 4100 | Autoscroll compares with the previous content height, not scroll events, because scroll events are "too async to trust with deltas landing every ~40ms". |
| 4003 to 4007 | The setup poll stops at its first failed read and is never restarted; a page that lost the server during first-run setup shows a notice and stops updating the steps until reloaded. |
| 4963 to 4968 | Jumping to a setting row waits two animation frames, then assumes the row exists on the General pane (`element.scrollIntoView` on a null element would throw inside the frame callback, which only the page-error banner would catch). Every setting row the palette offers targets the General pane. |
| 5250 to 5264 | Edit and branch: one attachment that cannot be read back drops every file of that edit (the batch ends with none), while the comment says the file "is left out, and says so". |
| 5372 to 5376 | A 27th viewer selection is silently not added (letters A to Z only). |
| 5452, 5468, 6133 | Text is inserted with the deprecated `document.execCommand('insertText')` (paste as plain text, a copied message's text, type-ahead), which keeps the browser's undo history. |
| 5634 to 5636 | `void emptyField.offsetWidth` forces a reflow to restart the empty fill-in box's flash animation. |
| 6896 to 6901 with setup-status.mjs:51 | The held-send line says `Try it again above.` but the setup steps (with their retry) show only in Build on an empty transcript; a send held in a chat with messages or on a bench has nothing above it to retry. |
| 5335 to 5338 | The leaderboard is read again on every change of the `place` object while on the leaderboard (in practice once per arrival). A failed read leaves the old entries (or nothing) with only a notice. |
| 4474, 6112 | A bench member's chat has no composer and no message actions (read only); there is no Stop in the Bench composer; per-model Stop and Retry are on the comparison's rows. |
| draft.mjs:70 | A message that did not go out is not put back if something new was written in that place meanwhile; the new text stays and the failed message is dropped from the composer (the notice still shows the failure). This is the stated rule, not an accident, but it can lose the failed text. |
| share.mjs:98 to 109 | The publish stream's last line is read only if it ends with a newline. |
| address.mjs:46 to 62 with 3576 | A path that is not a page address makes `addressPlace` return null and `startPlace(null)` throw; the server never serves the page there, so this is unreachable in practice. |
| 6111 | On a comparison whose bench is not yet listed the title is `undefined`, so the tab shows `mcbench-lite`. |
| whole range | There is no file drag and drop, and no rename of a chat or bench, in `App`. |
| 3457, 6468 | The last model is kept in `mcb-last-model` only when chosen in the composer's picker; switching chats changes `provider`/`model` without writing it, so a reload starts with the model last picked, not the model of the chat last open. |


## 16. Coverage checklist

| Range or module | Status |
|---|---|
| web/app.js 3406 to 3700 (state, favorites, prompts, settings state, place) | covered |
| web/app.js 3700 to 4100 (drafts, transcripts, notices, list stream, config, setup, held, seen, join, live world, autoscroll) | covered |
| web/app.js 4100 to 4480 (layers, Escape, theme and layout persistence, sidebar and split drags, published, Settings open/save/check, Share) | covered |
| web/app.js 4480 to 4900 (navigation, turn streams, new chat, archive, bench places, addresses, landing, missing) | covered |
| web/app.js 4900 to 5320 (binds, palette, sidebar search, bench run/rounds/fork/focus, edit and branch, details, votes) | covered |
| web/app.js 5320 to 5735 (bench on screen, leaderboard, labels, selections, submit, files, paste, stop, send, copy, branch, prompt palette, composer keys) | covered |
| web/app.js 5735 to 6135 (prompt library actions, usage, transcript rendering, document title, type-ahead) | covered |
| web/app.js 6135 to 6523 (dots, filters, archive section, bench rows, comparison, composer controls) | covered |
| web/app.js 6523 to 7221 (the layout: sidebar, header, notices, content, composer, viewer, overlays, Settings) | covered |
| web/app.js 7223 to 7241 (`ErrorBoundary`, mount) | covered |
| web/address.mjs | covered |
| web/place.mjs | covered |
| web/draft.mjs | covered |
| web/transcript.mjs | covered |
| web/tool-stream.mjs | covered |
| web/composer.mjs | covered |
| web/archive.mjs | covered |
| web/filters.mjs | covered |
| web/favorites.mjs | covered |
| web/command.mjs | covered |
| web/prompt-sync.mjs | covered |
| web/share.mjs | covered |
| web/published.mjs | covered |
| web/shared.mjs | covered |
| web/bench-details.mjs | covered |
| web/setup-status.mjs | covered |
| web/live-host.mjs (as far as the page's state is concerned) | covered |
| web/live-world.mjs (as far as the page's state is concerned; read whole for the state, sync and controls; engine and viewer internals left to the viewer agent) | covered |
| web/page-errors.js | covered |
| web/theme.js | covered |
| web/index.html | covered |
| web/layers.mjs and web/settings-panel.mjs (not in the brief's list; read because `App`'s Escape, binds and Settings phases depend on them) | covered |
| app.js before 3406, read only where `App` depends on it: `BIND_ACTIONS`, `comboOf`, `formatBind` (2616 to 2646), `SIDEBAR`, `NARROW`, `initialModelSelection` (1229 to 1249), `itemsFromMessages`, `branchNavigation` (336 to 420), `Ticker` (around 300) | covered for what `App` uses |
| Tests cited: address, place, draft, transcript, tool-stream, composer, archive, filters, favorites, command, prompt-sync, share, published, shared, bench-details, setup-status, live-world, live-world-states, live-views, page-errors, page-addresses, page-input, layers, settings-panel (`test/*.test.js`) | covered |
