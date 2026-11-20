//! `test/setup.test.js` (7 tests) and the rules of `test/setup-status.test.js`
//! the view holds (the count of the steps the app needs; an optional step
//! that cannot be done is no failure). The page's wording of a step
//! (`web/setup-status.mjs`: megabytes, rows, links) is the interface's.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use common::{scratch, serve, sha256, tiny_data};
use mcbench_setup::steps::native_steps;
use mcbench_setup::{Bytes, Data, Shell, Step, StepError, StepState, Steps, start};
use tokio_util::sync::CancellationToken;

fn steps() -> Arc<Steps> {
    Arc::new(Steps::new(native_steps(), Box::new(|_| {})))
}

fn step(steps: &Steps, id: &str) -> Step {
    steps.view().1.into_iter().find(|s| s.id == id).unwrap()
}

#[test]
fn every_step_waits_until_its_work_starts_and_only_the_shell_is_optional() {
    let (ready, list) = steps().view();
    assert!(!ready);
    assert_eq!(
        list.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        ["data", "textures", "shell"]
    );
    assert!(
        list.iter()
            .all(|s| s.state == StepState::Waiting && !s.title.is_empty() && !s.doing.is_empty())
    );
    assert_eq!(
        list.iter()
            .filter(|s| s.optional)
            .map(|s| s.id.as_str())
            .collect::<Vec<_>>(),
        ["shell"]
    );
}

#[tokio::test]
async fn a_step_runs_with_its_progress_then_is_done_with_how_long_it_took() {
    let steps = steps();
    let (release, released) = tokio::sync::oneshot::channel::<&str>();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let tracked = {
        let steps = steps.clone();
        tokio::spawn(async move {
            steps
                .track("textures", async {
                    Ok::<_, StepError>(released.await.unwrap())
                })
                .await
        })
    };
    while !matches!(step(&steps, "textures").state, StepState::Running { .. }) {
        tokio::task::yield_now().await;
    }
    steps
        .progress(
            "textures",
            Bytes {
                done: 10,
                total: Some(100),
            },
        )
        .unwrap();
    steps.detail("textures", "Downloading").unwrap();
    seen.lock().unwrap().push(step(&steps, "textures"));
    assert_eq!(
        seen.lock().unwrap()[0],
        Step {
            id: "textures".into(),
            title: "Minecraft's textures".into(),
            doing: "Downloading Minecraft's textures from Mojang".into(),
            optional: false,
            state: StepState::Running {
                detail: Some("Downloading".into()),
                progress: Some(Bytes {
                    done: 10,
                    total: Some(100)
                })
            },
            needs: None,
        }
    );
    release.send("the result").unwrap();
    assert_eq!(tracked.await.unwrap().unwrap(), "the result");
    assert!(
        matches!(step(&steps, "textures").state, StepState::Done { seconds } if seconds >= 0.0)
    );
}

#[tokio::test]
async fn a_failed_step_says_why_and_runs_again_when_it_is_next_needed_a_done_step_stays_done() {
    let steps = steps();
    let error = steps
        .track("textures", async {
            Err::<(), _>(StepError::Failed(
                "  the download failed\nerror: no network\n".into(),
            ))
        })
        .await
        .unwrap_err();
    assert!(error.contains("the download failed"));
    assert_eq!(
        step(&steps, "textures").state,
        StepState::Failed {
            error: "the download failed\nerror: no network".into()
        }
    );
    steps
        .track("textures", async { Ok::<_, StepError>(()) })
        .await
        .unwrap();
    let StepState::Done { seconds } = step(&steps, "textures").state else {
        panic!("done")
    };
    assert_eq!(
        steps
            .track("textures", async {
                Err::<(), _>(StepError::Failed("later".into()))
            })
            .await
            .unwrap_err(),
        "later"
    );
    assert_eq!(step(&steps, "textures").state, StepState::Done { seconds });
}

#[tokio::test]
async fn a_failure_about_the_machine_makes_a_step_unavailable_and_the_optional_shell_does_not_keep_the_app_from_ready()
 {
    let steps = steps();
    for id in ["data", "textures"] {
        steps
            .track(id, async { Ok::<_, StepError>(()) })
            .await
            .unwrap();
    }
    steps
        .track("shell", async {
            Err::<(), _>(StepError::Unavailable("WSL is not installed".into()))
        })
        .await
        .unwrap_err();
    assert_eq!(
        step(&steps, "shell").state,
        StepState::Unavailable {
            error: "WSL is not installed".into()
        }
    );
    assert!(steps.view().0);
}

