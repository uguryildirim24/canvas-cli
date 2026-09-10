//! `config path|get|set|edit` commands.

use serde_json::json;

use crate::cli::{ConfigCommand, Globals};
use crate::config::{self, Config};
use crate::exit::CliError;
use crate::output::{Envelope, print_human};
use crate::paths::CliPaths;

/// Dispatch config subcommands.
pub async fn run(globals: &Globals, command: ConfigCommand) -> Result<(), CliError> {
    let paths = CliPaths::resolve()?;
    match command {
        ConfigCommand::Path => {
            let path = paths.config_file();
            let result = json!({ "path": path });
            if globals.json {
                Envelope::new("canvas-cli/config@1", None, None)
                    .with_result(result)
                    .print_json();
            } else {
                println!("{}", path.display());
            }
            Ok(())
        }
        ConfigCommand::Get { key } => {
            let cfg = Config::load(&paths)?;
            let value = cfg.get_value(&key)?;
            let result = json!({ "key": key, "value": value });
            if globals.json {
                Envelope::new("canvas-cli/config@1", None, None)
                    .with_result(result)
                    .print_json();
            } else {
                match value {
                    serde_json::Value::String(s) => println!("{s}"),
                    other => println!("{other}"),
                }
            }
            Ok(())
        }
        ConfigCommand::Set { key, value } => {
            let mut cfg = Config::load(&paths)?;
            let previous = cfg.set_value(&key, &value)?;
            cfg.save(&paths)?;
            let result = json!({
                "key": key,
                "value": cfg.get_value(&key)?,
                "previous": previous,
            });
            if globals.json {
                Envelope::new("canvas-cli/config@1", None, None)
                    .with_result(result)
                    .print_json();
            } else {
                print_human([format!("set {key}")]);
            }
            Ok(())
        }
        ConfigCommand::Edit => {
            if globals.json {
                return Err(CliError::usage(
                    "--json cannot be used with config edit",
                ));
            }
            config::edit_config(&paths)
        }
    }
}
