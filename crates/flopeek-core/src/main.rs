use flopeek_core::diagnostic;
use flopeek_core::model::DiagnosticLimits;
use flopeek_core::protocol::{scan_project, serve_jsonl, status_project};
use serde_json::to_writer_pretty;
use std::env;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

mod cli;

const CLI_SCHEMA: &str = "flopeek-cli/v1";

fn usage() -> &'static str {
    "Flopeek deterministic Rust repository evidence\n\nUsage:\n  flopeek scan [PATH]\n  flopeek status [PATH]\n  flopeek diagnose [PATH] CONTEXT_ID\n  flopeek packet [PATH] CONTEXT_ID\n  flopeek lkg <propose|review|confirm|reject|revoke|get|history> --context ID [options]\n  flopeek serve\n  flopeek --version\n  flopeek --help\n\nThe only repository-truth core is Rust. Evidence is persisted in SQLite under .flopeek/."
}

fn project_root(argument: Option<String>) -> Result<PathBuf, String> {
    let root = match argument {
        Some(argument) => PathBuf::from(argument),
        None => env::current_dir()
            .map_err(|error| format!("Unable to read current directory: {error}"))?,
    };
    let root = root
        .canonicalize()
        .map_err(|error| format!("Unable to resolve project root {}: {error}", root.display()))?;
    if !root.is_dir() {
        return Err(format!(
            "Project root is not a directory: {}",
            root.display()
        ));
    }
    Ok(root)
}

fn print_json(value: &serde_json::Value) -> Result<(), String> {
    let mut stdout = BufWriter::new(std::io::stdout().lock());
    to_writer_pretty(&mut stdout, value)
        .map_err(|error| format!("Unable to serialize response: {error}"))?;
    writeln!(stdout).map_err(|error| format!("Unable to flush response: {error}"))
}

fn run() -> Result<i32, String> {
    flopeek_core::contract::validate()?;
    flopeek_core::contract::validate_lkg_protocol()?;
    let mut arguments = env::args().skip(1);
    let command = arguments.next().unwrap_or_else(|| "--help".to_string());
    match command.as_str() {
        "--version" | "-V" => {
            println!("flopeek {}", env!("CARGO_PKG_VERSION"));
            Ok(0)
        }
        "--help" | "-h" | "help" => {
            println!("{}", usage());
            Ok(0)
        }
        "serve" => {
            if arguments.next().is_some() {
                return Err("serve does not accept positional arguments.".to_string());
            }
            let stdout = std::io::stdout();
            let writer = BufWriter::with_capacity(128 * 1024, stdout.lock());
            serve_jsonl(std::io::stdin().lock(), writer)?;
            Ok(0)
        }
        "scan" => {
            let root = project_root(arguments.next())?;
            if arguments.next().is_some() {
                return Err("scan accepts at most one project path.".to_string());
            }
            let result = scan_project(&root)?;
            print_json(&serde_json::to_value(result).map_err(|error| error.to_string())?)?;
            Ok(0)
        }
        "status" => {
            let root = project_root(arguments.next())?;
            if arguments.next().is_some() {
                return Err("status accepts at most one project path.".to_string());
            }
            let mut result = status_project(&root)?;
            if let Some(object) = result.as_object_mut() {
                object.insert("schemaVersion".to_string(), serde_json::json!(CLI_SCHEMA));
            }
            print_json(&result)?;
            Ok(0)
        }
        "diagnose" | "packet" => {
            let positional = arguments.collect::<Vec<_>>();
            let (path, context_id) = match positional.as_slice() {
                [context_id] => (None, context_id.clone()),
                [path, context_id] => (Some(path.clone()), context_id.clone()),
                _ => {
                    return Err(format!("{command} accepts CONTEXT_ID or PATH CONTEXT_ID."));
                }
            };
            let root = project_root(path)?;
            let result = if command == "diagnose" {
                serde_json::to_value(diagnostic::diagnose_history(
                    &root,
                    &context_id,
                    DiagnosticLimits::default(),
                )?)
                .map_err(|error| error.to_string())?
            } else {
                serde_json::to_value(diagnostic::build_packet(
                    &root,
                    &context_id,
                    DiagnosticLimits::default(),
                )?)
                .map_err(|error| error.to_string())?
            };
            print_json(&result)?;
            Ok(0)
        }
        "lkg" => cli::run_lkg(&arguments.collect::<Vec<_>>()),
        other => Err(format!("Unknown command {other:?}.\n\n{}", usage())),
    }
}

fn main() {
    match run() {
        Ok(status) => std::process::exit(status),
        Err(error) => {
            eprintln!("flopeek: {error}");
            std::process::exit(1);
        }
    }
}
