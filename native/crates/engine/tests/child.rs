//! `test/engine.test.js` on engine children: the protocol's words and
//! files, a child that cannot start, one stopped while it starts, one that
//! dies with its stderr, and one that ignores `SIGTERM`. The fakes are
//! shell scripts on Unix and PowerShell scripts on Windows, as today's
//! tests fake the child process; the start that cannot read its tables is
//! the real program (`MCBENCH_TABLES` and `MCBENCH_ENGINE_PROGRAM` at build
//! time).

mod common;

use mcbench_engine::{EngineError, EngineProgram, PlaceCommands, VerifyRequest};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

fn never() -> CancellationToken {
    CancellationToken::new()
}

// engine.test.js: "the engine is built for the server and runs placements
// and experiments": the requests as the child reads them, in the child's
// own folder (its working directory), and its replies read: a placement on
// the world the last one left, an experiment and its failure, and the ticks
// with and without light.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_requests_are_the_protocols_words_in_the_childs_own_folder() {
    let folder = common::scratch("words");
    let log = folder.join("requests.log");
    let sh = format!(
        r#"pwd -P > {cwd}
echo MCBENCH_ENGINE_READY
while read -r line; do
  echo "$line" >> {log}
  set -- $line
  case $1 in
    PLACE) printf blueprint > "$4"; echo "MCBENCH_PLACED $2 3";;
    TICK) printf patch > "$4"; printf '[]' > "$5"; echo "MCBENCH_TICKED $2 3";;
    VERIFY)
      if [ -e failed ]; then
        echo "MCBENCH_VERIFY_FAILED $2 java.lang.UnsupportedOperationException: Light is not modelled"
      else
        mkdir -p "$5"; printf '{{"ok":true}}' > "$5/result.json"; printf '{{}}' > "$5/report.json"
        touch failed; echo "MCBENCH_VERIFIED $2 12"
      fi;;
  esac
done
"#,
        cwd = folder.join("cwd").display(),
        log = log.display()
    );
    let powershell = r#"(Get-Location).ProviderPath | Out-File -Encoding ascii -NoNewline '@CWD@'
[Console]::Out.WriteLine('MCBENCH_ENGINE_READY')
while ($null -ne ($line = [Console]::In.ReadLine())) {
  [IO.File]::AppendAllText('@LOG@', "$line`n")
  $w = $line.Split(' ')
  switch ($w[0]) {
    'PLACE' { [IO.File]::WriteAllText($w[3], 'blueprint'); [Console]::Out.WriteLine("MCBENCH_PLACED $($w[1]) 3") }
    'TICK' { [IO.File]::WriteAllText($w[3], 'patch'); [IO.File]::WriteAllText($w[4], '[]'); [Console]::Out.WriteLine("MCBENCH_TICKED $($w[1]) 3") }
    'VERIFY' {
      if (Test-Path failed) {
        [Console]::Out.WriteLine("MCBENCH_VERIFY_FAILED $($w[1]) java.lang.UnsupportedOperationException: Light is not modelled")
      } else {
        New-Item -ItemType Directory -Force $w[4] | Out-Null
        [IO.File]::WriteAllText("$($w[4])/result.json", '{"ok":true}'); [IO.File]::WriteAllText("$($w[4])/report.json", '{}')
        New-Item failed | Out-Null; [Console]::Out.WriteLine("MCBENCH_VERIFIED $($w[1]) 12")
      }
    }
  }
}
"#
    .replace("@CWD@", &folder.join("cwd").display().to_string())
    .replace("@LOG@", &log.display().to_string());
    let program = common::fake(&folder, "recording.sh", common::script(&sh, &powershell));
    let child = common::spawn(&program);
    child.ready(&never()).await.unwrap();
    let placed = child
        .place(
            &PlaceCommands("setblock 0 64 0 minecraft:stone\n".into()),
            false,
            &never(),
        )
        .await
        .unwrap();
    assert_eq!(placed.0, b"blueprint");
    let request = VerifyRequest {
        request: "{}".into(),
        commands: PlaceCommands(String::new()),
    };
    let report = child.verify(&request, |_| {}, &never()).await.unwrap();
    assert_eq!(report.result, "{\"ok\":true}");
    assert_eq!(report.files, [("report.json".to_string(), b"{}".to_vec())]);
    let failed = child.verify(&request, |_| {}, &never()).await.unwrap_err();
    assert_eq!(
        failed.to_string(),
        "The engine failed: java.lang.UnsupportedOperationException: Light is not modelled"
    );
    let stock = child.tick(100, true, &never()).await.unwrap();
    assert_eq!(
        (stock.changes, stock.entities),
        (b"patch".to_vec(), b"[]".to_vec())
    );
    child.tick(100, false, &never()).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(&log)
            .unwrap()
            .lines()
            .collect::<Vec<_>>(),
        [
            "PLACE 1 place.txt blueprint.bin continue",
            "VERIFY 2 verify-2/request.json verify-2/commands.txt verify-2/out",
            "VERIFY 3 verify-3/request.json verify-3/commands.txt verify-3/out",
            "TICK 4 100 patch.bin entities.json",
            "TICK 5 100 patch.bin entities.json nolight",
        ]
    );
    let cwd = std::fs::read_to_string(folder.join("cwd")).unwrap();
    assert_eq!(
        std::path::Path::new(cwd.trim()).canonicalize().unwrap(),
        child.folder().canonicalize().unwrap()
    );
    assert_eq!(
        std::fs::read_to_string(child.folder().join("place.txt")).unwrap(),
        "setblock 0 64 0 minecraft:stone\n"
    );
    child.stop().await;
    std::fs::remove_dir_all(&folder).unwrap();
}

