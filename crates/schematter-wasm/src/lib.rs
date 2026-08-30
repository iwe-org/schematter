use schematter_lib::CompileOptions;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn validate(document: &str, schema: &str, refs: &str) -> String {
    let options = match compile_options(refs) {
        Ok(options) => options,
        Err(message) => return failure(&[(String::new(), message)]),
    };

    match schematter_lib::validate_with(document, schema, &options) {
        Ok(violations) => {
            let violations = serde_json::to_value(&violations)
                .unwrap_or_else(|_| serde_json::Value::Array(Vec::new()));
            serde_json::json!({ "ok": true, "violations": violations }).to_string()
        }
        Err(errors) => failure(
            &errors
                .iter()
                .map(|error| (error.pointer.clone(), error.message.clone()))
                .collect::<Vec<_>>(),
        ),
    }
}

#[wasm_bindgen]
pub fn dialect() -> String {
    "https://document-schema.org/draft/2026-06/schema".to_string()
}

fn compile_options(refs: &str) -> Result<CompileOptions, String> {
    let refs = refs.trim();
    if refs.is_empty() {
        return Ok(CompileOptions::new());
    }
    let value: serde_json::Value =
        serde_json::from_str(refs).map_err(|error| format!("cannot read refs: {error}"))?;
    let serde_json::Value::Object(map) = value else {
        return Err("refs must be a JSON object of URI to schema".to_string());
    };
    Ok(map
        .into_iter()
        .fold(CompileOptions::new(), |options, (uri, schema)| {
            options.with_schema(uri, schema)
        }))
}

fn failure(errors: &[(String, String)]) -> String {
    let errors: Vec<serde_json::Value> = errors
        .iter()
        .map(|(pointer, message)| {
            serde_json::json!({
                "pointer": pointer,
                "message": message,
            })
        })
        .collect();
    serde_json::json!({ "ok": false, "errors": errors }).to_string()
}
