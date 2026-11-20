//! `mcbench-ui-dev`: the chat screen over a stand-in core and a fixture, for
//! the accessibility and input checks on each platform.
//!
//! ```text
//! mcbench-ui-dev [--fixture gate|long|many | --data FOLDER] [--preferences FILE] [--rate-ms N] [--gallery]
//!                [--open ADDRESS] [--size WxH] [--origin X,Y]
//!                [--a11y-dump PATH] [--log PATH] [--quit-after-ms N]
//!                [--simulate-ms N] [--archive-fails] [--running CHAT]
//!                [--setup-pending [--setup-ms N]] [--search-fails]
//!                [--settings PLAY]... [--settings-step-ms N] [--models N]
//!                [--settings-file PATH] [--unreadable CHAT] [--archive-ms N]
//!                [--lists-ms N]
//! ```
//!
//! `--settings PLAY` (repeated) is what the stand-in's Settings play
//! (`dev::standin::Play`): `claude-sign-in` (Claude Code installs with MB
//! progress, then its sign-in starts, waits with the address, wants the
//! pasted code), `chatgpt-sign-in` (a one-time code), `sign-in-fails`,
//! `sign-in-cancelled`, `install-fails`, `save-refused`, `read-fails`,
//! `shares-empty`, `shares-no-repo`, `shares-failed`, `remove-fails`,
//! `votes-0`, `votes-1`, `votes-unreadable`, `clear-fails`, `setup-play`
//! (every step through its states with download progress), `setup-fails`
//! (a needed step fails until `Try again`), `setup-optional-fails`, `no-ai`,
//! `install-refused`, `setup-wsl`, `setup-wsl-restart`, `setup-wsl-refused`,
//! and the three with setup otherwise ready: `setup-wsl-optional`,
//! `setup-wsl-optional-restart`, `setup-wsl-optional-refused`. The
//! flows move a step every `--settings-step-ms N` (700 by default).
//! `--models N` adds N models to OpenRouter's catalogue (the timings with
//! a 300-model catalogue). `--settings-file PATH` is the settings file the
//! stand-in names in the key rows' help (by default `settings.json` in the
//! scratch folder): screenshots pass a path that names no home folder.
//!
//! `--unreadable CHAT` makes opening that chat fail (`This chat could not be
//! read: …` with `Read again`); `--archive-ms N` answers the archive's reads
//! after N ms, so the archived section shows `Loading…` meanwhile
//! (chat-fix-2).
//!
//! `--setup-pending` starts with setup unfinished (the data done, the
//! textures running: `Sends when the app is ready. 1 of 2 done.` over a
//! held send, with `Don't send`); the running step finishes after
//! `--setup-ms N`, or without it when a line (Enter) arrives on the
//! binary's standard input.
//!
//! `--simulate-ms N` steps the stand-in through the list's states every N
//! ms (a Build chat runs, then finishes unseen; a bench model runs, then
//! fails). `--archive-fails` makes the archive's reads fail;
//! `--search-fails` every search. `--running
//! CHAT` lists that chat's turn as running from the start (the gate
//! fixture lists none running by default). `many` is 500
//! chats of 10 messages, none running (the sidebar's timings).
//!
//! `--open` opens a place given as today's address (`/chat/<id>`, `/new`,
//! `/bench/<id>`, `/settings`; `state::address`), the form deep links will
//! take.
//!
//! `--data <folder>` serves a folder `dev/export-data.py` wrote (a copy of
//! someone's chats: keep it in scratch) instead of a fixture; its window
//! preferences go to the folder's `preferences.json`. With a fixture they
//! are kept only in the file `--preferences` names (so a run that names
//! one reopens where the last left; the checks start fresh). `--rate-ms` sets the streamed answer's pace (60 ms a
//! word; 0 streams it at once).
//!
//! `--gallery` shows every shared control in each state instead of the chat
//! screen (`mcbench_ui::dev::gallery`).
//!
//! `--a11y-dump` writes the window's AccessKit tree as JSON whenever it
//! changes (while an assistive client is connected); `--log` writes one JSON
//! line per user-visible action; a link is logged and never opened.
//! `--scratch DIR` (required) holds the fixture's picture and the crash
//! logs; nothing goes to the system's temporary folder.
//!
//! The chat's checks (`verify/linux/check_chat.py`): `--turn full` streams
//! every item kind in each turn (`dev::turns`); `--refuse CHAT` refuses each
//! send to `CHAT` (`new` for a new chat); `--join CHAT` streams a turn when
//! `CHAT`'s running turn is joined; `--pick PATH` (repeated) is what `Attach
//! file` picks instead of the platform's dialog; `--select` starts the new
//! chat's draft with a selection, as the viewer will make one; `--answer
//! FILE` is the text each turn streams; (par-chat) `--model-facts
//! unknown|no-tools` makes the stand-in's chat model one no source describes
//! or one that cannot call tools (the header's warnings); `--providers-fail
//! TEXT` fails the first read of the providers (`Models failed to load.`); (bench agent) `--fixture bench` adds
//! an eight-model castle bench and a revealed one, the library and votes;
//! `--bench <play>` plays a bench or library failure (`standin::BenchPlay`),
//! `--prompts N` and `--prompt-chars N` add N prompts of that length; `--fixture long-pictures` is `long`
//! with twenty pictures of their own files. Sent files and shots are kept
//! under the scratch folder's `files/`.
//!
//! (share agent) `--share <play>` (repeated) plays a share failure or state
//! (`standin::SharePlay`: fails-exporting, fails-publishing, fails-waiting,
//! held, empty, read-fails, unshare-fails, not-joined); a publish moves a
//! step every `--share-step-ms N` (900 by default).
//!
//! `--active-pacing` is a measurement flag: the window paces its frames as
//! an active window does even while it is not the active one (GPUI's
//! `inactive_frame_interval` none instead of about 30 frames a second). The
//! rig's window on the Mac's virtual display is never the active one, so
//! its timings of sparse input take this flag; the app has no such flag.
//!
//! `mcbench-ui-dev --redraw IN OUT` writes the file IN as a send prepares it
//! (a picture redrawn as JPEG) to OUT and exits, with no window
//! (`verify/linux/canvas/compare.py`).

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use mcbench_ui::dev::{fixtures, standin};
use mcbench_ui::{ActionLog, Options};