// engine.test.js: "an engine that cannot start reports why" (and "a failed
// build reaches the caller and the next warm builds again": nothing is
// built, the next child starts again). The program that cannot read a
// table exits with code 2 and its sentence, which is the start's failure
// and every request's.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[cfg_attr(
    not(all(mcbench_tables, mcbench_program)),
    ignore = "needs Minecraft's tables and the children's program: set MCBENCH_TABLES and MCBENCH_ENGINE_PROGRAM at build time"
)]
async fn an_engine_that_cannot_start_reports_why_and_the_next_starts_again() {
    let folder = common::scratch("cannot-start");
    for name in ["blocks.txt", "loot.json"] {
        std::fs::copy(common::tables().join(name), folder.join(name)).unwrap();
    }
    let program = EngineProgram::new(common::program().binary(), &folder);
    let expected = format!(
        "The engine stopped (code 2): mcbench-core: cannot read Minecraft's table {}: ",
        folder.join("datafix.txt").display()
    );
    for _ in 0..2 {
        let child = program.spawn().unwrap();
        let error = child.ready(&never()).await.unwrap_err().to_string();
        assert!(error.starts_with(&expected), "{error}");
        assert!(!child.alive());
        let again = child.state(&never()).await.unwrap_err().to_string();
        assert_eq!(again, error);
    }
    std::fs::remove_dir_all(&folder).unwrap();
}

// engine.test.js: "stopping the engine while it is being built stops the
// build" and "an engine stopped while it is built never spawns, and its
// requests fail plainly". A child is not built; one stopped while it
// starts (it never said it is ready) is gone, its start fails with its
// exit, and its requests fail plainly.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_child_stopped_while_it_starts_is_gone_and_its_requests_fail_plainly() {
    let folder = common::scratch("stopped-starting");
    let program = common::fake(
        &folder,
        "slow.sh",
        common::script("exec sleep 600\n", "Start-Sleep 600\n"),
    );
    let child = common::spawn(&program);
    let pid = child.pid();
    let began = Instant::now();
    child.stop().await;
    assert!(began.elapsed() < common::AT_ONCE);
    assert!(!common::alive(pid));
    assert_eq!(
        child.ready(&never()).await.unwrap_err().to_string(),
        "The engine stopped (SIGTERM): "
    );
    assert!(!child.alive());
    assert_eq!(
        child
            .place(
                &PlaceCommands("setblock 0 64 0 minecraft:stone\n".into()),
                true,
                &never()
            )
            .await
            .unwrap_err()
            .to_string(),
        "The engine was stopped"
    );
    std::fs::remove_dir_all(&folder).unwrap();
}

