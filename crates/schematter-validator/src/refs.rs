use std::borrow::Cow;
use std::collections::HashMap;

use serde_yaml_ng::Value;

use crate::compile::{SchemaError, DIALECT_V1};
use crate::resolve::Fetcher;

struct Scope {
    key: String,
    base: Option<String>,
}

struct Context<'a> {
    fetcher: &'a Fetcher,
    documents: HashMap<String, Value>,
    errors: Vec<SchemaError>,
}

impl Context<'_> {
    fn document(&mut self, key: &str, site: &str) -> Option<Value> {
        if let Some(document) = self.documents.get(key) {
            return Some(document.clone());
        }
        match self.fetcher.fetch(key) {
            Ok(value) => match serde_yaml_ng::to_value(value) {
                Ok(document) => {
                    self.remember(key, &document);
                    Some(document)
                }
                Err(error) => {
                    self.fail(site, format!("cannot resolve '{key}': {error}"));
                    None
                }
            },
            Err(error) => {
                self.fail(site, format!("cannot resolve '{key}': {error}"));
                None
            }
        }
    }

    fn remember(&mut self, key: &str, document: &Value) {
        self.documents.insert(key.to_string(), document.clone());
        if let Some(id) = document_base(document, Some(key)) {
            if id != key {
                self.documents.insert(id, document.clone());
            }
        }
    }

    fn fail(&mut self, pointer: &str, message: String) {
        self.errors.push(SchemaError {
            pointer: pointer.to_string(),
            message,
        });
    }
}

pub(crate) fn resolve_refs<'a>(
    source: &'a str,
    fetcher: &Fetcher,
) -> Result<Cow<'a, str>, Vec<SchemaError>> {
    let Ok(mut root) = serde_yaml_ng::from_str::<Value>(source) else {
        return Ok(Cow::Borrowed(source));
    };
    if !has_ref(&root) {
        return Ok(Cow::Borrowed(source));
    }

    let base = document_base(&root, fetcher.base_uri());
    let scope = Scope {
        key: base.clone().unwrap_or_default(),
        base,
    };

    let mut context = Context {
        fetcher,
        documents: HashMap::new(),
        errors: Vec::new(),
    };
    context.remember(&scope.key, &root);

    let mut stack = Vec::new();
    resolve_node(&mut root, &scope, "", &mut stack, &mut context);

    if !context.errors.is_empty() {
        return Err(context.errors);
    }

    serde_yaml_ng::to_string(&root)
        .map(Cow::Owned)
        .map_err(|error| {
            vec![SchemaError {
                pointer: String::new(),
                message: error.to_string(),
            }]
        })
}

fn has_ref(value: &Value) -> bool {
    match value {
        Value::Mapping(map) => map.iter().any(|(key, value)| {
            if key.as_str() == Some("$ref") {
                return true;
            }
            key.as_str() != Some("frontmatter") && has_ref(value)
        }),
        Value::Sequence(items) => items.iter().any(has_ref),
        _ => false,
    }
}

fn resolve_node(
    node: &mut Value,
    scope: &Scope,
    pointer: &str,
    stack: &mut Vec<String>,
    context: &mut Context,
) {
    if let Value::Sequence(items) = node {
        for (index, item) in items.iter_mut().enumerate() {
            resolve_node(item, scope, &format!("{pointer}/{index}"), stack, context);
        }
        return;
    }

    let Value::Mapping(map) = node else {
        return;
    };
    let mut map = std::mem::take(map);
    let reference = map.shift_remove("$ref");

    for (key, value) in map.iter_mut() {
        let Some(name) = key.as_str() else {
            continue;
        };
        if name == "frontmatter" {
            continue;
        }
        resolve_node(value, scope, &format!("{pointer}/{name}"), stack, context);
    }

    let Some(reference) = reference else {
        *node = Value::Mapping(map);
        return;
    };

    let site = format!("{pointer}/$ref");
    let Some(reference) = reference.as_str() else {
        context.fail(&site, "'$ref' must be a string".to_string());
        *node = Value::Mapping(map);
        return;
    };

    let Some(target) = resolve_reference(reference, scope, pointer, stack, context) else {
        *node = Value::Mapping(map);
        return;
    };

    match target {
        Value::Mapping(mut merged) => {
            for (key, value) in map {
                merged.insert(key, value);
            }
            *node = Value::Mapping(merged);
        }
        target => {
            if map.is_empty() {
                *node = target;
            } else {
                context.fail(
                    &site,
                    format!(
                        "'{reference}' does not resolve to a mapping, so it cannot be extended"
                    ),
                );
                *node = Value::Mapping(map);
            }
        }
    }
}

