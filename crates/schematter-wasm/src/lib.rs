use wasm_bindgen::prelude::*;

/// Validate a markdown `document` against a `schema` source.
///
/// Returns a JSON string. On success:
/// `{"ok":true,"violations":[{breadcrumb,message,hint,schemaPath,keyword}, ...]}`
/// (an empty list means the document conforms). When the schema itself does not
/// compile: `{"ok":false,"errors":[{pointer,message}, ...]}`.
#[wasm_bindgen]
pub fn validate(document: &str, schema: &str) -> String {
    match schematter_lib::validate(document, schema) {
        Ok(violations) => {
            let violations = serde_json::to_value(&violations)
                .unwrap_or_else(|_| serde_json::Value::Array(Vec::new()));
            serde_json::json!({ "ok": true, "violations": violations }).to_string()
        }
        Err(errors) => {
            let errors: Vec<serde_json::Value> = errors
                .iter()
                .map(|error| {
                    serde_json::json!({
                        "pointer": error.pointer,
                        "message": error.message,
                    })
                })
                .collect();
            serde_json::json!({ "ok": false, "errors": errors }).to_string()
        }
    }
}

/// The document-schema dialect this build validates against.
#[wasm_bindgen]
pub fn dialect() -> String {
    "https://document-schema.org/draft/2026-06/schema".to_string()
}