#[tokio::test]
async fn ready_waits_for_every_step_that_is_not_optional() {
    let steps = steps();
    steps
        .track("data", async { Ok::<_, StepError>(()) })
        .await
        .unwrap();
    assert!(!steps.view().0);
    steps
        .track("textures", async { Ok::<_, StepError>(()) })
        .await
        .unwrap();
    assert!(steps.view().0);
}

#[tokio::test]
async fn an_unknown_step_is_a_mistake_in_the_code() {
    assert_eq!(
        steps()
            .track("nothing", async { Ok::<_, StepError>(()) })
            .await
            .unwrap_err(),
        "No setup step nothing"
    );
}

// setup-status.test.js: the count is of the steps the app needs; an optional
// step that cannot be done is no failure.
#[tokio::test]
async fn the_count_is_of_the_steps_the_app_needs_and_an_optional_step_that_cannot_be_done_is_no_failure()
 {
    let steps = steps();
    steps
        .track("data", async { Ok::<_, StepError>(()) })
        .await
        .unwrap();
    steps
        .track("shell", async {
            Err::<(), _>(StepError::Unavailable("WSL is not installed".into()))
        })
        .await
        .unwrap_err();
    let (ready, list) = steps.view();
    let required: Vec<&Step> = list.iter().filter(|s| !s.optional).collect();
    let done = required
        .iter()
        .filter(|s| matches!(s.state, StepState::Done { .. }))
        .count();
    let failed = list
        .iter()
        .any(|s| matches!(s.state, StepState::Failed { .. }));
    assert_eq!((ready, done, required.len(), failed), (false, 1, 2, false));
    assert!(steps.retry().is_err());
    steps
        .track("textures", async {
            Err::<(), _>(StepError::Failed("fetch failed".into()))
        })
        .await
        .unwrap_err();
    assert!(
        steps
            .view()
            .1
            .iter()
            .any(|s| matches!(s.state, StepState::Failed { .. }))
    );
}

