use wasm_bindgen::prelude::*;

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

#[wasm_bindgen]
pub fn dialect() -> String {
    "https://document-schema.org/draft/2026-06/schema".to_string()
}
