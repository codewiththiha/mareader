//! Headless command-line client for `mareader-engine` with zero UI deps.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

mod driver;
mod host;
mod output;
mod ui_cmd;

use std::path::PathBuf;
use std::process::ExitCode;

use serde_json::{Value, json};

use crate::driver::Runner;

const USAGE: &str = "\
usage: mareader-cli [--state-dir <path>] <verb> [args...]

verbs:
  commands              print every registered command spec as JSON
  exec <id> [json]      execute one command id against the session
  run <script.jsonl>    replay a JSONL command script (`-` for stdin)
  inspect               print current session state as JSON
  render [opts...]      headlessly render a document page to PNG
  mcp [opts...]         run the stdio MCP server (headless or live UI)
  ui [opts...] <op>     control a running Mareader desktop window";

enum Verb {
    Commands,
    Exec { id: String, params: String },
    Run { script: PathBuf },
    Inspect,
    Render { rest: Vec<String> },
    Mcp { rest: Vec<String> },
    Ui { rest: Vec<String> },
}

struct Cli {
    state_dir: Option<PathBuf>,
    verb: Verb,
}

fn main() -> ExitCode {
    match run() {
        Ok(Some(value)) => {
            match serde_json::to_string_pretty(&value) {
                Ok(text) => println!("{text}"),
                Err(why) => {
                    eprintln!("mareader-cli: {why}");
                    return ExitCode::FAILURE;
                }
            }
            ExitCode::SUCCESS
        }
        Ok(None) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("mareader-cli: {why}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<Option<Value>, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cli = parse_args(&args)?;
    match cli.verb {
        Verb::Commands => Ok(Some(output::commands_json())),
        Verb::Inspect => {
            let runner = Runner::open(cli.state_dir);
            Ok(Some(output::inspect_json(&runner.session)))
        }
        Verb::Exec { id, params } => {
            let mut runner = Runner::open(cli.state_dir);
            runner.exec(&id, &params)?;
            runner.persist();
            Ok(Some(json!({
                "id": id,
                "events": output::events_json(&runner.events),
                "exports": output::exports_json(&runner.events),
                "word_lists": output::word_lists_json(&runner.events),
                "clipboard": runner.host.take_clipboard(),
                "state": output::inspect_json(&runner.session),
            })))
        }
        Verb::Run { script } => {
            let mut runner = Runner::open(cli.state_dir);
            let steps = runner.run_script(&script)?;
            runner.persist();
            Ok(Some(json!({
                "steps": steps,
                "events": output::events_json(&runner.events),
                "exports": output::exports_json(&runner.events),
                "word_lists": output::word_lists_json(&runner.events),
                "clipboard": runner.host.take_clipboard(),
                "state": output::inspect_json(&runner.session),
            })))
        }
        Verb::Render { rest } => {
            let refs: Vec<&str> = rest.iter().map(String::as_str).collect();
            ui_cmd::run_render(cli.state_dir, &refs).map(Some)
        }
        Verb::Mcp { rest } => {
            let refs: Vec<&str> = rest.iter().map(String::as_str).collect();
            ui_cmd::run_mcp(cli.state_dir, &refs)?;
            Ok(None)
        }
        Verb::Ui { rest } => {
            let refs: Vec<&str> = rest.iter().map(String::as_str).collect();
            ui_cmd::run_ui(&refs).map(Some)
        }
    }
}

fn parse_args(args: &[String]) -> Result<Cli, String> {
    let mut state_dir = std::env::var_os("MAREADER_STATE_DIR").map(PathBuf::from);
    let mut iter = args.iter();
    let verb = loop {
        let Some(arg) = iter.next() else {
            return Err(USAGE.to_owned());
        };
        if arg == "--state-dir" {
            let Some(dir) = iter.next() else {
                return Err("--state-dir requires a path".to_owned());
            };
            state_dir = Some(PathBuf::from(dir));
            continue;
        }
        if let Some(dir) = arg.strip_prefix("--state-dir=") {
            state_dir = Some(PathBuf::from(dir));
            continue;
        }
        if arg == "-h" || arg == "--help" {
            return Err(USAGE.to_owned());
        }
        break arg.as_str();
    };
    let rest: Vec<&str> = iter.map(String::as_str).collect();
    let verb = parse_verb(verb, &rest)?;
    Ok(Cli { state_dir, verb })
}

fn parse_verb(verb: &str, rest: &[&str]) -> Result<Verb, String> {
    match (verb, rest) {
        ("commands", []) => Ok(Verb::Commands),
        ("inspect", []) => Ok(Verb::Inspect),
        ("exec", [id]) => Ok(Verb::Exec {
            id: (*id).to_owned(),
            params: "{}".to_owned(),
        }),
        ("exec", [id, params]) => Ok(Verb::Exec {
            id: (*id).to_owned(),
            params: (*params).to_owned(),
        }),
        ("run", [script]) => Ok(Verb::Run {
            script: PathBuf::from(script),
        }),
        ("render", rest) => Ok(Verb::Render {
            rest: rest.iter().map(|s| (*s).to_owned()).collect(),
        }),
        ("mcp", rest) => Ok(Verb::Mcp {
            rest: rest.iter().map(|s| (*s).to_owned()).collect(),
        }),
        ("ui", rest) => Ok(Verb::Ui {
            rest: rest.iter().map(|s| (*s).to_owned()).collect(),
        }),
        _ => Err(USAGE.to_owned()),
    }
}
