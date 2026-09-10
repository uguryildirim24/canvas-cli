//! One command's finished work: the §7 envelope plus its human renderer.
//!
//! Every command core returns [`Handled`]. The CLI turns it into terminal
//! output and a process exit; `canvas mcp` reads the same envelope and
//! serializes it into a tool result. There is one implementation of each
//! command, and the adapter never shells out to `canvas`.

use std::io::{self, Write};
use std::process::ExitCode;
use std::rc::Rc;

use serde::Serialize;

use crate::output::{Envelope, Freshness, IdentityRef, Outcome, PartialScope, error_envelope};

/// The §7 envelope, type-erased so one value can carry any command's result.
///
/// `Envelope<T>` stays generic for the `--json` writer, which is the output
/// contract; this trait exposes what a caller needs without naming `T`.
// `canvas mcp` reads every accessor; the CLI needs only the writer and the exit.
#[allow(dead_code)]
pub trait JsonEnvelope {
    /// Write the one JSON document `--json` prints.
    fn write_json(&self, w: &mut dyn Write) -> io::Result<()>;
    /// The same document as a value, for the MCP adapter.
    fn to_value(&self) -> serde_json::Value;
    fn schema(&self) -> &str;
    fn exit(&self) -> u8;
    fn outcome(&self) -> Outcome;
    fn freshness(&self) -> &[Freshness];
    fn partial(&self) -> &[PartialScope];
    fn warnings(&self) -> &[String];
    fn identity(&self) -> Option<&IdentityRef>;
}

impl<T: Serialize> JsonEnvelope for Rc<Envelope<T>> {
    fn write_json(&self, w: &mut dyn Write) -> io::Result<()> {
        Envelope::write_json(self, w)
    }

    fn to_value(&self) -> serde_json::Value {
        serde_json::to_value(&**self).unwrap_or(serde_json::Value::Null)
    }

    fn schema(&self) -> &str {
        &self.schema
    }

    fn exit(&self) -> u8 {
        self.exit
    }

    fn outcome(&self) -> Outcome {
        self.outcome
    }

    fn freshness(&self) -> &[Freshness] {
        &self.freshness
    }

    fn partial(&self) -> &[PartialScope] {
        &self.partial
    }

    fn warnings(&self) -> &[String] {
        &self.warnings
    }

    fn identity(&self) -> Option<&IdentityRef> {
        self.identity.as_ref()
    }
}

/// How the human (non-`--json`) form of a finished command is printed.
enum Human {
    /// Render from the envelope.
    Render(Box<dyn FnOnce() -> io::Result<()>>),
    /// Write these bytes to stdout verbatim (`calendar --ics -`).
    Raw(Vec<u8>),
    /// Print the error message on stderr.
    Message(String),
}

/// A finished command: the envelope, and how to print it for a person.
pub struct Handled {
    envelope: Box<dyn JsonEnvelope>,
    human: Human,
}

#[allow(dead_code)]
impl Handled {
    /// A completed command: its envelope and the renderer for human output.
    pub fn new<T, F>(envelope: Envelope<T>, human: F) -> Self
    where
        T: Serialize + 'static,
        F: FnOnce(&Envelope<T>) -> io::Result<()> + 'static,
    {
        let envelope = Rc::new(envelope);
        let rendered = Rc::clone(&envelope);
        Self {
            envelope: Box::new(envelope),
            human: Human::Render(Box::new(move || human(&rendered))),
        }
    }

    /// A completed command whose human output is these stdout bytes.
    ///
    /// Raw-output forms (§7) reject `--json` with exit 2 before they reach
    /// here, so the bytes are the whole answer.
    pub fn raw<T>(envelope: Envelope<T>, stdout: Vec<u8>) -> Self
    where
        T: Serialize + 'static,
    {
        Self {
            envelope: Box::new(Rc::new(envelope)),
            human: Human::Raw(stdout),
        }
    }

    /// An `error@1` envelope, printed as one stderr line for a person.
    pub fn error(
        code: &str,
        message: &str,
        exit: u8,
        profile: Option<String>,
        identity: Option<IdentityRef>,
    ) -> Self {
        let mut envelope = error_envelope(code, message, None, serde_json::json!({}), exit);
        envelope.profile = if identity.is_some() { profile } else { None };
        envelope.identity = identity;
        Self {
            envelope: Box::new(Rc::new(envelope)),
            human: Human::Message(message.to_owned()),
        }
    }

    /// An `error@1` envelope built elsewhere (status, details, telemetry).
    pub fn error_envelope<T: Serialize + 'static>(envelope: Envelope<T>, message: String) -> Self {
        Self {
            envelope: Box::new(Rc::new(envelope)),
            human: Human::Message(message),
        }
    }

    /// The §7 envelope this command produced.
    pub fn envelope(&self) -> &dyn JsonEnvelope {
        self.envelope.as_ref()
    }

    /// Process exit code (§14).
    pub fn exit(&self) -> u8 {
        self.envelope.exit()
    }

    /// Print the envelope (`--json`) or the human form, and return the exit.
    pub fn emit(self, json: bool) -> ExitCode {
        let Self { envelope, human } = self;
        if json && !matches!(human, Human::Raw(_)) {
            if let Err(e) = envelope.write_json(&mut io::stdout()) {
                let _ = writeln!(io::stderr(), "failed to write JSON: {e}");
                return ExitCode::from(1);
            }
            return ExitCode::from(envelope.exit());
        }
        match human {
            Human::Render(render) => {
                for warning in envelope.warnings() {
                    let _ = writeln!(io::stderr(), "warning: {warning}");
                }
                if let Err(e) = render() {
                    let _ = writeln!(io::stderr(), "{e}");
                    return ExitCode::from(1);
                }
            }
            Human::Raw(bytes) => {
                for warning in envelope.warnings() {
                    let _ = writeln!(io::stderr(), "warning: {warning}");
                }
                let mut out = io::stdout();
                if out.write_all(&bytes).and_then(|()| out.flush()).is_err() {
                    return ExitCode::from(1);
                }
            }
            Human::Message(message) => {
                let _ = writeln!(io::stderr(), "{message}");
            }
        }
        ExitCode::from(envelope.exit())
    }
}
