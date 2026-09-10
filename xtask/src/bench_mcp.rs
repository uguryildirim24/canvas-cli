//! What the agent surface costs: `cargo xtask bench --mcp`.
//!
//! Three numbers, all measured against the same primed fixture the SPEC §13
//! metrics use:
//!
//! - **Schema size per tool.** The bytes `tools/list` actually puts on the
//!   wire for each tool, and a token estimate from them. A host pays this
//!   once per session, before the model has read a single course.
//! - **Warm `todo.list` round trip** over stdio, p50 and p95. The target is
//!   p95 < 100 ms, excluding model time.
//! - **Tool calls per workflow** for the six workflows the shipped skill
//!   documents. A workflow that needs four round trips costs four turns.
//!
//! The client here is hand-written JSON-RPC over the child's pipes, so the
//! measured bytes and times are the ones a host sees.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::Instant;

use anyhow::{Context, Result, bail};

use crate::bench::percentile;
use serde_json::{Value, json};

/// The revision the measurement negotiates.
const PROTOCOL: &str = "2026-07-28";

/// Round trips discarded before the timed ones.
pub const WARMUP: u32 = 5;

/// SPEC-adjacent target for one warm tool call over stdio, in milliseconds.
pub const ROUND_TRIP_P95_MS: f64 = 100.0;

/// Bytes per token in the estimate below.
///
/// A rule of thumb, not a tokenizer: byte-pair encoders in the `cl100k`
/// family average close to four bytes per token on English text and on JSON
/// with English identifiers. The report says so, and the byte count next to
/// it is exact.
pub const BYTES_PER_TOKEN: usize = 4;

/// What one tool's definition costs a host.
pub struct ToolCost {
    pub name: String,
    pub bytes: usize,
    pub tokens: usize,
}

/// Round-trip latency of one warm tool call.
pub struct Latency {
    pub p50: f64,
    pub p95: f64,
    pub runs: u32,
}

impl Latency {
    #[must_use]
    pub fn missed(&self) -> bool {
        self.p95 > ROUND_TRIP_P95_MS
    }
}

/// One workflow of the shipped skill, as round trips.
pub struct WorkflowCost {
    pub name: &'static str,
    /// The calls this benchmark issued, in order, with their outcome.
    pub calls: Vec<(String, String)>,
    /// Calls the workflow needs that this harness cannot issue.
    pub unmeasured: usize,
    /// Why those calls are unmeasured. Empty when there are none.
    pub unmeasured_note: &'static str,
    /// Wall-clock time of the measured calls, in milliseconds.
    pub total_ms: f64,
}

impl WorkflowCost {
    /// Round trips the workflow costs in full.
    #[must_use]
    pub fn total_calls(&self) -> usize {
        self.calls.len() + self.unmeasured
    }
}

/// Everything `--mcp` adds to the report.
pub struct Report {
    pub catalog: Vec<ToolCost>,
    pub latency: Latency,
    pub workflows: Vec<WorkflowCost>,
}

impl Report {
    /// Bytes every `tools/list` puts on the wire.
    #[must_use]
    pub fn catalog_bytes(&self) -> usize {
        self.catalog.iter().map(|tool| tool.bytes).sum()
    }

    /// Estimated tokens of the whole catalog.
    #[must_use]
    pub fn catalog_tokens(&self) -> usize {
        self.catalog.iter().map(|tool| tool.tokens).sum()
    }

    #[must_use]
    pub fn missed(&self) -> bool {
        self.latency.missed()
    }
}

/// One `canvas mcp` process, spoken to as a host would.
struct Client {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Client {
    fn start(mut command: Command) -> Result<Self> {
        command
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command.spawn().context("spawn canvas mcp")?;
        let stdin = child.stdin.take().context("no stdin pipe")?;
        let stdout = BufReader::new(child.stdout.take().context("no stdout pipe")?);
        Ok(Self {
            child,
            stdin: Some(stdin),
            stdout,
            next_id: 1,
        })
    }

