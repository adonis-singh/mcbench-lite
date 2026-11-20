//! The core's start without a window, from an empty data folder:
//!
//!   start_timing <empty data folder> <bundle.tar.zst> <its lock file> <mcbench-lite>
//!
//! Starts the app's core (the app's own backend: setup with the scratch
//! bundle unpacked as the shipped data, the textures fetched from Mojang
//! and baked, the tools made on `<mcbench-lite>` as the engine children and
//! the build child) and prints, in milliseconds from `Core::start`'s call,
//! when the first `Update::Conversations` was published and when every
//! setup step but the shell was done, then stops the core.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mcbench_lite::core::{Core, Parts};
use mcbench_setup::StepState;
use mcbench_ui::{Update, Window};

struct Clock {
    started: Instant,
    list: Mutex<Option<Duration>>,
    steps: Mutex<Option<Duration>>,
    failed: Mutex<Option<String>>,
}

impl Window for Clock {
    fn publish(&self, update: Update) {
        let at = self.started.elapsed();
        match update {
            Update::Conversations(_) => {
                self.list.lock().unwrap().get_or_insert(at);
            }
            Update::Setup(steps) => {
                for step in steps.iter().filter(|s| s.id != "shell") {
                    if let StepState::Failed { error } = &step.state {
                        *self.failed.lock().unwrap() = Some(format!("{}: {error}", step.id));
                    }
                }
                if steps
                    .iter()
                    .filter(|s| s.id != "shell")
                    .all(|s| matches!(s.state, StepState::Done { .. }))
                {
                    self.steps.lock().unwrap().get_or_insert(at);
                }
            }
            _ => {}
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [data, bundle, lock, program] = args.as_slice() else {
        eprintln!(
            "usage: start_timing <empty data folder> <bundle.tar.zst> <its lock file> <mcbench-lite>"
        );
        std::process::exit(2)
    };
    let data = PathBuf::from(data);
    assert!(
        !data.exists() || std::fs::read_dir(&data).unwrap().next().is_none(),
        "the data folder must be empty"
    );
    let bundle = std::fs::read(bundle).unwrap();
    let lock = mcbench_setup::bundle::Lock::parse(&std::fs::read_to_string(lock).unwrap()).unwrap();
    let clock = Arc::new(Clock {
        started: Instant::now(),
        list: Mutex::new(None),
        steps: Mutex::new(None),
        failed: Mutex::new(None),
    });
    let folder = data.clone();
    let core = Core::start(Parts {
        data: data.clone(),
        program: PathBuf::from(program),
        host: Arc::new(mcbench_store::SystemHost),
        shipped: Box::new(move || mcbench_setup::Data::unpack(&bundle, &lock, &folder)),
        client: mcbench_setup::fetch::client(),
        backend: None,
        installer: None,
        share: None,
        live: Default::default(),
    })
    .unwrap();
    mcbench_ui::Core::attach(&core, clock.clone());
    while clock.steps.lock().unwrap().is_none() {
        if let Some(error) = clock.failed.lock().unwrap().clone() {
            eprintln!("setup failed: {error}");
            std::process::exit(1);
        }
        assert!(
            clock.started.elapsed() < Duration::from_secs(600),
            "setup took over 10 minutes"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let ms = |d: Option<Duration>| d.map_or(-1.0, |d| d.as_secs_f64() * 1000.0);
    println!(
        "first Conversations {:.1} ms, setup but the shell done {:.1} ms",
        ms(*clock.list.lock().unwrap()),
        ms(*clock.steps.lock().unwrap())
    );
    core.shutdown();
}