// engine.test.js: "a dead engine fails requests with its own stderr, and a
// restart starts with fresh output".
// On Windows the first child ends itself with `Stop-Process -Force`
// (`TerminateProcess`), which has no signal: its exit reads its code.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dead_child_fails_requests_with_its_own_stderr_and_a_new_one_starts_with_fresh_output() {
    let folder = common::scratch("dead");
    let first = common::fake(
        &folder,
        "first.sh",
        common::script(
            "echo MCBENCH_ENGINE_READY\nread -r line\nprintf 'first process ran out of memory' >&2\nkill -KILL $$\n",
            "[Console]::Out.WriteLine('MCBENCH_ENGINE_READY')\n$null = [Console]::In.ReadLine()\n[Console]::Error.Write('first process ran out of memory')\nStop-Process -Id $PID -Force\n",
        ),
    );
    let died = if cfg!(windows) {
        DIED_ON_WINDOWS
    } else {
        "SIGKILL"
    };
    let child = common::spawn(&first);
    child.ready(&never()).await.unwrap();
    let pending = child
        .place(
            &PlaceCommands("setblock 0 64 0 minecraft:stone\n".into()),
            true,
            &never(),
        )
        .await
        .unwrap_err();
    assert_eq!(
        pending.to_string(),
        format!("The engine stopped ({died}): first process ran out of memory")
    );
    assert!(matches!(pending, EngineError::Stopped(_)));
    assert!(!child.alive());
    assert_eq!(
        child.tick(1, true, &never()).await.unwrap_err().to_string(),
        format!("The engine stopped ({died}): first process ran out of memory")
    );
    // The next child's failure says only its own output.
    let second = common::fake(
        &folder,
        "second.sh",
        common::script(
            "printf second >&2\nexit 1\n",
            "[Console]::Error.Write('second')\nexit 1\n",
        ),
    );
    let again = common::spawn(&second);
    let error = again.ready(&never()).await.unwrap_err().to_string();
    assert_eq!(error, "The engine stopped (code 1): second");
    std::fs::remove_dir_all(&folder).unwrap();
}

// engine.test.js: "an old process closing late leaves the new process's
// requests alone". A child that ignores SIGTERM gets SIGKILL ten seconds
// after it was told to stop, its stop resolves then, and another child
// answers its requests meanwhile. Windows has no SIGTERM to ignore: there
// the stop ends the child's job at once, before the other child answers.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_child_that_ignores_sigterm_is_killed_after_ten_seconds_and_another_answers_meanwhile() {
    let folder = common::scratch("late");
    let stubborn = common::fake(
        &folder,
        "stubborn.sh",
        common::script(
            "trap '' TERM\necho MCBENCH_ENGINE_READY\nwhile true; do sleep 1; done\n",
            "[Console]::Out.WriteLine('MCBENCH_ENGINE_READY')\nwhile ($true) { Start-Sleep 1 }\n",
        ),
    );
    let answering = common::fake(
        &folder,
        "answering.sh",
        common::script(
            "echo MCBENCH_ENGINE_READY\nwhile read -r verb id file; do printf 'time 0\\n' > \"$file\"; echo \"MCBENCH_DONE $id 1\"; done\n",
            "[Console]::Out.WriteLine('MCBENCH_ENGINE_READY')\nwhile ($null -ne ($line = [Console]::In.ReadLine())) { $w = $line.Split(' '); [IO.File]::WriteAllText($w[2], \"time 0`n\"); [Console]::Out.WriteLine(\"MCBENCH_DONE $($w[1]) 1\") }\n",
        ),
    );
    let old = std::sync::Arc::new(common::spawn(&stubborn));
    old.ready(&never()).await.unwrap();
    let pid = old.pid();
    let began = Instant::now();
    let stopping = tokio::spawn({
        let old = old.clone();
        async move { old.stop().await }
    });
    let new = common::spawn(&answering);
    assert_eq!(new.state(&never()).await.unwrap(), "time 0\n");
    // On Unix the stubborn child is closing for ten seconds; on Windows its
    // job ends at once (its stop's time is checked below, not that it came
    // before the new child's answer: which of the two comes first is the
    // machine's timing).
    if cfg!(unix) {
        assert!(common::alive(pid), "it is still closing");
    }
    stopping.await.unwrap();
    let waited = began.elapsed();
    if cfg!(windows) {
        assert!(waited < common::AT_ONCE, "{waited:?}");
    } else {
        assert!(
            waited >= Duration::from_secs(10) && waited < Duration::from_secs(10) + common::AT_ONCE,
            "{waited:?}"
        );
    }
    assert!(!common::alive(pid));
    assert!(new.alive());
    assert_eq!(new.state(&never()).await.unwrap(), "time 0\n");
    new.stop().await;
    std::fs::remove_dir_all(&folder).unwrap();
}

/// How the first child of the dead-child case reads on Windows, where
/// `Stop-Process -Force` ends powershell.exe with `TerminateProcess` and
/// cmd.exe passes its code on.
const DIED_ON_WINDOWS: &str = "code -1";