fn resolve_reference(
    reference: &str,
    scope: &Scope,
    pointer: &str,
    stack: &mut Vec<String>,
    context: &mut Context,
) -> Option<Value> {
    let site = format!("{pointer}/$ref");
    let site = site.as_str();
    let (target, fragment) = match reference.split_once('#') {
        Some((target, fragment)) => (target, fragment),
        None => (reference, ""),
    };
    let key = if target.is_empty() {
        scope.key.clone()
    } else {
        absolute_uri(target, scope.base.as_deref())
    };

    let visiting = format!("{key}#{fragment}");
    if let Some(start) = stack.iter().position(|entry| entry == &visiting) {
        let mut chain: Vec<&str> = stack[start..].iter().map(String::as_str).collect();
        chain.push(&visiting);
        context.fail(site, format!("reference cycle: {}", chain.join(" -> ")));
        return None;
    }

    let document = context.document(&key, site)?;

    if let Some(dialect) = document.get("$schema").and_then(Value::as_str) {
        if dialect != DIALECT_V1 {
            context.fail(site, format!("'{key}' is not a document schema"));
            return None;
        }
    }

    let mut node = match node_at(&document, &key, fragment) {
        Ok(node) => node.clone(),
        Err(message) => {
            context.fail(site, message);
            return None;
        }
    };

    let target_scope = if key == scope.key {
        Scope {
            key: scope.key.clone(),
            base: scope.base.clone(),
        }
    } else {
        Scope {
            base: document_base(&document, Some(&key)),
            key,
        }
    };

    stack.push(visiting);
    resolve_node(&mut node, &target_scope, pointer, stack, context);
    stack.pop();

    Some(node)
}

fn node_at<'a>(document: &'a Value, key: &str, fragment: &str) -> Result<&'a Value, String> {
    if fragment.is_empty() {
        return Ok(document);
    }
    if !fragment.starts_with('/') {
        return Err(format!("'{key}#{fragment}' is not a JSON pointer"));
    }
    let mut node = document;
    for token in fragment[1..].split('/') {
        let token = token.replace("~1", "/").replace("~0", "~");
        if token == "frontmatter" {
            return Err(format!("'{key}#{fragment}' is not a document schema"));
        }
        node = match node {
            Value::Mapping(map) => map.get(token.as_str()),
            Value::Sequence(items) => token.parse::<usize>().ok().and_then(|at| items.get(at)),
            _ => None,
        }
        .ok_or_else(|| format!("cannot resolve '{key}#{fragment}': no such node"))?;
    }
    Ok(node)
}

fn document_base(document: &Value, fallback: Option<&str>) -> Option<String> {
    match document.get("$id").and_then(Value::as_str) {
        Some(id) => Some(absolute_uri(id, fallback)),
        None => fallback.map(str::to_string),
    }
}

fn absolute_uri(target: &str, base: Option<&str>) -> String {
    if has_scheme(target) {
        return target.to_string();
    }
    let Some(base) = base.filter(|base| !base.is_empty()) else {
        return target.to_string();
    };
    let Ok(base) = jsonschema::uri::from_str(base) else {
        return target.to_string();
    };
    match jsonschema::uri::resolve_against(&base.borrow(), target) {
        Ok(uri) => uri.as_str().to_string(),
        Err(_) => target.to_string(),
    }
}

