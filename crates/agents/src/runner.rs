use std::collections::VecDeque;
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot};

use crate::invocation::{Invocation, RunPlan};
use crate::parse::{OutputParser, RunEvent};
use crate::profile::OutputFormat;

const STDERR_TAIL_LINES: usize = 20;
const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunUpdate {
    Event(RunEvent),
    Stderr(String),
    Started(u32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunStatus {
    Completed {
        final_text: String,
    },
    Failed {
        message: String,
        stderr_tail: Vec<String>,
    },
    Stopped,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunOutcome {
    pub status: RunStatus,
    pub exit_code: Option<i32>,
}

fn command_for(invocation: &Invocation) -> Command {
    let mut command = Command::new(&invocation.program);
    command.args(&invocation.args);
    if let Some(cwd) = &invocation.cwd {
        command.current_dir(cwd);
    }
    command.kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    command
}

fn failed(message: String, stderr_tail: Vec<String>, exit_code: Option<i32>) -> RunOutcome {
    RunOutcome {
        status: RunStatus::Failed {
            message,
            stderr_tail,
        },
        exit_code,
    }
}

pub async fn run_stop_invocation(invocation: &Invocation) {
    let mut command = command_for(invocation);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Ok(mut child) = command.spawn() {
        let _ = child.wait().await;
    }
}

fn snapshot(tail: &Mutex<VecDeque<String>>) -> Vec<String> {
    tail.lock()
        .map(|lines| lines.iter().cloned().collect())
        .unwrap_or_default()
}

pub async fn run(
    plan: RunPlan,
    format: OutputFormat,
    events: mpsc::UnboundedSender<RunUpdate>,
    stop: oneshot::Receiver<()>,
) -> RunOutcome {
    let cleanup = plan.cleanup.clone();
    let outcome = run_plan(plan, format, events, stop).await;
    for path in cleanup {
        let _ = std::fs::remove_file(path);
    }
    outcome
}

async fn run_plan(
    plan: RunPlan,
    format: OutputFormat,
    events: mpsc::UnboundedSender<RunUpdate>,
    mut stop: oneshot::Receiver<()>,
) -> RunOutcome {
    let mut command = command_for(&plan.run);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return failed(
                format!("cannot start {}: {error}", plan.run.program),
                Vec::new(),
                None,
            );
        }
    };
    if let Some(process_id) = child.id() {
        let _ = events.send(RunUpdate::Started(process_id));
    }
    let (Some(stdin), Some(stdout), Some(stderr)) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        return failed(
            "cannot attach to the process pipes".to_string(),
            Vec::new(),
            None,
        );
    };

    let prompt = plan.run.stdin.clone();
    let stdin_task = tokio::spawn(async move {
        let mut stdin = stdin;
        if let Some(prompt) = prompt {
            let _ = stdin.write_all(prompt.as_bytes()).await;
        }
        let _ = stdin.shutdown().await;
    });

    let stderr_tail = Arc::new(Mutex::new(VecDeque::new()));
    let stderr_task = {
        let stderr_tail = Arc::clone(&stderr_tail);
        let events = events.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Ok(mut tail) = stderr_tail.lock() {
                    if tail.len() == STDERR_TAIL_LINES {
                        tail.pop_front();
                    }
                    tail.push_back(line.clone());
                }
                let _ = events.send(RunUpdate::Stderr(line));
            }
        })
    };

    let mut parser = OutputParser::new(format);
    let mut final_text: Option<String> = None;
    let mut failure_message: Option<String> = None;
    let mut stdout_lines = BufReader::new(stdout).lines();
    let mut stop_armed = true;

    let mut handle_events = |parsed: Vec<RunEvent>| {
        for event in parsed {
            match &event {
                RunEvent::Final(text) => final_text = Some(text.clone()),
                RunEvent::Failed(message) => failure_message = Some(message.clone()),
                _ => {}
            }
            let _ = events.send(RunUpdate::Event(event));
        }
    };

    loop {
        tokio::select! {
            stop_result = &mut stop, if stop_armed => {
                if stop_result.is_ok() {
                    let _ = child.kill().await;
                    if let Some(stop_invocation) = &plan.stop {
                        run_stop_invocation(stop_invocation).await;
                    }
                    stdin_task.abort();
                    stderr_task.abort();
                    return RunOutcome { status: RunStatus::Stopped, exit_code: None };
                }
                stop_armed = false;
            }
            line = stdout_lines.next_line() => match line {
                Ok(Some(line)) => handle_events(parser.feed(&line)),
                _ => break,
            }
        }
    }
    handle_events(parser.finish());

    let exit_code = child.wait().await.ok().and_then(|status| status.code());
    let _ = stdin_task.await;
    let _ = stderr_task.await;
    let tail = snapshot(&stderr_tail);

    match (exit_code, final_text) {
        (Some(0), Some(final_text)) => RunOutcome {
            status: RunStatus::Completed { final_text },
            exit_code,
        },
        _ => {
            let message = failure_message
                .or_else(|| tail.last().cloned())
                .unwrap_or_else(|| match exit_code {
                    Some(code) => format!("exited with code {code}"),
                    None => "exited without a result".to_string(),
                });
            failed(message, tail, exit_code)
        }
    }
}

fn first_line(bytes: &[u8]) -> Option<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

pub async fn probe(invocation: &Invocation) -> Result<String, String> {
    let mut command = command_for(invocation);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = command
        .spawn()
        .map_err(|error| format!("cannot start {}: {error}", invocation.program))?;
    let output = tokio::time::timeout(PROBE_TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| "no answer within 10 seconds".to_string())?
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        return Ok(first_line(&output.stdout)
            .or_else(|| first_line(&output.stderr))
            .unwrap_or_default());
    }
    Err(first_line(&output.stderr)
        .or_else(|| first_line(&output.stdout))
        .unwrap_or_else(|| match output.status.code() {
            Some(code) => format!("exited with code {code}"),
            None => "exited without a result".to_string(),
        }))
}