    /// Send one request and read its response. Returns the elapsed ms too.
    fn request(&mut self, method: &str, arguments: Value) -> Result<(Value, f64)> {
        let id = self.next_id;
        self.next_id += 1;
        let mut params = arguments;
        params["_meta"] = json!({
            "io.modelcontextprotocol/protocolVersion": PROTOCOL,
            "io.modelcontextprotocol/clientInfo": { "name": "xtask-bench", "version": "0" },
            "io.modelcontextprotocol/clientCapabilities": {},
        });
        let line = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let started = Instant::now();
        let stdin = self.stdin.as_mut().context("the pipe is closed")?;
        writeln!(stdin, "{line}")?;
        stdin.flush()?;
        loop {
            let mut buffer = String::new();
            if self.stdout.read_line(&mut buffer)? == 0 {
                bail!("the server closed stdout while answering {method}");
            }
            let message: Value =
                serde_json::from_str(&buffer).with_context(|| format!("not JSON-RPC: {buffer}"))?;
            if message["id"] == json!(id) {
                let elapsed = started.elapsed().as_secs_f64() * 1000.0;
                if let Some(error) = message.get("error").filter(|e| !e.is_null()) {
                    bail!("{method} failed: {error}");
                }
                return Ok((message["result"].clone(), elapsed));
            }
        }
    }

    /// One tool call. A domain refusal is an answer, so only a protocol
    /// failure is an error here.
    fn call(&mut self, name: &str, arguments: &Value) -> Result<(Value, f64)> {
        self.request(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        )
    }

