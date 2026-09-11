//! What the agent surface costs: `cargo xtask bench --mcp`.
//!
//! The surface is one tool, `getclitools` (§21.2), so there are three
//! numbers and they are all about what that one tool costs:
//!
//! - **Schema size.** The bytes `tools/list` actually puts on the wire, and
//!   a token estimate from them. A host pays this once per session, before
//!   the model has read anything.
//! - **Warm `getclitools` round trip** over stdio, p50 and p95. The target is
//!   p95 < 100 ms, excluding model time.
//! - **Answer size.** The bytes the one call returns: the whole `canvas`
//!   command reference, which the model reads once and then runs commands.
//!
//! There is no workflow table any more. Every step of every skill workflow is
//! a `canvas` command the agent runs itself, so a workflow costs exactly one
//! MCP round trip whatever it does next.
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

/// The only tool the server serves (§19 item 50).
const TOOL: &str = "getclitools";

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

/// What the one call returns: the whole `canvas` command reference.
pub struct ReferenceCost {
    pub bytes: usize,
    pub tokens: usize,
    /// How many commands the reference describes.
    pub commands: usize,
}

/// Everything `--mcp` adds to the report.
pub struct Report {
    pub catalog: Vec<ToolCost>,
    pub latency: Latency,
    pub reference: ReferenceCost,
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

/// Measure the agent surface. `command` builds a fresh `canvas` invocation.
pub fn measure(command: &dyn Fn() -> Command, runs: u32) -> Result<Report> {
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

    // Warm round trip of the one call a session opens with.
    for _ in 0..WARMUP {
        client.call(TOOL, &json!({}))?;
    }
    let mut samples = Vec::new();
    let mut answer = String::new();
    for _ in 0..runs {
        let (result, elapsed) = client.call(TOOL, &json!({}))?;
        if result["isError"] == json!(true) {
            bail!("{TOOL} refused: {result}");
        }
        result["content"][0]["text"]
            .as_str()
            .context("the tool answered with no text")?
            .clone_into(&mut answer);
        samples.push(elapsed);
    }
    let latency = Latency {
        p50: percentile(&samples, 0.50),
        p95: percentile(&samples, 0.95),
        runs,
    };
    let bytes = answer.len();
    let reference = ReferenceCost {
        bytes,
        tokens: bytes.div_ceil(BYTES_PER_TOKEN),
        commands: answer
            .lines()
            .filter(|line| line.starts_with("### "))
            .count(),
    };

    client.stop();
    Ok(Report {
        catalog,
        latency,
        reference,
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

    /// The benchmark measures the tool the server actually serves.
    #[test]
    fn the_measured_tool_is_the_only_tool() {
        assert_eq!(TOOL, "getclitools");
    }
}
