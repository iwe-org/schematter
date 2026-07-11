use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use schematter_lib::tokens::count_tokens;
use schematter_lib::{build_document, compile_schema, Violation};

/// Validate markdown documents against a document schema.
#[derive(Parser)]
#[command(name = "schematter", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Validate markdown documents against a schema file.
    Validate {
        /// Markdown files to validate (reads stdin when omitted).
        markdown: Vec<PathBuf>,

        /// Schema file (YAML).
        #[arg(short, long)]
        schema: PathBuf,

        /// Output format.
        #[arg(short, long, value_enum, default_value_t = Format::Text)]
        format: Format,

        /// Print the binding trace (which section/block bound to which schema
        /// entry) instead of validating.
        #[arg(long)]
        explain: bool,
    },
}

#[derive(Copy, Clone, ValueEnum)]
enum Format {
    Text,
    Json,
}

/// Exit codes mirror the spec: `0` clean, `1` violations, `2` schema/IO errors.
fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<ExitCode> {
    let Cli { command } = Cli::parse();
    match command {
        Command::Validate {
            markdown,
            schema,
            format,
            explain,
        } => validate(markdown, schema, format, explain),
    }
}

struct Report {
    key: String,
    violations: Vec<Violation>,
}

fn validate(
    markdown: Vec<PathBuf>,
    schema: PathBuf,
    format: Format,
    explain: bool,
) -> Result<ExitCode> {
    let schema_source = fs::read_to_string(&schema)
        .with_context(|| format!("reading schema {}", schema.display()))?;
    let schema_name = schema_name(&schema);

    let compiled = match compile_schema(&schema_source) {
        Ok(compiled) => compiled,
        Err(errors) => {
            for error in errors {
                if error.pointer.is_empty() {
                    eprintln!("schema '{schema_name}': {}", error.message);
                } else {
                    eprintln!(
                        "schema '{schema_name}' {}: {}",
                        error.pointer, error.message
                    );
                }
            }
            return Ok(ExitCode::from(2));
        }
    };

    let inputs = read_inputs(&markdown)?;

    if explain {
        for (key, source) in &inputs {
            let document = build_document(source, count_tokens);
            println!("{key}  [schema: {schema_name}]");
            print!("{}", compiled.explain(&document));
            println!();
        }
        return Ok(ExitCode::SUCCESS);
    }

    let mut reports = Vec::new();
    for (key, source) in &inputs {
        let document = build_document(source, count_tokens);
        let violations = compiled.validate(&document);
        if !violations.is_empty() {
            reports.push(Report {
                key: key.clone(),
                violations,
            });
        }
    }

    match format {
        Format::Text => print_text(&reports),
        Format::Json => print_json(&schema_name, &reports)?,
    }

    Ok(if reports.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

fn read_inputs(paths: &[PathBuf]) -> Result<Vec<(String, String)>> {
    if paths.is_empty() {
        let mut buffer = String::new();
        io::stdin()
            .read_to_string(&mut buffer)
            .context("reading markdown from stdin")?;
        return Ok(vec![("<stdin>".to_string(), buffer)]);
    }
    paths
        .iter()
        .map(|path| {
            let source =
                fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
            Ok((document_key(path), source))
        })
        .collect()
}

fn document_key(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("<stdin>")
        .to_string()
}

fn schema_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("schema")
        .to_string()
}

fn print_text(reports: &[Report]) {
    for report in reports {
        for violation in &report.violations {
            let breadcrumb = violation.breadcrumb_text();
            if breadcrumb.is_empty() {
                println!("{}: {}", report.key, violation.message);
            } else {
                println!("{} › {breadcrumb}: {}", report.key, violation.message);
            }
            if let Some(hint) = &violation.hint {
                println!("  hint: {hint}");
            }
        }
    }
}

fn print_json(schema_name: &str, reports: &[Report]) -> Result<()> {
    let reports: Vec<serde_json::Value> = reports
        .iter()
        .map(|report| {
            serde_json::json!({
                "key": report.key,
                "schema": schema_name,
                "violations": report.violations,
            })
        })
        .collect();
    println!("{}", serde_json::to_string_pretty(&reports)?);
    Ok(())
}