    fn stop(&mut self) {
        self.stdin.take();
        let _ = self.child.wait();
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The outcome an envelope reports, for the workflow table.
fn outcome_of(result: &Value) -> String {
    let document = &result["structuredContent"];
    let outcome = document["outcome"].as_str().unwrap_or("?");
    let exit = document["exit"].as_u64().unwrap_or_default();
    format!("{outcome} ({exit})")
}

/// One workflow of the skill: what to issue, and what stays unmeasured.
struct Workflow {
    name: &'static str,
    calls: Vec<(&'static str, Value)>,
    unmeasured: usize,
    unmeasured_note: &'static str,
}

/// The six workflows the shipped skill documents.
///
/// Each entry is the sequence this harness can issue offline, plus the count
/// of calls the workflow needs that it cannot: a submission needs the network
/// and a person, a reconciliation has no journal to resolve, and a transfer
/// needs the network.
fn workflows(course: i64, assignment: i64) -> Vec<Workflow> {
    let course = course.to_string();
    let assignment = assignment.to_string();
    vec![
        Workflow {
            name: "organize the week",
            calls: vec![("todo.list", json!({}))],
            unmeasured: 0,
            unmeasured_note: "",
        },
        Workflow {
            name: "read an assignment",
            calls: vec![
                (
                    "assignments.list",
                    json!({ "course": course, "search": "Assignment 1" }),
                ),
                (
                    "assignment.get",
                    json!({ "course": course, "assignment": assignment }),
                ),
            ],
            unmeasured: 0,
            unmeasured_note: "",
        },
        Workflow {
            name: "prepare and submit with approval",
            calls: vec![
                (
                    "assignment.get",
                    json!({ "course": course, "assignment": assignment }),
                ),
                (
                    "submission.get",
                    json!({ "course": course, "assignment": assignment }),
                ),
            ],
            unmeasured: 3,
            unmeasured_note: "`submission.prepare`, then `submission.execute` twice: the \
                              first returns `input_required`, the retry carries the \
                              approval. All three need the network and a real file",
        },
        Workflow {
            name: "reconcile an unknown outcome",
            calls: vec![
                ("receipts.list", json!({})),
                (
                    "submission.get",
                    json!({ "course": course, "assignment": assignment }),
                ),
            ],
            unmeasured: 1,
            unmeasured_note: "`submission.reconcile`, which needs an unresolved journal",
        },
        Workflow {
            name: "reply and message with approval",
            calls: vec![
                ("discussions.list", json!({ "course": course })),
                ("inbox.list", json!({})),
            ],
            unmeasured: 4,
            unmeasured_note: "a prepare, then an execute twice (the first returns \
                              `input_required`, the retry carries the approval), and \
                              `operation.status`: all four need the network and a person",
        },
        Workflow {
            name: "download course files",
            calls: vec![
                ("files.list", json!({ "course": course, "tree": true })),
                ("modules.list", json!({ "course": course, "items": true })),
                ("download.plan", json!({ "course": course })),
            ],
            unmeasured: 1,
            unmeasured_note: "`download.run`, which needs the network",
        },
    ]
}

/// Measure the agent surface. `command` builds a fresh `canvas` invocation.
pub fn measure(
    command: &dyn Fn() -> Command,
    runs: u32,
    course: i64,
    assignment: i64,
) -> Result<Report> {
    let mut client = Client::start(command())?;

    // What a host pays before the model has read anything.
    let (listed, _) = client.request("tools/list", json!({}))?;
    let tools = listed["tools"]
        .as_array()
        .context("tools/list returned no tools")?;
    let mut catalog = Vec::new();
    for tool in tools {
        let name = tool["name"].as_str().unwrap_or_default().to_owned();
        let bytes = serde_json::to_string(tool)?.len();
        catalog.push(ToolCost {
            name,
            bytes,
            tokens: bytes.div_ceil(BYTES_PER_TOKEN),
        });
    }

    // Warm round trip of the command a session opens with.
    for _ in 0..WARMUP {
        client.call("todo.list", &json!({}))?;
    }
    let mut samples = Vec::new();
    for _ in 0..runs {
        let (result, elapsed) = client.call("todo.list", &json!({}))?;
        if result["isError"] == json!(true) {
            bail!("todo.list refused: {}", result["structuredContent"]);
        }
        samples.push(elapsed);
    }
    let latency = Latency {
        p50: percentile(&samples, 0.50),
        p95: percentile(&samples, 0.95),
        runs,
    };

    // Round trips per documented workflow.
    let mut measured = Vec::new();
    for workflow in workflows(course, assignment) {
        let mut issued = Vec::new();
        let mut total = 0.0;
        for (tool, arguments) in workflow.calls {
            let (result, elapsed) = client.call(tool, &arguments)?;
            total += elapsed;
            issued.push((tool.to_owned(), outcome_of(&result)));
        }
        measured.push(WorkflowCost {
            name: workflow.name,
            calls: issued,
            unmeasured: workflow.unmeasured,
            unmeasured_note: workflow.unmeasured_note,
            total_ms: total,
        });
    }

    client.stop();
    Ok(Report {
        catalog,
        latency,
        workflows: measured,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_token_estimate_rounds_up() {
        assert_eq!(1_usize.div_ceil(BYTES_PER_TOKEN), 1);
        assert_eq!(4_usize.div_ceil(BYTES_PER_TOKEN), 1);
        assert_eq!(5_usize.div_ceil(BYTES_PER_TOKEN), 2);
    }

    /// Every workflow the skill documents is measured, and every one that
    /// cannot be measured in full says why.
    #[test]
    fn the_six_skill_workflows_are_covered() {
        let workflows = workflows(101, 10101);
        let names: Vec<&str> = workflows.iter().map(|w| w.name).collect();
        assert_eq!(
            names,
            [
                "organize the week",
                "read an assignment",
                "prepare and submit with approval",
                "reconcile an unknown outcome",
                "reply and message with approval",
                "download course files",
            ]
        );
        for workflow in workflows {
            let name = workflow.name;
            assert!(!workflow.calls.is_empty(), "{name} issues no call");
            assert_eq!(
                workflow.unmeasured > 0,
                !workflow.unmeasured_note.is_empty(),
                "{name} does not explain its unmeasured calls"
            );
        }
    }
}