fn has_scheme(target: &str) -> bool {
    let mut chars = target.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() => {}
        _ => return false,
    }
    for char in chars {
        match char {
            ':' => return true,
            'a'..='z' | 'A'..='Z' | '0'..='9' | '+' | '-' | '.' => {}
            _ => return false,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use indoc::indoc;
    use serde_json::json;

    use crate::compile::{compile_schema, compile_schema_with, SchemaError};
    use crate::document::{Document, Section};
    use crate::resolve::CompileOptions;
    use crate::violation::Violation;

    const LIB_URI: &str = "https://example.com/lib.yaml";

    fn document(headers: &[&str]) -> Document {
        Document {
            frontmatter: json!({}),
            frontmatter_error: None,
            body_tokens: 10,
            blocks: vec![],
            sections: headers
                .iter()
                .map(|header| Section {
                    header: header.to_string(),
                    level: 1,
                    header_tokens: 1,
                    subtree_tokens: 1,
                    blocks: vec![],
                    sections: vec![],
                })
                .collect(),
        }
    }

    fn violations(source: &str, options: &CompileOptions, headers: &[&str]) -> Vec<Violation> {
        compile_schema_with(source, options)
            .expect("schema compiles")
            .validate(&document(headers))
    }

    fn errors(source: &str, options: &CompileOptions) -> Vec<SchemaError> {
        compile_schema_with(source, options)
            .err()
            .expect("schema fails to compile")
    }

    fn error(source: &str, options: &CompileOptions) -> SchemaError {
        let mut all = errors(source, options);
        assert_eq!(all.len(), 1, "expected exactly one error, got {all:?}");
        all.remove(0)
    }

    #[test]
    fn section_ref_applies_the_referenced_entry() {
        let options = CompileOptions::new().with_schema(
            LIB_URI,
            json!({ "sections": [{ "header": { "const": "Summary" } }] }),
        );
        let source = indoc! {"
            sections:
              - $ref: 'https://example.com/lib.yaml#/sections/0'
            additionalSections: false
        "};
        assert!(violations(source, &options, &["Summary"]).is_empty());
        let found = violations(source, &options, &["Notes"]);
        assert_eq!(found.len(), 2);
        assert!(found[0].message.contains("Summary"), "{found:?}");
    }

    #[test]
    fn top_level_ref_merges_and_local_keywords_win() {
        let options = CompileOptions::new().with_schema(
            LIB_URI,
            json!({ "maxTokens": 1, "sections": [{ "header": { "const": "Summary" } }] }),
        );
        let inherited = indoc! {"
            $ref: https://example.com/lib.yaml
        "};
        assert_eq!(violations(inherited, &options, &["Summary"]).len(), 1);

        let overridden = indoc! {"
            $ref: https://example.com/lib.yaml
            maxTokens: 100
        "};
        assert!(violations(overridden, &options, &["Summary"]).is_empty());
        assert_eq!(violations(overridden, &options, &["Notes"]).len(), 1);
    }

    #[test]
    fn local_defs_ref_needs_no_options() {
        let source = indoc! {"
            $defs:
              summary:
                header: { const: Summary }
            sections:
              - $ref: '#/$defs/summary'
        "};
        let compiled = compile_schema(source).expect("schema compiles");
        assert!(compiled.validate(&document(&["Summary"])).is_empty());
        assert_eq!(compiled.validate(&document(&["Notes"])).len(), 1);
    }

    #[test]
    fn refs_resolve_in_reduced_and_header_positions() {
        let source = indoc! {"
            $defs:
              short: { maxTokens: 3 }
              summary: { const: Summary }
            allSections:
              $ref: '#/$defs/short'
            sections:
              - header:
                  $ref: '#/$defs/summary'
        "};
        let compiled = compile_schema(source).expect("schema compiles");
        assert!(compiled.validate(&document(&["Summary"])).is_empty());
        assert_eq!(compiled.validate(&document(&["Notes"])).len(), 1);
    }

    #[test]
    fn relative_ref_resolves_against_the_documents_id() {
        let options = CompileOptions::new().with_schema(
            "https://example.com/sections.yaml",
            json!({ "header": { "const": "Summary" } }),
        );
        let source = indoc! {"
            $id: https://example.com/main.yaml
            sections:
              - $ref: sections.yaml
        "};
        assert!(violations(source, &options, &["Summary"]).is_empty());
        assert_eq!(violations(source, &options, &["Notes"]).len(), 1);
    }

    fn recording_resolver(
        answer: Result<serde_json::Value, String>,
    ) -> (CompileOptions, Arc<Mutex<Vec<String>>>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&calls);
        let options = CompileOptions::new().with_resolver(move |uri: &str| {
            recorded.lock().unwrap().push(uri.to_string());
            answer.clone()
        });
        (options, calls)
    }

    const SECTION_REF: &str = indoc! {"
        sections:
          - $ref: https://example.com/lib.yaml
    "};

    #[test]
    fn resolver_is_called_on_a_registry_miss() {
        let (options, calls) = recording_resolver(Ok(json!({ "header": { "const": "Summary" } })));
        assert!(violations(SECTION_REF, &options, &["Summary"]).is_empty());
        assert_eq!(*calls.lock().unwrap(), vec![LIB_URI.to_string()]);
    }

    #[test]
    fn registered_schema_shadows_the_resolver() {
        let (options, calls) = recording_resolver(Err("must not be called".to_string()));
        let options = options.with_schema(LIB_URI, json!({ "header": { "const": "Summary" } }));
        assert!(violations(SECTION_REF, &options, &["Summary"]).is_empty());
        assert!(calls.lock().unwrap().is_empty());
    }

    #[test]
    fn a_fetched_document_is_indexed_by_its_own_id() {
        let (options, calls) = recording_resolver(Ok(json!({
            "$id": "https://example.com/common.yaml",
            "$defs": {
                "summary": { "header": { "const": "Summary" } },
                "tasks": { "header": { "const": "Tasks" } },
            },
        })));
        let source = indoc! {"
            $id: https://example.com/main.yaml
            sections:
              - $ref: 'lib/common.yaml#/$defs/summary'
              - $ref: 'https://example.com/common.yaml#/$defs/tasks'
            additionalSections: false
        "};
        assert!(violations(source, &options, &["Summary", "Tasks"]).is_empty());
        assert_eq!(
            *calls.lock().unwrap(),
            vec!["https://example.com/lib/common.yaml".to_string()]
        );
    }

    #[test]
    fn resolver_failure_carries_the_referencing_site() {
        let (options, _) = recording_resolver(Err("network is down".to_string()));
        assert_eq!(
            error(SECTION_REF, &options),
            SchemaError {
                pointer: "/sections/0/$ref".to_string(),
                message: format!("cannot resolve '{LIB_URI}': network is down"),
            }
        );
    }

    #[test]
    fn unresolvable_ref_without_options_is_an_error() {
        let all = compile_schema(SECTION_REF).err().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].pointer, "/sections/0/$ref");
        assert!(all[0].message.contains(LIB_URI), "{}", all[0].message);
    }

    #[test]
    fn reference_cycles_are_reported() {
        let options = CompileOptions::new()
            .with_schema("https://example.com/a.yaml", json!({ "$ref": "b.yaml" }))
            .with_schema("https://example.com/b.yaml", json!({ "$ref": "a.yaml" }));
        let found = error("$ref: https://example.com/a.yaml\n", &options);
        assert_eq!(found.pointer, "/$ref");
        assert!(found.message.starts_with("reference cycle: "), "{found:?}");
        assert!(found.message.contains("a.yaml"), "{found:?}");
        assert!(found.message.contains("b.yaml"), "{found:?}");
    }

    #[test]
    fn frontmatter_refs_are_left_to_the_json_schema_layer() {
        let source = indoc! {"
            $defs:
              summary:
                header: { const: Summary }
            sections:
              - $ref: '#/$defs/summary'
            frontmatter:
              $ref: https://example.com/meta.json
        "};
        assert_eq!(
            error(source, &CompileOptions::new()),
            SchemaError {
                pointer: "/frontmatter".to_string(),
                message: "external references are not allowed".to_string(),
            }
        );
    }

    #[test]
    fn ref_to_a_json_schema_is_rejected() {
        let options = CompileOptions::new().with_schema(
            LIB_URI,
            json!({ "$schema": "https://json-schema.org/draft/2020-12/schema" }),
        );
        assert_eq!(
            error(SECTION_REF, &options),
            SchemaError {
                pointer: "/sections/0/$ref".to_string(),
                message: format!("'{LIB_URI}' is not a document schema"),
            }
        );
    }

    #[test]
    fn ref_into_a_frontmatter_node_is_rejected() {
        let options = CompileOptions::new().with_schema(
            LIB_URI,
            json!({ "frontmatter": { "type": "object" }, "sections": [] }),
        );
        let source = indoc! {"
            sections:
              - $ref: 'https://example.com/lib.yaml#/frontmatter'
        "};
        assert_eq!(
            error(source, &options),
            SchemaError {
                pointer: "/sections/0/$ref".to_string(),
                message: format!("'{LIB_URI}#/frontmatter' is not a document schema"),
            }
        );
    }

    #[test]
    fn a_missing_pointer_is_an_error() {
        let options = CompileOptions::new().with_schema(LIB_URI, json!({ "sections": [] }));
        let source = indoc! {"
            sections:
              - $ref: 'https://example.com/lib.yaml#/sections/3'
        "};
        assert_eq!(
            error(source, &options),
            SchemaError {
                pointer: "/sections/0/$ref".to_string(),
                message: format!("cannot resolve '{LIB_URI}#/sections/3': no such node"),
            }
        );
    }
}
