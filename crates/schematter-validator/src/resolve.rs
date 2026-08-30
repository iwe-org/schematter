use std::collections::HashMap;
use std::error::Error;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::compile::{SchemaError, DIALECT_V1};

pub type ResolveError = String;

pub trait Resolver: Send + Sync {
    fn resolve(&self, uri: &str) -> Result<Value, ResolveError>;
}

impl<F> Resolver for F
where
    F: Fn(&str) -> Result<Value, ResolveError> + Send + Sync,
{
    fn resolve(&self, uri: &str) -> Result<Value, ResolveError> {
        self(uri)
    }
}

#[derive(Default, Clone)]
pub struct CompileOptions {
    schemas: Vec<(String, Value)>,
    resolver: Option<Arc<dyn Resolver>>,
    base_uri: Option<String>,
}

impl CompileOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_schema(mut self, uri: impl Into<String>, schema: Value) -> Self {
        self.schemas.push((uri.into(), schema));
        self
    }

    pub fn with_schema_source(
        self,
        uri: impl Into<String>,
        source: &str,
    ) -> Result<Self, SchemaError> {
        let uri = uri.into();
        let parsed = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(source)
            .map_err(|error| parse_error(&uri, &error))
            .and_then(|value| {
                serde_json::to_value(value).map_err(|error| parse_error(&uri, &error))
            });
        Ok(self.with_schema(uri, parsed?))
    }

    pub fn with_resolver(mut self, resolver: impl Resolver + 'static) -> Self {
        self.resolver = Some(Arc::new(resolver));
        self
    }

    pub fn with_base_uri(mut self, base_uri: impl Into<String>) -> Self {
        self.base_uri = Some(base_uri.into());
        self
    }
}

fn parse_error(uri: &str, error: &dyn std::fmt::Display) -> SchemaError {
    SchemaError {
        pointer: String::new(),
        message: format!("cannot parse '{uri}': {error}"),
    }
}

pub(crate) fn is_document_schema(value: &Value) -> bool {
    value.get("$schema").and_then(Value::as_str) == Some(DIALECT_V1)
}

pub(crate) struct Fetcher {
    schemas: Vec<(String, Value)>,
    resolver: Option<Arc<dyn Resolver>>,
    base_uri: Option<String>,
    cache: Mutex<HashMap<String, Value>>,
}

impl Fetcher {
    pub(crate) fn new(options: &CompileOptions) -> Arc<Self> {
        Arc::new(Self {
            schemas: options.schemas.clone(),
            resolver: options.resolver.clone(),
            base_uri: options.base_uri.clone(),
            cache: Mutex::new(HashMap::new()),
        })
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.schemas.is_empty() && self.resolver.is_none()
    }

    pub(crate) fn base_uri(&self) -> Option<&str> {
        self.base_uri.as_deref()
    }

    pub(crate) fn schemas(&self) -> &[(String, Value)] {
        &self.schemas
    }

    pub(crate) fn registered(&self, uri: &str) -> Option<&Value> {
        self.schemas
            .iter()
            .rev()
            .find(|(key, _)| key == uri)
            .map(|(_, value)| value)
    }

    pub(crate) fn fetch(&self, uri: &str) -> Result<Value, ResolveError> {
        if let Some(value) = self.registered(uri) {
            return Ok(value.clone());
        }
        if let Some(value) = self.cache.lock().expect("resolver cache").get(uri) {
            return Ok(value.clone());
        }
        let Some(resolver) = &self.resolver else {
            return Err(
                "no schema is registered under that URI and no resolver is configured".to_string(),
            );
        };
        let value = resolver.resolve(uri)?;
        self.cache
            .lock()
            .expect("resolver cache")
            .insert(uri.to_string(), value.clone());
        Ok(value)
    }
}

#[derive(Clone)]
pub(crate) struct SchemaRetriever(pub(crate) Arc<Fetcher>);

impl jsonschema::Retrieve for SchemaRetriever {
    fn retrieve(
        &self,
        uri: &jsonschema::Uri<String>,
    ) -> Result<Value, Box<dyn Error + Send + Sync>> {
        let uri = uri.as_str();
        let value = self
            .0
            .fetch(uri)
            .map_err(|error| format!("cannot resolve '{uri}': {error}"))?;
        if is_document_schema(&value) {
            return Err(format!("'{uri}' is a document schema, not a JSON Schema").into());
        }
        Ok(value)
    }
}