fn pair(value: &str, separator: char, flag: &str) -> (f32, f32) {
    let (a, b) = value.split_once(separator).unwrap_or_else(|| {
        panic!("{flag} takes two numbers separated by '{separator}', not {value}")
    });
    let number = |text: &str| -> f32 {
        text.parse()
            .unwrap_or_else(|_| panic!("{flag}: {text} is not a number"))
    };
    (number(a), number(b))
}

/// The folder the caller names for the dev binary's picture and crash logs
/// (`--scratch DIR`, the first argument pair): never the system's temp
/// folder.
fn scratch_folder() -> PathBuf {
    let args: Vec<String> = std::env::args().collect();
    let at = args
        .iter()
        .position(|a| a == "--scratch")
        .unwrap_or_else(|| {
            panic!("mcbench-ui-dev needs --scratch DIR: the folder for its picture and crash logs")
        });
    let dir = PathBuf::from(
        args.get(at + 1)
            .unwrap_or_else(|| panic!("--scratch needs a folder")),
    );
    std::fs::create_dir_all(&dir)
        .unwrap_or_else(|e| panic!("cannot create {}: {e}", dir.display()));
    dir
}

fn main() {
    // `--redraw IN OUT`: an attachment as a send prepares it
    // (`state::upload::read_path`), written out; for comparing with a
    // browser's canvas. No window.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--redraw") {
        let (input, output) = (&args[2], &args[3]);
        let file = mcbench_ui::state::upload::read_path(std::path::Path::new(input))
            .unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(output, &file.data).unwrap_or_else(|e| panic!("cannot write {output}: {e}"));
        return;
    }
    let started = std::time::Instant::now();
    let scratch = scratch_folder();
    mcbench_ui::crash::install(&scratch.join("logs"), env!("CARGO_PKG_VERSION"), |_| false);
    // par-shell: a panic starts this program again into the error screen;
    // this process may be that screen's (it shows it once the arguments are
    // read, before any of them is acted on). Only a run that asks for a
    // crash (`--crash`) opts in: any other run of the dev binary that
    // panics writes its log and exits 101, so a check or a timing run never
    // leaves a second process and never opens a window nobody asked for.
    let error_screen = if std::env::args().any(|a| a == "--crash") {
        mcbench_ui::crash::relaunch_on_panic(
            std::env::current_exe().unwrap_or_else(|e| panic!("this program's path: {e}")),
            std::env::args_os().skip(1).collect(),
        )
    } else {
        None
    };
    let mut options = Options::app();
    options.started = started;
    let mut fixture = "gate".to_string();
    // bench agent
    let mut bench_plays: Vec<standin::BenchPlay> = Vec::new();
    let mut many_prompts = 0usize;
    let mut prompt_chars = 200usize;
    let mut fatal: Option<String> = None;
    let mut gallery = false;
    let mut data: Option<PathBuf> = None;
    let mut preferences: Option<PathBuf> = None;
    let mut rate = standin::DEV_RATE;
    let mut crash: Option<String> = None;
    let mut script = mcbench_ui::dev::turns::Script::Plain;
    let mut refuse: Vec<String> = Vec::new();
    // par-chat: the facts the stand-in's own model answers with.
    let mut model_facts: Option<String> = None;
    let mut providers_fail: Option<String> = None;
    let mut join: Vec<String> = Vec::new();
    let mut picks: Vec<PathBuf> = Vec::new();
    let mut answer: Option<PathBuf> = None;
    let mut crash_after = Duration::from_millis(2000);
    // share agent: the share plays and the publish's clock.
    let mut share_plays: Vec<standin::SharePlay> = Vec::new();
    let mut share_step = Duration::from_millis(900);
    // sidebar agent: the list's states on a clock, and a failing archive.
    let mut simulate: Option<Duration> = None;
    let mut archive_fails = false;
    let mut running: Vec<String> = Vec::new();
    let mut setup_pending = false;
    let mut search_fails = false;
    let mut setup_after: Option<Duration> = None;
    // settings agent
    let mut plays: Vec<standin::Play> = Vec::new();
    let mut settings_step = Duration::from_millis(700);
    let mut extra_models: usize = 0;
    let mut settings_file: Option<PathBuf> = None;
    // chat-fix-2: rows nothing else shows.
    let mut unreadable: Vec<String> = Vec::new();
    let mut archive_delay: Option<Duration> = None;
    // start-place agent: the first lists N ms late (the real core's cold start).
    let mut lists_delay: Option<Duration> = None;
    // pane agent
    let mut pane_flags = mcbench_ui::dev::pane::DevFlags::default();
    // The real renderer with one world (feature `real-pane`): the engine's
    // blueprint bytes, their y offset and the renderer's data folder.
    let mut blueprint: Option<PathBuf> = None;
    let mut y_offset: i32 = 64;
    let mut draw_data: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| panic!("{arg} needs a value"));
        match arg.as_str() {
            "--fixture" => fixture = value(),
            "--size" => {
                options.size = pair(&value(), 'x', "--size");
                options.size_given = true;
            }
            "--data" => data = Some(PathBuf::from(value())),
            "--preferences" => preferences = Some(PathBuf::from(value())),
            "--rate-ms" => {
                rate = Duration::from_millis(
                    value()
                        .parse()
                        .unwrap_or_else(|_| panic!("--rate-ms takes milliseconds")),
                )
            }
            "--origin" => options.origin = Some(pair(&value(), ',', "--origin")),
            "--a11y-dump" => options.a11y_dump = Some(PathBuf::from(value())),
            "--log" => options.log = Some(ActionLog::create(&PathBuf::from(value()))),
            "--quit-after-ms" => {
                let ms: u64 = value()
                    .parse()
                    .unwrap_or_else(|_| panic!("--quit-after-ms takes milliseconds"));
                options.quit_after = Some(Duration::from_millis(ms));
            }
            "--scratch" => {
                value();
            }
            "--fatal" => fatal = Some(value()),
            "--gallery" => gallery = true,
            "--open" => {
                let address = value();
                options.open = Some(
                    mcbench_ui::state::address::address_place(&address).unwrap_or_else(|| {
                        panic!("--open takes an address such as /chat/<id>, /new, /bench/<id>, not {address}")
                    }),
                );
            }
            "--simulate-ms" => {
                simulate =
                    Some(Duration::from_millis(value().parse().unwrap_or_else(
                        |_| panic!("--simulate-ms takes milliseconds"),
                    )))
            }
            "--archive-fails" => archive_fails = true,
            "--search-fails" => search_fails = true,
            "--running" => running.push(value()),
            "--setup-pending" => setup_pending = true,
            "--setup-ms" => {
                setup_after = Some(Duration::from_millis(
                    value()
                        .parse()
                        .unwrap_or_else(|_| panic!("--setup-ms takes milliseconds")),
                ))
            }
            "--crash" => crash = Some(value()),
            "--settings" => plays.push(standin::Play::named(&value())),
            "--settings-step-ms" => {
                settings_step = Duration::from_millis(
                    value()
                        .parse()
                        .unwrap_or_else(|_| panic!("--settings-step-ms takes milliseconds")),
                )
            }
            "--settings-file" => settings_file = Some(PathBuf::from(value())),
            // share agent
            "--share" => share_plays.push(standin::SharePlay::named(&value())),
            "--share-step-ms" => {
                share_step = Duration::from_millis(
                    value()
                        .parse()
                        .unwrap_or_else(|_| panic!("--share-step-ms takes milliseconds")),
                )
            }
            // bench agent: bench and library plays, and many prompts.
            "--bench" => bench_plays.push(standin::BenchPlay::named(&value())),
            "--prompts" => {
                many_prompts = value()
                    .parse()
                    .unwrap_or_else(|_| panic!("--prompts takes a count"))
            }
            "--prompt-chars" => {
                prompt_chars = value()
                    .parse()
                    .unwrap_or_else(|_| panic!("--prompt-chars takes a count"))
            }
            "--models" => {
                extra_models = value()
                    .parse()
                    .unwrap_or_else(|_| panic!("--models takes a count"))
            }
            "--unreadable" => unreadable.push(value()),
            "--lists-ms" => {
                lists_delay = Some(Duration::from_millis(
                    value()
                        .parse()
                        .unwrap_or_else(|_| panic!("--lists-ms takes milliseconds")),
                ))
            }
            "--archive-ms" => {
                archive_delay =
                    Some(Duration::from_millis(value().parse().unwrap_or_else(
                        |_| panic!("--archive-ms takes milliseconds"),
                    )))
            }
            "--turn" => {
                script = match value().as_str() {
                    "full" => mcbench_ui::dev::turns::Script::Full,
                    "plain" => mcbench_ui::dev::turns::Script::Plain,
                    other => panic!("--turn takes full or plain, not {other}"),
                }
            }
            "--refuse" => refuse.push(value()),
            // par-chat: `--model-facts unknown|no-tools`.
            "--model-facts" => model_facts = Some(value()),
            // par-chat: `--providers-fail TEXT`, the first providers' read.
            "--providers-fail" => providers_fail = Some(value()),
            "--join" => join.push(value()),
            "--pick" => picks.push(PathBuf::from(value())),
            "--answer" => answer = Some(PathBuf::from(value())),
            "--select" => options.selections.push(mcbench_ui::Selection {
                letter: "A".into(),
                label: "Redstone Lamp, unlit".into(),
                min: [1, 1, 1],
                max: [1, 2, 1],
                blocks: 2,
                block: Some("minecraft:redstone_lamp".into()),
            }),
            // mac-real: a measurement flag (see the header).
            "--active-pacing" => options.inactive_frame_interval = None,
            "--crash-after-ms" => {
                crash_after = Duration::from_millis(
                    value()
                        .parse()
                        .unwrap_or_else(|_| panic!("--crash-after-ms takes milliseconds")),
                )
            }
            // pane agent: the fake 3D pane's states and failures, and where
            // exports go with no dialog.
            "--pane" => pane_flags.add(&value()),
            "--blueprint" => blueprint = Some(PathBuf::from(value())),
            "--y-offset" => {
                y_offset = value()
                    .parse()
                    .unwrap_or_else(|_| panic!("--y-offset takes a whole number"))
            }
            "--draw-data" => draw_data = Some(PathBuf::from(value())),
            "--save-to" => {
                options.saver = mcbench_ui::view::world::fixed_saver(PathBuf::from(value()))
            }
            other => panic!("unknown argument {other}"),
        }
    }
    if let Some(screen) = error_screen {
        screen.apply(&mut options);
        // `--crash screen`: the error screen's own process panics too.
        if crash.as_deref() == Some("screen") {
            mcbench_ui::dev::PANIC_WHILE_DRAWING.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        mcbench_ui::fatal(screen.message, options);
    }
    if let Some(how) = crash {
        crash_later(how, crash_after);
    }
    if gallery {
        mcbench_ui::run_gallery(options);
    }
    if let Some(message) = fatal {
        mcbench_ui::fatal(message, options);
    }
    // A link only goes to the log.
    options.opener = Rc::new(|_, _| {});
    std::fs::create_dir_all(&scratch)
        .unwrap_or_else(|e| panic!("cannot create {}: {e}", scratch.display()));
    let picture = scratch.join("lamp.png");
    std::fs::write(&picture, fixtures::PICTURE)
        .unwrap_or_else(|e| panic!("cannot write {}: {e}", picture.display()));
    let mut texts = fixtures::Texts::new(&picture);
    if let Some(path) = &answer {
        texts.streamed = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("cannot read --answer {}: {e}", path.display()));
    }
    // Sent files and shots go to the exported folder's copy, or the scratch.
    let files = data.clone().unwrap_or_else(|| scratch.join("files"));
    let from_folder = data.is_some();
    let core = match data {
        Some(folder) => standin::StandIn::from_folder(&folder, texts.streamed.clone(), rate),
        None => {
            let chats = match fixture.as_str() {
                "gate" => fixtures::gate(&texts),
                "long" => fixtures::long(),
                "long-pictures" => {
                    // Twenty pictures of their own files, for the memory
                    // after scrolling.
                    let pictures: Vec<PathBuf> = (0..20)
                        .map(|n| {
                            let path = scratch.join(format!("picture-{n}.png"));
                            std::fs::write(&path, fixtures::PICTURE)
                                .unwrap_or_else(|e| panic!("cannot write {}: {e}", path.display()));
                            path
                        })
                        .collect();
                    fixtures::long_with_pictures(&pictures)
                }
                "many" => fixtures::many(),
                // bench agent
                "bench" => fixtures::bench(&texts),
                other => panic!("no fixture {other}: gate, long, long-pictures, many or bench"),
            };
            standin::StandIn::new(chats, texts.streamed.clone(), rate, preferences)
        }
    };
    core.set_script(script);
    // bench agent: the bench fixture's library and votes, the plays.
    if fixture == "bench" && !from_folder {
        core.bench_fixture();
    }
    if many_prompts > 0 {
        core.many_prompts(many_prompts, prompt_chars);
    }
    for play in bench_plays {
        core.bench_play(play);
    }
    // settings agent: the settings file under the scratch folder, the plays,
    // a bigger catalogue.
    core.set_settings_file(settings_file.unwrap_or_else(|| scratch.join("settings.json")));
    for play in &plays {
        core.settings_play(*play);
    }
    if extra_models > 0 {
        core.add_synthetic_models("openrouter", extra_models);
    }
    core.keep_files(&files, &picture);
    if let Some(text) = &providers_fail {
        core.fail_providers_read(text);
    }
    // par-chat: the header's model warnings.
    if let Some(kind) = &model_facts {
        let facts = match kind.as_str() {
            "unknown" => mcbench_ui::ModelFacts::unknown(),
            "no-tools" => mcbench_ui::ModelFacts {
                known: true,
                tools: Some(false),
                ..mcbench_ui::ModelFacts::unknown()
            },
            other => panic!("--model-facts takes unknown or no-tools, not {other}"),
        };
        core.describe_model("anthropic", standin::MODEL, facts);
    }
    for chat in &refuse {
        core.refuse_sends_to(chat);
    }
    for chat in &join {
        core.run_turn_on_join(chat);
    }
    if !picks.is_empty() {
        options.picker = mcbench_ui::view::fixed_picker(picks);
    }
    let core = Arc::new(core);
    // pane agent: the fake 3D pane and the live world's clock.
    let _pane = pane_flags.start(&core);
    if let Some(blueprint) = blueprint {
        real_pane(
            &core,
            &blueprint,
            y_offset,
            &draw_data.expect(
                "--blueprint goes with --draw-data <a data folder with minecraft/ and textures/>",
            ),
        );
    }
    core.fail_archive_reads(archive_fails);
    if search_fails {
        core.fail_searches(Some(standin::SEARCH_FAILURE.to_string()));
    }
    for chat in &running {
        core.mark_running(chat);
    }
    for chat in &unreadable {
        core.fail_reads_of(chat);
    }
    if let Some(delay) = archive_delay {
        core.delay_archive_reads(delay);
    }
    if let Some(delay) = lists_delay {
        core.delay_first_lists(delay);
    }
    if let Some(every) = simulate {
        core.simulate(every);
    }
    // share agent: the plays, and a publish moving on its clock.
    for play in &share_plays {
        core.share_play(*play);
    }
    {
        let core = core.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(share_step);
                core.share_tick();
            }
        });
    }
    // settings agent: the sign-ins, installs and setup plays move on a
    // clock.
    {
        let core = core.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(settings_step);
                core.settings_tick();
            }
        });
    }
    if setup_after.is_some() && !setup_pending {
        panic!("--setup-ms goes with --setup-pending");
    }
    if setup_pending {
        core.setup_pending();
        let core = core.clone();
        std::thread::spawn(move || {
            match setup_after {
                Some(after) => std::thread::sleep(after),
                None => {
                    let mut line = String::new();
                    // End of input (no terminal) finishes nothing.
                    let read = std::io::stdin()
                        .read_line(&mut line)
                        .unwrap_or_else(|e| panic!("cannot read standard input: {e}"));
                    if read == 0 {
                        return;
                    }
                }
            }
            core.finish_setup();
        });
    }
    mcbench_ui::run(core, options)
}