// A failed step tried again (the retry): start runs again, so the failed
// download runs, and the step that waited on it after it. The downloads
// are from a server on loopback.
#[tokio::test]
async fn retry_runs_the_failed_step_again_and_the_step_that_waited_on_it_with_nothing_failed_it_is_refused()
 {
    let folder = scratch("retry");
    let server = serve(HashMap::new()).await;
    let jar = tiny_data(
        &folder.join("shipped"),
        &format!("{}/client.jar", server.url),
    );
    let rootfs = b"the root filesystem".to_vec();
    let rootfs_file = folder.join("given-rootfs.tar.zst");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(&rootfs_file, &rootfs).unwrap();
    // The jar is not served yet: the textures fail.
    let steps = steps();
    let prepared = Arc::new(Mutex::new(Vec::new()));
    let shell = |prepared: Arc<Mutex<Vec<(std::path::PathBuf, String)>>>| -> Shell {
        let (path, sha) = (rootfs_file.clone(), sha256(&rootfs));
        Box::pin(async move {
            prepared.lock().unwrap().push((path, sha));
            Ok(())
        })
    };
    let data = |folder: &std::path::Path| -> Box<dyn FnOnce() -> Result<Data, String> + Send> {
        let shipped = folder.join("shipped");
        Box::new(move || Ok(Data::at(&shipped)))
    };
    let first = start(
        steps.clone(),
        folder.path_buf(),
        data(&folder),
        common::client(),
        shell(prepared.clone()),
        CancellationToken::new(),
    )
    .await;
    assert!(first.err().unwrap().contains("HTTP 404"));
    assert!(matches!(step(&steps, "data").state, StepState::Done { .. }));
    assert!(
        matches!(&step(&steps, "textures").state, StepState::Failed { error } if error.contains("HTTP 404"))
    );
    steps.retry().unwrap();
    // The shell went on in the background.
    let waited = std::time::Instant::now();
    while !matches!(step(&steps, "shell").state, StepState::Done { .. }) {
        assert!(
            waited.elapsed().as_secs() < 30,
            "the shell step is {:?}",
            step(&steps, "shell").state
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    server
        .state
        .lock()
        .unwrap()
        .files
        .insert("/client.jar".into(), jar);
    let ready = start(
        steps.clone(),
        folder.path_buf(),
        data(&folder),
        common::client(),
        shell(prepared.clone()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(steps.view().0);
    assert!(
        ready.assets.textures.is_file()
            && ready.assets.atlas.is_file()
            && ready.assets.client_jar.is_file()
    );
    assert_eq!(
        std::fs::read_to_string(&ready.assets.atlas_map).unwrap(),
        r#"{"block/test":[0,0,2,2]}"#,
        "the atlas holds the fetched texture at its own box"
    );
    assert_eq!(
        ready.assets.pack,
        folder.join("shipped").join("mcbench-pack.bin")
    );
    assert!(ready.shell.is_none(), "a done shell is not started again");
    let prepared = prepared.lock().unwrap().clone();
    assert_eq!(prepared.len(), 1);
    assert_eq!(std::fs::read(&prepared[0].0).unwrap(), rootfs);
    assert_eq!(prepared[0].1, sha256(&rootfs));
    assert_eq!(steps.retry().unwrap_err(), "No setup step failed");
    // A later start finds everything in place: nothing is downloaded.
    let asked = server.state.lock().unwrap().requests.len();
    let again = Arc::new(Steps::new(native_steps(), Box::new(|_| {})));
    start(
        again.clone(),
        folder.path_buf(),
        data(&folder),
        common::client(),
        shell(Arc::new(Mutex::new(Vec::new()))),
        CancellationToken::new(),
    )
    .await
    .unwrap()
    .shell
    .unwrap()
    .await
    .unwrap()
    .unwrap();
    assert_eq!(server.state.lock().unwrap().requests.len(), asked);
    assert!(
        again
            .view()
            .1
            .iter()
            .all(|s| matches!(s.state, StepState::Done { .. }))
    );
}

// No root filesystem: the shell step is unavailable with its work's
// sentence, and the other steps run as before.
#[tokio::test]
async fn a_shell_not_given_is_unavailable_with_the_callers_sentence() {
    let folder = scratch("no-shell");
    let steps = steps();
    let failed = start(
        steps.clone(),
        folder.path_buf(),
        Box::new(|| Err("no data here".to_string())),
        common::client(),
        Box::pin(async {
            Err(mcbench_setup::StepError::Unavailable(
                "No root filesystem is named".to_string(),
            ))
        }),
        CancellationToken::new(),
    )
    .await;
    assert_eq!(failed.err().unwrap(), "no data here");
    let waited = std::time::Instant::now();
    loop {
        match step(&steps, "shell").state {
            StepState::Unavailable { error } => {
                assert_eq!(error, "No root filesystem is named");
                break;
            }
            other => assert!(
                waited.elapsed().as_secs() < 30,
                "the shell step is {other:?}"
            ),
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let _ = std::fs::remove_dir_all(&folder);
}

// ui-seam-wsl-install.md: a step that needs the person names it in `needs`
// with its state, both in one change; running and done clear it.
#[tokio::test]
async fn a_step_that_needs_the_person_says_so_with_its_state_and_running_clears_it() {
    use mcbench_setup::Needs::{InstallWsl, Restart};
    type Seen = Vec<(StepState, Option<mcbench_setup::Needs>)>;
    let seen: Arc<Mutex<Seen>> = Arc::default();
    let record = Arc::clone(&seen);
    let steps = Steps::new(
        native_steps(),
        Box::new(move |list: &[Step]| {
            let shell = list.iter().find(|s| s.id == "shell").unwrap();
            record
                .lock()
                .unwrap()
                .push((shell.state.clone(), shell.needs));
        }),
    );
    let taken = || std::mem::take(&mut *seen.lock().unwrap());
    let needs = |needs, failed| {
        Err::<(), _>(StepError::Needs {
            needs,
            failed,
            sentence: "the step's sentence".into(),
        })
    };
    let unavailable = StepState::Unavailable {
        error: "the step's sentence".into(),
    };
    let failed = StepState::Failed {
        error: "the step's sentence".into(),
    };
    let running = StepState::Running {
        detail: None,
        progress: None,
    };
    let said = steps
        .track("shell", async { needs(InstallWsl, false) })
        .await
        .unwrap_err();
    assert_eq!(said, "the step's sentence");
    assert_eq!(
        taken(),
        [
            (running.clone(), None),
            (unavailable.clone(), Some(InstallWsl))
        ]
    );
    assert_eq!(steps.needs("shell"), Ok(Some(InstallWsl)));
    steps
        .track("shell", async { needs(InstallWsl, true) })
        .await
        .unwrap_err();
    assert_eq!(
        taken(),
        [(running.clone(), None), (failed, Some(InstallWsl))]
    );
    assert!(steps.retry().is_ok());
    steps
        .set("shell", unavailable.clone(), Some(Restart))
        .unwrap();
    assert_eq!(taken(), [(unavailable, Some(Restart))]);
    steps
        .track("shell", async { Ok::<_, StepError>(()) })
        .await
        .unwrap();
    let list = taken();
    assert_eq!(list[0], (running, None));
    assert!(matches!(list[1], (StepState::Done { .. }, None)));
    assert_eq!(steps.needs("shell"), Ok(None));
    assert_eq!(steps.needs("nothing"), Err("No setup step nothing".into()));
}
