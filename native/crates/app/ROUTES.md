# The routes of `server/index.js` and where each lives in Rust

The server's side of the inventory (the interface's owner keeps the page's side). Every route and stream of `server/index.js` `handleApi` and `route`, with what it reads and writes, the test that defines it today, and its home now. A method named `Core::x` is `mcbench_lite::core::Core::x` (`src/core/routes/*.rs`, the lifecycle's under `mcbench_agent::lifecycle::Turns`); it is `async` unless the table says "sync", and is run on the core's runtime (`core.handle().spawn(core.x(..))`). Refusals are `Refusal { status, message }` with today's status and sentence. Fates: **command** (a `mcbench_ui::Command` the core handles), **core** (a method on `Core`, for a `Command` the interface adds), **gone** (HTTP, the page's own files, the browser renderer), **share** (inventoried only, below), **no home** (nothing serves it yet).

| # | Method and path | Reads / writes | Defined by | Fate | Home |
|---|---|---|---|---|---|
| 1 | GET `/api/minecraft/status` | the live view's version, the wasm engine's address | live-world tests (page) | gone | the live world is the core's: `Update::Live` (`core/live.rs`); frames behind the renderer's seam |
| 2 | GET `/api/minecraft/import` | the commands that take the page's world to the view's | `live-views.test.js` | gone | `core/live.rs` places the open chat's world in the in-process engine |
| 3 | GET `/api/config` | settings' providers, favourites, Minecraft's version | `provider-login.test.js` (hosted) | command | `Update::Providers` (`Core::provider_list`, Cursor left out) and `Update::Favorites` after attach and after `Command::SaveSettings`; `Core::config` for the whole body (no `hosted`: the hosted version is dropped) |
| 4 | GET `/api/setup` | setup's steps | `setup.test.js`, `setup-status.test.js` | core | `Core::setup_view` (sync); every change is `Update::Setup`. Setup is `mcbench_setup::start`, its shell step the core's (`PlatformSandbox::shell_step`: the files `mcbench_setup::shell::files` names, the platform's sandbox made of them and prepared; without them the shell step is unavailable) |
| 5 | POST `/api/setup/retry` | runs the steps again | `setup.test.js` | command | `Command::RetrySetup` → `Core::retry_setup` (sync); ends in `Update::Setup`, after `Update::Error` (`No setup step failed`) on the refusal |
| 6 | POST `/api/cli-installs` | installs a CLI into the tools folder | `cli-install.test.js` | command | `Command::InstallCli` → `Core::install_cli` (sync) over `mcbench_providers::harness::install::CliInstaller`; ends in `Update::Install`, after `Update::Error` on a refusal (then the latest install, when there is one). Each change of the install is pushed as `Update::Install` (progress at most ten a second), and its end is followed by `Update::Providers` and `Update::Settings` read fresh |
| 7 | GET `/api/cli-installs/:program` | the latest install | `cli-install.test.js` | command | no polling: `Update::Install` on each change; `SettingsView` carries each program's latest install (`Core::cli_install`, sync) |
| 8 | GET `/api/activity` | holds, titles, queued rounds | `provider-login.test.js` | core | `Core::busy`: `Turns::busy` (holds and titles under way), then the queued bench rounds |
| 9 | POST `/api/provider-logins` | starts a CLI sign-in | `provider-login.test.js` | command | `Command::SignIn` → `Core::sign_in` (sync) over `mcbench_providers::login::SignIns`; ends in `Update::SignIn`, after `Update::Error` on a refusal. Each change of the sign-in is pushed as `Update::SignIn` (for every provider of its login), and its end is followed by `Update::Providers` and `Update::Settings` read fresh |
| 10 | GET `/api/provider-logins/:provider` | the latest sign-in | `provider-login.test.js` | command | no polling: `Update::SignIn` on each change; `SettingsView` carries each provider's latest sign-in (`Core::sign_in_state`, sync) |
| 11 | DELETE `/api/provider-logins/:provider` | cancels it | `provider-login.test.js` | command | `Command::CancelSignIn` → `Core::cancel_sign_in` (sync); as row 9 |
| 12 | POST `/api/provider-logins/:provider/input` | the pasted code to the CLI | `provider-login.test.js` | command | `Command::SignInInput` → `Core::sign_in_input`; as row 9 |
| 13 | POST `/api/provider-logouts` | the CLI's logout | `provider-login.test.js` | command | `Command::SignOut` → `Core::sign_out`; ends in `Update::Providers` then `Update::Settings` read fresh, after `Update::Error` on a failure |
| 14 | PUT `/api/model-favorites` | settings' favourites | `settings.test.js` | command | `Command::SetFavorites` → `Core::set_favorites` (`Core::model_favorites` for the body); ends in `Update::Favorites` |
| 15 | GET `/api/models` | the provider's model list | `llm.test.js` | command | `Command::Models` → `Core::model_list` → `Update::Models` (`Core::models` for the body) |
| 16 | GET `/api/model-info` | a model's facts | `llm.test.js` | command | `Command::ModelInfo` → `Core::model_facts` → `Update::ModelInfo` (`Core::model_info` for the body) |
| 17 | POST `/api/prompts/generate` | Improve with AI (a completion) | `prompt-generation.test.js` | command | `Command::GeneratePrompt` → `Core::generate_prompt` (text from `mcbench_agent::texts::improve_request`) with a cancellation token per request number → `Update::Generated`; `Command::CancelGenerate` fires the token (the page's abort): `Generated::Cancelled`, nothing for a request that has ended; the core's shutdown fires every one |
| 18 | GET `/api/prompts` | `prompts.json` | `prompts.test.js` | command | `Command::ReadPrompts` → `Core::prompts` (`mcbench_store::LibraryPrompt`) → `Update::Prompts`, also after a `prompt_library` call |
| 19 | PUT `/api/prompts` | writes `prompts.json` | `prompts.test.js` | command | `Command::SavePrompts` → `Core::save_prompts` → `Update::PromptsSaved` alone (no `Update::Prompts` of its own) |
| 20 | GET `/api/settings` | settings, logins, CLI versions | `settings.test.js`, `provider-login.test.js` | command | `Command::ReadSettings` → `Core::settings_view` (the typed `SettingsView` of `Core::settings`'s body, Cursor left out) → `Update::Settings`; with `refresh`, `Update::Providers` first when the read worked |
| 21 | PUT `/api/settings` | writes `settings.json` | `settings.test.js` | command | `Command::SaveSettings` → `Core::save_settings` (the draft as the page's PUT body, Cursor's saved fields kept) → `Core::update_settings`; ends in `Update::Providers`, `Update::Favorites`, `Update::SettingsSaved(Ok)`; a refusal is `Update::SettingsSaved(Err)` alone |
| 22 | GET `/api/assets/:name` | the viewer's assets | `assets.test.js` | gone | the renderer reads `mcbench_setup::assets(...)` |
| 23 | GET `/api/block-states` | see_build page's block states | `block-states.test.js` | gone | the renderer's |
| 24 | GET `/api/block-tints` | see_build page's tints | `block-tints.test.js` | gone | the renderer's |
| 25 | GET `/api/blocks/:id/icon.png` | a block's icon | `block-icon.test.js` | no home | `mcbench_setup::icon` makes it; no `Core` method yet |
| 26 | GET `/api/images/:chat/:name` | a chat's image file | `attachment-serving.test.js` | core | `mcbench_ui::Core::image_file` (sync) over `Core::image_file`: the path and type |
| 27 | GET `/api/attachments/:chat/:name` | an attachment file | `attachment-serving.test.js` | core | `mcbench_ui::Core::attachment_file` (sync) over `Core::attachment_file`: path, type, inline or a download |
| 28 | GET `/api/conversations/stream` | the chat list, then each change | `server-archive.test.js` | command | `Update::Conversations` on each change (coalesced); `Core::conversations` for the whole `listPayload` |
| 29 | POST `/api/conversations` | a new chat row | `server-turns.test.js` | command | `Command::Send` with `Target::NewChat`; `Core::create_conversation` |
| 30 | GET `/api/search` | the search index | `store.test.js` | command | `Command::Search` → `Core::search_hits` → `Update::Found` (`Core::search` for the body) |
| 31 | GET `/api/leaderboard` | votes | `store.test.js` | command | `Command::ReadLeaderboard` → `Core::leaderboard_board` → `Update::Leaderboard` (`votes` and `entries` in the server's order; `Core::leaderboard` for the body) |
| 32 | DELETE `/api/votes` | every vote | `store.test.js` | command | `Command::ClearVotes` → `Core::clear_votes`; ends in `Update::Leaderboard`, after `Update::Error` on a failure, then `Update::Bench` for every bench whose details were asked for |
| 33 | POST `/api/benches` | a bench, its first round | `server-turns.test.js`, `resolution.test.js` | command | `Command::Send` with `Target::NewBench` → `Core::create_bench` → `Turns::create_bench`; ends in `Update::BenchSent` after the list and the details, or `Update::SendRefused` |
| 34 | GET `/api/archive` | the archive | `server-archive.test.js` | command | `Command::ReadArchive` → `Core::archive_list` → `Update::Archive` (`Core::archive` for the body) |
| 35 | POST `/api/conversations/:id/restore` | unarchives | `server-archive.test.js` | command | `Command::Restore(Subject::Chat)` → `Core::restore_conversation` |
| 36 | POST `/api/benches/:id/restore` | unarchives, advances | `server-archive.test.js` | command | `Command::Restore(Subject::Bench)` → `Core::restore_bench` |
| 37 | DELETE `/api/conversations/:id` | deletes for good | `server-archive.test.js` | command | `Command::Delete(Subject::Chat)` → `Core::delete_conversation` |
| 38 | DELETE `/api/benches/:id` | deletes for good | `server-archive.test.js` | command | `Command::Delete(Subject::Bench)` → `Core::delete_bench` |
| 39 | GET `/api/shares` | the share repository | `share-routes.test.js` | share | `Command::ReadShares` (`core/share.rs`) |
| 40 | DELETE `/api/shares/:shareId` | removes a share | `share-routes.test.js` | share | `Command::RemoveShare` (`core/share.rs`) |
| 41 | GET `/api/benches/:id` | the bench, round stats | `bench-details.test.js` (page) | command | `Command::ReadBench` → `Core::bench` (`mcbench_agent::BenchDetails`, `to_json` is the body) → `Update::Bench`; published unasked after each change of a bench whose details were asked for (`src/core/bench.rs`) |
| 42 | POST `/api/benches/:id/vote` | a vote | `server-turns.test.js` | command | `Command::Vote` → `Core::vote`; ends in `Update::Bench` then `Update::Leaderboard`, a refusal in `Update::Error` then `Update::Bench` |
| 43 | POST `/api/benches/:id/reveal` | reveals | `server-turns.test.js` | command | `Command::Reveal` → `Core::reveal`; ends in `Update::Conversations` then `Update::Bench`, `Update::Error` first on a failure |
| 44 | POST `/api/benches/:id/branch` | follows a round's version | `server-turns.test.js` | command | `Command::BenchBranch` → `Core::bench_branch` → `Turns::bench_branch` (the bench view's frame: renderer); as row 43 |
| 45 | POST `/api/benches/:id/rounds` | a round, or a round's new version | `server-turns.test.js` | command | `Command::Send` with `Target::Bench` (`edit_of`: the round) → `Core::bench_round` → `Turns::bench_round`; as row 33, `round` today's `rounds` |
| 46 | POST `/api/benches/:id/retry` | retries a model | `server-turns.test.js` | command | `Command::RetryMember` → `Core::retry_member` → `Turns::retry_member`; ends in `Update::Conversations`, `Update::Error` first on a refusal |
| 47-49 | GET, POST, DELETE `/api/benches/:id/share` | a bench's share | `share-routes.test.js` | share | `Command::ReadShare`, `Publish`, `Unshare` (`core/share.rs`) |
| 50 | POST `/api/benches/:id/archive` | archives | `server-archive.test.js` | command | `Command::Archive(Subject::Bench)` → `Core::archive_bench` |
| 51 | GET `/api/conversations/:id` | the transcript | `server-turns.test.js` | command | `Command::Open` → `Update::Transcript` or `Update::TranscriptFailed` and nothing else (it does not mark the chat seen); `Core::conversation` (`Transcript::to_json` is the body) |
| 52 | GET `/api/conversations/:id/schematic` | settles at tick 0, `.schem` | `schematic.test.js` | core | `Core::schematic` |
| 53-55 | GET, POST, DELETE `/api/conversations/:id/share` | a chat's share | `share-routes.test.js` | share | `Command::ReadShare`, `Publish`, `Unshare` (`core/share.rs`) |
| 56 | POST `/api/conversations/:id/selection` | names a box | `selection.test.js` | core | `Core::selection` |
| 57 | POST `/api/conversations/:id/seen` | `seen_at` | `server-archive.test.js` | command | `Command::MarkSeen` → `Core::seen` (the only command that marks a chat seen; `Command::Open` does not, as route 51 does not); ends in `Update::Conversations` |
| 58 | POST `/api/conversations/:id/world` | see_build page's world stream | `see-build-world.test.js` | gone | the renderer's (`see_build` behind the seam) |
| 59 | POST `/api/conversations/:id/dismiss-error` | dismisses | `server-turns.test.js` | command | `Command::DismissError` → `Core::dismiss_error` → `Turns::dismiss_error`; ends in the chat's `Update::Transcript` |
| 60 | POST `/api/conversations/:id/archive` | archives | `server-archive.test.js` | command | `Command::Archive(Subject::Chat)` → `Core::archive_conversation` |
| 61 | POST `/api/conversations/:id/fork` | a new chat from a bench round | `server-turns.test.js` | command | `Command::Fork` → `Core::fork`; ends in `Update::Forked`, after the list with the new chat when it worked |
| 62 | POST `/api/conversations/:id/chat` | the prompt, the turn (NDJSON) | `server-turns.test.js`, `resolution.test.js` | command | `Command::Send` (an edit or a retry with `edit_of`) → `Core::chat` → `Turns::send`; a refusal before the turn is `Update::SendRefused`; then `Update::TurnJoined`, the events as `Update::Turn`, the transcript at the start and the end (`src/core/turns.rs`) |
| 63 | GET `/api/conversations/:id/stream` | re-attaches | `turn-stream.test.js` | command | `Command::JoinTurn` → `Core::join_turn`: `Update::TurnJoined`, the snapshot's transcript and the events so far, then the live ones (`src/core/turns.rs`); `Update::TurnNotRunning` |
| 64 | POST `/api/conversations/:id/stop` | aborts the turn | `server-turns.test.js` | command | `Command::Stop` → `Turns::stop` |
| 65 | POST `/api/conversations/:id/branch` | another branch | `server-turns.test.js` | command | `Command::Branch` → `Turns::branch`; ends in the chat's `Update::Transcript` (`Core::branch` for the body) |
| | `route`: the token link, `auth.authorized`, `foreignRequest`, security headers, `blueMap.serve`, `coreWasm.serve`, static files and `/vendor/` | | `page-addresses.test.js`, `attachment-serving.test.js` (3rd), `provider-login.test.js` (bind) | gone | no HTTP: the window and the core are one process |
| | the process: `startHttpServer`, `shutdown`, `exitOnSignals`, `exitOnFatal`, the purge timer | | `server-turns.test.js`, `server-fatal.test.js` | core | `Core::start` (purge, `Turns::start`, setup), `Core::shutdown`, the fatal watcher (`core/threads.rs`) |
| | the bench round views (`benchView`, `BenchComposer`, `liveViews.frame`) | | `bench-world.test.js` | no home | the live view of a bench round waits for the renderer; `mcbench_world::bench` holds the composer |

Counts: command 35 (3, 5, 6, 7, 9 to 16, 18, 20, 21, 28 to 32, 34 to 38, 50, 51, 57, 59, 60, 62 to 65), core 16, gone 8 routes and the HTTP layer, share 10, no home 2 (block icons, bench round views). Key binds and the page's addresses are not routes (the page's own state): the interface's.

## Share (`core/share.rs`; `share-routes.test.js` ported by name in `tests/core_share.rs`)

| Route | What it does | What it needs |
|---|---|---|
| GET `/api/shares` | every share on the site, newest first, with its chat or bench (`archived`) | `settings.shareRepo`; `git` and `gh` on PATH; `Shares.list(repo)` reads the GitHub Pages repository's `builds/` |
| DELETE `/api/shares/:shareId` | removes a share (`[0-9a-f]{12}`) | `git` commit and push to the repo |
| GET `.../share` (chat, bench) | the page's address or null | `shares.url(repo, shareId(id))` |
| POST `.../share` (chat, bench) | publishes, streaming its states as NDJSON (`publishStream`) | a world that is not empty, no turn running; `share-viewer.js` (the viewer page, the world as JSON chunks, a bench's models side by side), `git clone/commit/push` and `gh` for Pages; the X post's text |
| DELETE `.../share` (chat, bench) | removes it | `git` |

## Tests and where they are ported

| Today's file | Cases about routes | Ported |
|---|---|---|
| `server-turns.test.js` (34) | 27 lifecycle cases | `mcbench-agent` `tests/turns.rs` (already) |
| | bench chat only through its bench (464, through the core's methods), fork reveal (477), votes (495) | `tests/core_routes.rs` |
| | a fork whose workspace cannot be copied (653), the failure-injection cases of rounds, retries, branch switches and saves (628, 696, 709, 718, 774, 436, 797), a second server on the folder (818: `mcbench-setup`'s `InstanceLock` tests) | not ported here: they inject store or sandbox failures into `Turns` (the lifecycle's, `mcbench-agent`) |
| `server-archive.test.js` (5) | 4 of the store | `mcbench-store` `tests/archive.rs` (already) |
| | the list's archived counts (247) | `tests/core_routes.rs` |
| `server-fatal.test.js` (1) | 1 | `tests/core_fatal.rs` |
| `resolution.test.js` (9) | 140, 154, 166 | `mcbench-agent` (the lifecycle's `read_resolution`); `tests/core_routes.rs` checks the refusal through the core |
| `provider-login.test.js` (12) | 9 login flows | `mcbench-providers` `tests/login.rs` (already) |
| | `/api/activity` (262) | `tests/core_routes.rs` |
| | `/api/config` hosted (278), MCBENCH_BIND (255) | gone |
| | Settings show no terminal command (250) | `tests/core_routes.rs` |
| `attachment-serving.test.js` (3) | which types show inline (50) | `src/core/routes/files.rs` unit test |
| | 304s and slow clients (89, 101) | gone (HTTP) |
| `page-addresses.test.js` (2) | | gone (HTTP and the page) |
| `share-routes.test.js` (8) | | share: not built |

Settings, sign-in, installs, setup's retry, the votes and sharing: `tests/core_settings.rs` (each command's updates on success and on a failure, a sign-in and an install driven through their states by a fake CLI and a fake download, each change published once in order) and `tests/core_settings_diff.rs` (`SettingsView` against `GET /api/settings`, the file after each save against today's `PUT`, the vote count and `DELETE /api/votes`). The commands themselves: `tests/core_commands.rs` (each command's updates on success and on a failure with a real cause), `tests/core_turn_order.rs` (a join at each position of a turn, a bench round's turn, two chats' turns interleaved) and `tests/core_commands_diff.rs` (27 operations through today's `handleApi` and through `Core::command` on two clones of a fixture: answers, rows and files agree).