/// `--crash panic|abort|fastfail|segv|objc`: the process crashes on purpose
/// after `after`, on a thread of its own (panic, abort, fastfail, segv) or on
/// the main thread through the event loop (objc, macOS only: an uncaught
/// NSException), for the crash checks in the VMs. `abort` is the C library's
/// `abort()`, as C code aborts; `fastfail` is Rust's `std::process::abort`,
/// which on Windows is `__fastfail` and leaves no record (the limit, kept
/// visible). The others end in the crash log and a quiet exit; a panic
/// (`panic` on its own thread, `draw` on the window's thread at its next
/// draw, `screen` as `draw` and then in the error screen's process too)
/// then starts this program again into the error screen (par-shell).
fn crash_later(how: String, after: Duration) {
    match how.as_str() {
        "panic" | "abort" | "fastfail" | "segv" | "draw" | "screen" => {}
        #[cfg(target_os = "macos")]
        "objc" => {}
        other => {
            panic!(
                "--crash takes panic, draw, screen, abort, fastfail, segv or (on macOS) objc, not {other}"
            )
        }
    }
    std::thread::spawn(move || {
        std::thread::sleep(after);
        match how.as_str() {
            "panic" => panic!("a panic on purpose (--crash panic)"),
            // par-shell: on the window's own thread, at its next draw (and,
            // for `screen`, in the error screen's process too).
            "draw" | "screen" => {
                mcbench_ui::dev::PANIC_WHILE_DRAWING
                    .store(true, std::sync::atomic::Ordering::SeqCst);
            }
            "abort" => c_abort(),
            "fastfail" => std::process::abort(),
            "segv" => segv(),
            #[cfg(target_os = "macos")]
            "objc" => objc_exception(),
            _ => unreachable!("checked above"),
        }
    });
}

