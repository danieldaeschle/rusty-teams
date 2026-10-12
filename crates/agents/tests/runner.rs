#![cfg(unix)]

use std::time::{Duration, Instant};

use agents::{Invocation, OutputFormat, RunEvent, RunPlan, RunStatus, RunUpdate, probe, run};
use tokio::sync::{mpsc, oneshot};

fn plan(program: &str, args: &[&str], stdin: Option<&str>) -> RunPlan {
    RunPlan {
        run: Invocation {
            program: program.to_string(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
            cwd: Some("/".into()),
            stdin: stdin.map(str::to_string),
        },
        stop: None,
        cleanup: Vec::new(),
    }
}

#[tokio::test]
async fn completes_with_claude_final_text() {
    let fixture = format!(
        "{}/tests/fixtures/claude-stream.jsonl",
        env!("CARGO_MANIFEST_DIR")
    );
    let (sender, mut receiver) = mpsc::unbounded_channel();
    let (_keep_alive, stop) = oneshot::channel();
    let outcome = run(
        plan("sh", &["-c", &format!("cat '{fixture}'")], Some("prompt")),
        OutputFormat::ClaudeStreamJson,
        sender,
        stop,
    )
    .await;
    assert_eq!(
        outcome.status,
        RunStatus::Completed {
            final_text: "The first word in `/etc/hostname` is `HOST01`.".to_string()
        }
    );
    assert_eq!(outcome.exit_code, Some(0));
    let mut saw_step = false;
    while let Ok(update) = receiver.try_recv() {
        if update == RunUpdate::Event(RunEvent::Step("Reading hostname".to_string())) {
            saw_step = true;
        }
    }
    assert!(saw_step);
}

#[tokio::test]
async fn failing_command_reports_stderr() {
    let (sender, mut receiver) = mpsc::unbounded_channel();
    let (_keep_alive, stop) = oneshot::channel();
    let outcome = run(
        plan("sh", &["-c", "echo boom >&2; exit 3"], None),
        OutputFormat::ClaudeStreamJson,
        sender,
        stop,
    )
    .await;
    assert_eq!(outcome.exit_code, Some(3));
    match outcome.status {
        RunStatus::Failed {
            message,
            stderr_tail,
        } => {
            assert_eq!(message, "boom");
            assert_eq!(stderr_tail, vec!["boom".to_string()]);
        }
        other => panic!("unexpected status {other:?}"),
    }
    assert_eq!(
        receiver.try_recv().ok(),
        Some(RunUpdate::Stderr("boom".to_string()))
    );
}

#[tokio::test]
async fn missing_program_fails_readably() {
    let (sender, _receiver) = mpsc::unbounded_channel();
    let (_keep_alive, stop) = oneshot::channel();
    let outcome = run(
        plan("definitely-not-a-real-program", &[], None),
        OutputFormat::Text,
        sender,
        stop,
    )
    .await;
    assert!(matches!(
        outcome.status,
        RunStatus::Failed { message, .. } if message.contains("cannot start definitely-not-a-real-program")
    ));
}

#[tokio::test]
async fn stop_kills_the_process() {
    let (sender, _receiver) = mpsc::unbounded_channel();
    let (stop_sender, stop) = oneshot::channel();
    let started = Instant::now();
    let handle = tokio::spawn(run(
        plan("sh", &["-c", "sleep 30"], None),
        OutputFormat::Text,
        sender,
        stop,
    ));
    tokio::time::sleep(Duration::from_millis(200)).await;
    stop_sender.send(()).unwrap();
    let outcome = handle.await.unwrap();
    assert_eq!(outcome.status, RunStatus::Stopped);
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn probe_returns_the_first_output_line() {
    let invocation = Invocation {
        program: "sh".to_string(),
        args: ["-c", "echo '2.1.290 (Claude Code)'; echo more"]
            .map(String::from)
            .to_vec(),
        cwd: None,
        stdin: None,
    };
    assert_eq!(
        probe(&invocation).await,
        Ok("2.1.290 (Claude Code)".to_string())
    );
}

#[tokio::test]
async fn probe_reports_stderr_of_a_failing_command() {
    let invocation = Invocation {
        program: "sh".to_string(),
        args: ["-c", "echo 'not logged in' >&2; exit 2"]
            .map(String::from)
            .to_vec(),
        cwd: None,
        stdin: None,
    };
    assert_eq!(probe(&invocation).await, Err("not logged in".to_string()));
    let missing = Invocation {
        program: "definitely-not-a-program".to_string(),
        args: Vec::new(),
        cwd: None,
        stdin: None,
    };
    assert!(
        probe(&missing)
            .await
            .unwrap_err()
            .starts_with("cannot start")
    );
}

#[tokio::test]
async fn cleanup_files_are_removed_when_the_run_ends() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("mcp.json");
    std::fs::write(&config, "{}").unwrap();
    let mut finished_plan = plan("sh", &["-c", "exit 0"], None);
    finished_plan.cleanup = vec![config.clone()];
    let (sender, _receiver) = mpsc::unbounded_channel();
    let (_keep_alive, stop) = oneshot::channel();
    run(finished_plan, OutputFormat::ClaudeStreamJson, sender, stop).await;
    assert!(!config.exists());

    std::fs::write(&config, "{}").unwrap();
    let mut stopped_plan = plan("sleep", &["30"], None);
    stopped_plan.cleanup = vec![config.clone()];
    let (sender, _receiver) = mpsc::unbounded_channel();
    let (stop_sender, stop) = oneshot::channel();
    stop_sender.send(()).unwrap();
    run(stopped_plan, OutputFormat::ClaudeStreamJson, sender, stop).await;
    assert!(!config.exists());
}
