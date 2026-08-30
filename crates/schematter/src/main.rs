use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use schematter_lib::tokens::count_tokens;
use schematter_lib::{build_document, compile_schema_with, CompileOptions, Violation};

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

        /// Register a schema `$ref` may resolve to, as `URI=FILE` (or as
        /// `FILE` alone when the file carries its own `$id`). Repeatable.
        #[arg(long = "ref", value_name = "URI=FILE", value_parser = parse_ref)]
        refs: Vec<Reference>,

        /// Read unregistered references from disk, resolved against the schema
        /// file's own location. Only `file:` and relative references.
        #[arg(long)]
        resolve_refs: bool,
    },
}

const BASE_SCHEME: &str = "schema";
const BASE_URI: &str = "schema:///";

#[derive(Clone)]
struct Reference {
    uri: Option<String>,
    path: PathBuf,
}

fn parse_ref(value: &str) -> Result<Reference, String> {
    match value.split_once('=') {
        Some((uri, path)) if !uri.is_empty() && !path.is_empty() => Ok(Reference {
            uri: Some(uri.to_string()),
            path: PathBuf::from(path),
        }),
        Some(_) => Err("expected URI=FILE".to_string()),
        None => Ok(Reference {
            uri: None,
            path: PathBuf::from(value),
        }),
    }
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
            refs,
            resolve_refs,
        } => validate(markdown, schema, format, explain, refs, resolve_refs),
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
    refs: Vec<Reference>,
    resolve_refs: bool,
) -> Result<ExitCode> {
    let schema_source = fs::read_to_string(&schema)
        .with_context(|| format!("reading schema {}", schema.display()))?;
    let schema_name = schema_name(&schema);
    let options = compile_options(&schema, &refs, resolve_refs)?;

    let compiled = match compile_schema_with(&schema_source, &options) {
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

fn compile_options(
    schema: &Path,
    refs: &[Reference],
    resolve_refs: bool,
) -> Result<CompileOptions> {
    let mut options = CompileOptions::new();

    for reference in refs {
        let value = read_schema(&reference.path)?;
        let uri = match &reference.uri {
            Some(uri) => uri.clone(),
            None => value
                .get("$id")
                .and_then(|id| id.as_str())
                .map(str::to_string)
                .ok_or_else(|| {
                    anyhow!(
                        "{} has no $id; register it as URI={}",
                        reference.path.display(),
                        reference.path.display()
                    )
                })?,
        };
        options = options.with_schema(uri, value);
    }

    if resolve_refs {
        let directory = schema
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .to_path_buf();
        options = options
            .with_base_uri(BASE_URI)
            .with_resolver(move |uri: &str| resolve_from_disk(uri, &directory));
    }

    Ok(options)
}

fn read_schema(path: &Path) -> Result<serde_json::Value> {
    let source =
        fs::read_to_string(path).with_context(|| format!("reading schema {}", path.display()))?;
    parse_schema(&source).with_context(|| format!("parsing schema {}", path.display()))
}

fn parse_schema(source: &str) -> Result<serde_json::Value> {
    let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(source)?;
    Ok(serde_json::to_value(value)?)
}

fn resolve_from_disk(uri: &str, directory: &Path) -> Result<serde_json::Value, String> {
    let path = match scheme(uri) {
        Some(BASE_SCHEME) => directory.join(decode(uri.strip_prefix(BASE_URI).unwrap_or(uri))),
        Some("file") => PathBuf::from(local_path(uri)),
        Some(scheme) => {
            return Err(format!(
            "'{scheme}:' references are not read from disk; only file and relative references are"
        ))
        }
        None => directory.join(uri),
    };
    let source =
        fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    parse_schema(&source).map_err(|error| format!("{}: {error}", path.display()))
}

fn scheme(uri: &str) -> Option<&str> {
    let end = uri.find(':')?;
    let scheme = &uri[..end];
    let mut chars = scheme.chars();
    if !chars.next()?.is_ascii_alphabetic() {
        return None;
    }
    chars
        .all(|char| char.is_ascii_alphanumeric() || matches!(char, '+' | '-' | '.'))
        .then_some(scheme)
}

fn strip_authority(rest: &str) -> &str {
    rest.strip_prefix("//").unwrap_or(rest)
}

fn decode(encoded: &str) -> String {
    let bytes = encoded.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' && at + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&encoded[at + 1..at + 3], 16) {
                out.push(byte);
                at += 3;
                continue;
            }
        }
        out.push(bytes[at]);
        at += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn local_path(uri: &str) -> String {
    let path = decode(strip_authority(&uri["file:".len()..]));
    match path.as_bytes() {
        [b'/', drive, b':', ..] if drive.is_ascii_alphabetic() => path[1..].to_string(),
        _ => path,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheme_is_read_only_from_a_valid_prefix() {
        assert_eq!(scheme("https://example.com/x.yaml"), Some("https"));
        assert_eq!(scheme("schema:///lib/x.yaml"), Some("schema"));
        assert_eq!(scheme("./x.yaml"), None);
        assert_eq!(scheme("lib/x.yaml"), None);
        assert_eq!(scheme("9lives:/x"), None);
    }

    #[test]
    fn file_uris_become_local_paths() {
        assert_eq!(
            local_path("file:///tmp/schemas/x.yaml"),
            "/tmp/schemas/x.yaml"
        );
        assert_eq!(local_path("file:///D:/schemas/x.yaml"), "D:/schemas/x.yaml");
        assert_eq!(
            local_path("file:///tmp/my%20schemas/x.yaml"),
            "/tmp/my schemas/x.yaml"
        );
    }
}