/// The C library's `abort()`.
fn c_abort() -> ! {
    unsafe extern "C" {
        fn abort() -> !;
    }
    unsafe { abort() }
}

/// A segmentation fault (an access violation on Windows): a write through
/// a null pointer, which the compiler cannot remove.
fn segv() {
    unsafe { std::ptr::write_volatile(std::ptr::null_mut::<u8>(), 1) }
}

/// Raises an uncaught NSException (`[NSException raise:format:]` through
/// `objc_exception_throw`), on this thread.
#[cfg(target_os = "macos")]
fn objc_exception() {
    unsafe extern "C" {
        fn objc_getClass(name: *const std::ffi::c_char) -> *mut std::ffi::c_void;
        fn sel_registerName(name: *const std::ffi::c_char) -> *mut std::ffi::c_void;
        fn objc_msgSend();
    }
    unsafe {
        let class = objc_getClass(c"NSException".as_ptr());
        let raise = sel_registerName(c"raise:format:".as_ptr());
        let string_class = objc_getClass(c"NSString".as_ptr());
        let from = sel_registerName(c"stringWithUTF8String:".as_ptr());
        let send_str: unsafe extern "C" fn(
            *mut std::ffi::c_void,
            *mut std::ffi::c_void,
            *const std::ffi::c_char,
        ) -> *mut std::ffi::c_void = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let name = send_str(string_class, from, c"McbenchCrash".as_ptr());
        let reason = send_str(
            string_class,
            from,
            c"an uncaught exception on purpose (--crash objc)".as_ptr(),
        );
        let send_raise: unsafe extern "C" fn(
            *mut std::ffi::c_void,
            *mut std::ffi::c_void,
            *mut std::ffi::c_void,
            *mut std::ffi::c_void,
        ) = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        send_raise(class, raise, name, reason);
    }
}

/// pane agent: the real renderer (`mcbench-draw`) drawing one world, the
/// engine's blueprint bytes, for every chat's view (`--blueprint`).
#[cfg(feature = "real-pane")]
fn real_pane(
    core: &Arc<standin::StandIn>,
    blueprint: &std::path::Path,
    y_offset: i32,
    data: &std::path::Path,
) {
    use mcbench_render::{AdapterChoice, Renderer, WorldUpload};
    // The data folder's one shipped data and one bake, as the app reads
    // them (`minecraft/<id>`, `textures/<id>`).
    let only = |name: &str| {
        let folder = data.join(name);
        let entries: Vec<PathBuf> = std::fs::read_dir(&folder)
            .unwrap_or_else(|e| panic!("{}: {e}", folder.display()))
            .map(|entry| entry.expect("a folder entry").path())
            .collect();
        match entries.as_slice() {
            [one] => one.clone(),
            _ => panic!(
                "{} holds {} entries, not one",
                folder.display(),
                entries.len()
            ),
        }
    };
    let assets = mcbench_setup::start::assets(
        &mcbench_setup::Data::at(&only("minecraft")),
        &mcbench_setup::Textures::at(&only("textures")),
    );
    let draw = Arc::new(
        mcbench_draw::live::Draw::start(
            &assets,
            AdapterChoice::Hardware,
            mcbench_draw::store::DEFAULT_BYTES,
        )
        .unwrap_or_else(|e| panic!("the renderer: {e:?}")),
    );
    let bytes = std::fs::read(blueprint).unwrap_or_else(|e| panic!("{}: {e}", blueprint.display()));
    let report = draw
        .world(WorldUpload {
            name: "live:blueprint".into(),
            y_offset,
            blueprint: bytes,
            entities: None,
            base: None,
        })
        .unwrap_or_else(|e| panic!("the world: {e:?}"));
    eprintln!(
        "[renderer] {}: {} blocks, {} quads, bounds {:?} to {:?}",
        draw.report().adapter.name,
        report.blocks,
        report.quads,
        report.bounds.min,
        report.bounds.max
    );
    let stop = draw.clone();
    core.set_real(
        draw,
        "live:blueprint".into(),
        report.bounds,
        Box::new(move || stop.shutdown()),
    );
}

#[cfg(not(feature = "real-pane"))]
fn real_pane(_: &Arc<standin::StandIn>, _: &std::path::Path, _: i32, _: &std::path::Path) {
    panic!("--blueprint needs the dev binary built with --features real-pane");
}
