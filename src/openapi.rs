//! OpenAPI (3.x, and the common parts of Swagger 2) as a load-test plan:
//! every operation becomes a concrete request, with parameters taken from
//! the spec's examples, defaults and enums, or from `--set`.

use serde_json::Value;

/// Most request variants one endpoint expands to when parameters have
/// several values
const MAX_VARIANTS: usize = 64;
/// How deep generated example bodies follow nested schemas
const MAX_DEPTH: usize = 6;
/// How far anyOf/oneOf/allOf and array items are followed. Schemas may
/// refer back to themselves; this is what ends that.
const MAX_NESTING: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum In {
    Path,
    Query,
    Header,
    Cookie,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub location: In,
    pub required: bool,
    /// "integer", "date", "string[]"
    pub kind: String,
    pub description: String,
    /// The values the spec allows, when it lists them
    pub options: Vec<String>,
    /// From the spec's example, default or enum; empty when it gives none
    pub value: Option<String>,
    /// Made up from the type, because the spec gives no value
    pub guess: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Operation {
    pub method: String,
    pub path: String,
    /// Its first tag; from the path when the spec doesn't tag it
    pub tag: String,
    pub summary: String,
    pub description: String,
    pub params: Vec<Param>,
    /// Content type and example body
    pub body: Option<(String, Vec<u8>)>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AuthKind {
    Bearer,
    Basic,
    ApiKey {
        name: String,
        location: In,
    },
    /// OAuth2, OpenID Connect: pepe needs the resulting token
    Token(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct AuthScheme {
    pub name: String,
    pub kind: AuthKind,
}

impl AuthScheme {
    /// What to pass to --auth for this scheme
    pub fn hint(&self) -> String {
        match &self.kind {
            AuthKind::Bearer | AuthKind::Token(_) => "--auth bearer:TOKEN".into(),
            AuthKind::Basic => "--auth basic:USER:PASSWORD".into(),
            AuthKind::ApiKey { .. } => "--auth apikey:VALUE".into(),
        }
    }

    pub fn describe(&self) -> String {
        match &self.kind {
            AuthKind::Bearer => "bearer token".into(),
            AuthKind::Basic => "basic auth".into(),
            AuthKind::ApiKey { name, location } => format!(
                "API key in {} {name}",
                match location {
                    In::Query => "query parameter",
                    In::Cookie => "cookie",
                    _ => "header",
                }
            ),
            AuthKind::Token(flow) => format!("{flow} token"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Spec {
    pub title: String,
    pub version: String,
    pub base_url: String,
    /// Schemes the API declares; a request needs at least one
    pub auth: Vec<AuthScheme>,
    /// Tags that have operations, with their descriptions: the spec's own
    /// order first, then as they come up
    pub tags: Vec<(String, String)>,
    pub operations: Vec<Operation>,
}

/// The text is the spec itself, not where to find it: a path or a URL
/// has no line breaks and doesn't open a JSON object
pub fn is_document(source: &str) -> bool {
    let text = source.trim();
    text.starts_with('{') || text.contains('\n')
}

/// Where a spec was read from, when that was a URL: a spec fetched over
/// http says where the API is when it names no server
pub fn origin(source: &str) -> Option<&str> {
    (source.starts_with("http://") || source.starts_with("https://")).then_some(source)
}

/// Read a spec from a file, an http(s) URL, or the document itself; JSON
/// or YAML
pub async fn load(source: &str) -> Result<Value, String> {
    let text = if is_document(source) {
        source.to_string()
    } else if origin(source).is_some() {
        let response = reqwest::Client::new()
            .get(source)
            .timeout(std::time::Duration::from_secs(20))
            .send()
            .await
            .map_err(|e| format!("can't fetch {source}: {e}"))?;
        if !response.status().is_success() {
            return Err(format!("can't fetch {source}: HTTP {}", response.status()));
        }
        response
            .text()
            .await
            .map_err(|e| format!("can't read {source}: {e}"))?
    } else {
        std::fs::read_to_string(source).map_err(|e| format!("can't read {source}: {e}"))?
    };
    parse_document(&text)
}

fn parse_document(text: &str) -> Result<Value, String> {
    if let Ok(doc) = serde_json::from_str::<Value>(text) {
        return Ok(doc);
    }
    serde_norway::from_str::<Value>(text).map_err(|e| format!("not valid JSON or YAML: {e}"))
}

/// Follow a `$ref` within the document (`#/components/...`)
fn resolve<'a>(doc: &'a Value, value: &'a Value) -> &'a Value {
    let mut current = value;
    for _ in 0..16 {
        let Some(pointer) = current.get("$ref").and_then(Value::as_str) else {
            break;
        };
        let Some(target) = pointer.strip_prefix('#').and_then(|p| doc.pointer(p)) else {
            break;
        };
        current = target;
    }
    current
}

fn text(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// A JSON value as it goes into a URL or header
fn plain(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(items) => items.iter().map(plain).collect::<Vec<_>>().join(","),
        other => other.to_string(),
    }
}

/// The value a schema itself suggests: example, default, const or first enum
fn schema_example(doc: &Value, schema: &Value, nesting: usize) -> Option<Value> {
    if nesting > MAX_NESTING {
        return None;
    }
    let schema = resolve(doc, schema);
    for key in ["example", "default", "const"] {
        if let Some(v) = schema.get(key).filter(|v| !v.is_null()) {
            return Some(v.clone());
        }
    }
    if let Some(v) = schema.get("examples").and_then(|e| e.get(0)) {
        return Some(v.clone());
    }
    if let Some(v) = schema.get("enum").and_then(|e| e.get(0)) {
        return Some(v.clone());
    }
    // A nullable or one-of-several type: use the first that suggests something
    for key in ["anyOf", "oneOf", "allOf"] {
        if let Some(found) = schema
            .get(key)
            .and_then(Value::as_array)
            .and_then(|options| {
                options
                    .iter()
                    .find_map(|o| schema_example(doc, o, nesting + 1))
            })
        {
            return Some(found);
        }
    }
    None
}

/// A value of the right shape for a schema, when the spec suggests none
fn generate(doc: &Value, schema: &Value, depth: usize) -> Value {
    if depth > MAX_DEPTH {
        return Value::Null;
    }
    let schema = resolve(doc, schema);
    if let Some(example) = schema_example(doc, schema, 0) {
        return example;
    }
    for key in ["anyOf", "oneOf"] {
        let first = schema
            .get(key)
            .and_then(Value::as_array)
            .and_then(|options| {
                options
                    .iter()
                    .find(|o| resolve(doc, o).get("type").and_then(Value::as_str) != Some("null"))
            });
        if let Some(option) = first {
            return generate(doc, option, depth + 1);
        }
    }
    // 3.1 allows a list of types: take the first that isn't null
    let kind = match schema.get("type") {
        Some(Value::String(t)) => t.as_str(),
        Some(Value::Array(types)) => types
            .iter()
            .filter_map(Value::as_str)
            .find(|t| *t != "null")
            .unwrap_or("string"),
        _ if schema.get("properties").is_some() => "object",
        _ => "string",
    };
    match kind {
        "integer" => Value::from(schema.get("minimum").and_then(Value::as_i64).unwrap_or(1)),
        "number" => Value::from(schema.get("minimum").and_then(Value::as_f64).unwrap_or(1.0)),
        "boolean" => Value::Bool(true),
        "array" if depth < MAX_DEPTH => Value::Array(
            schema
                .get("items")
                .map(|items| generate(doc, items, depth + 1))
                .into_iter()
                .collect(),
        ),
        "object" if depth < MAX_DEPTH => {
            let mut object = serde_json::Map::new();
            let merged = schema.get("allOf").and_then(Value::as_array);
            for part in std::iter::once(schema).chain(merged.into_iter().flatten()) {
                let part = resolve(doc, part);
                for (name, property) in part
                    .get("properties")
                    .and_then(Value::as_object)
                    .into_iter()
                    .flatten()
                {
                    object.insert(name.clone(), generate(doc, property, depth + 1));
                }
            }
            Value::Object(object)
        }
        "array" | "object" => Value::Null,
        _ => Value::from(match schema.get("format").and_then(Value::as_str) {
            Some("uuid") => "00000000-0000-4000-8000-000000000000",
            Some("date") => "2024-01-01",
            Some("date-time") => "2024-01-01T00:00:00Z",
            Some("email") => "user@example.com",
            Some("uri" | "url") => "https://example.com",
            _ => "string",
        }),
    }
}

/// A short name for a schema's type: "integer", "date", "string[]"
fn type_name(doc: &Value, schema: &Value, nesting: usize) -> String {
    if nesting > MAX_NESTING {
        return "object".into();
    }
    let schema = resolve(doc, schema);
    for key in ["anyOf", "oneOf", "allOf"] {
        let option = schema
            .get(key)
            .and_then(Value::as_array)
            .and_then(|options| {
                options
                    .iter()
                    .find(|o| resolve(doc, o).get("type").and_then(Value::as_str) != Some("null"))
            });
        if let Some(option) = option {
            return type_name(doc, option, nesting + 1);
        }
    }
    let kind = match schema.get("type") {
        Some(Value::String(t)) => t.as_str(),
        Some(Value::Array(types)) => types
            .iter()
            .filter_map(Value::as_str)
            .find(|t| *t != "null")
            .unwrap_or("string"),
        _ if schema.get("properties").is_some() => "object",
        _ => "string",
    };
    match kind {
        "array" => format!(
            "{}[]",
            schema.get("items").map_or("string".to_string(), |items| {
                type_name(doc, items, nesting + 1)
            })
        ),
        // A string's format says more than "string"
        "string" => match schema.get("format").and_then(Value::as_str) {
            Some(format) => format.to_string(),
            None => "string".to_string(),
        },
        other => other.to_string(),
    }
}

/// The values a schema allows, when it lists them
fn schema_options(doc: &Value, schema: &Value, nesting: usize) -> Vec<String> {
    if nesting > MAX_NESTING {
        return Vec::new();
    }
    let schema = resolve(doc, schema);
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        return values.iter().filter(|v| !v.is_null()).map(plain).collect();
    }
    for key in ["anyOf", "oneOf", "allOf"] {
        let found = schema
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|option| schema_options(doc, option, nesting + 1))
            .find(|options| !options.is_empty());
        if let Some(found) = found {
            return found;
        }
    }
    if let Some(items) = schema.get("items") {
        return schema_options(doc, items, nesting + 1);
    }
    if type_name(doc, schema, nesting) == "boolean" {
        return vec!["true".into(), "false".into()];
    }
    Vec::new()
}

/// The first line of a description
fn first_line(value: Option<&Value>) -> String {
    text(value)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .to_string()
}

/// Groups for operations the spec doesn't tag: the first part of the path
/// that they don't all share
fn path_groups(paths: &[&str]) -> Vec<String> {
    let segments: Vec<Vec<&str>> = paths
        .iter()
        .map(|path| {
            path.split('/')
                .filter(|s| !s.is_empty() && !s.starts_with('{'))
                .collect()
        })
        .collect();
    let mut shared = 0;
    while segments.len() > 1
        && segments
            .iter()
            .all(|s| s.len() > shared + 1 && s[shared] == segments[0][shared])
    {
        shared += 1;
    }
    segments
        .iter()
        .map(|s| s.get(shared).map_or("other".to_string(), |s| s.to_string()))
        .collect()
}

impl Spec {
    /// `origin`: where the spec came from, for specs with no (or a relative)
    /// server. `server`: use this base URL instead of the spec's.
    pub fn parse(doc: &Value, origin: Option<&str>, server: Option<&str>) -> Result<Spec, String> {
        let paths = doc
            .get("paths")
            .and_then(Value::as_object)
            .ok_or("not an OpenAPI document: it has no `paths`")?;
        let info = doc.get("info");

        let mut operations = Vec::new();
        for (path, item) in paths {
            let item = resolve(doc, item);
            let shared = item.get("parameters").and_then(Value::as_array);
            for method in ["get", "head", "options", "post", "put", "patch", "delete"] {
                let Some(operation) = item.get(method) else {
                    continue;
                };
                // Operation parameters replace path-level ones of the same name
                let own = operation.get("parameters").and_then(Value::as_array);
                let mut params: Vec<Param> = Vec::new();
                let mut body = None;
                for raw in shared
                    .into_iter()
                    .flatten()
                    .chain(own.into_iter().flatten())
                {
                    let raw = resolve(doc, raw);
                    let name = text(raw.get("name"));
                    let location = match raw.get("in").and_then(Value::as_str) {
                        Some("path") => In::Path,
                        Some("query") => In::Query,
                        Some("header") => In::Header,
                        Some("cookie") => In::Cookie,
                        // Swagger 2 puts the body among the parameters
                        Some("body") => {
                            let schema = raw.get("schema").unwrap_or(&Value::Null);
                            let example = generate(doc, schema, 0);
                            body = Some((
                                "application/json".to_string(),
                                example.to_string().into_bytes(),
                            ));
                            continue;
                        }
                        _ => continue,
                    };
                    // 3.x keeps the type under `schema`, Swagger 2 inline
                    let schema = raw.get("schema").unwrap_or(raw);
                    let value = raw
                        .get("example")
                        .cloned()
                        .or_else(|| {
                            let examples = raw.get("examples")?.as_object()?;
                            resolve(doc, examples.values().next()?)
                                .get("value")
                                .cloned()
                        })
                        .or_else(|| schema_example(doc, schema, 0))
                        .filter(|v| !v.is_null());
                    let param = Param {
                        name: name.clone(),
                        location,
                        required: location == In::Path
                            || raw
                                .get("required")
                                .and_then(Value::as_bool)
                                .unwrap_or(false),
                        kind: type_name(doc, schema, 0),
                        description: Some(first_line(raw.get("description")))
                            .filter(|d| !d.is_empty())
                            .unwrap_or_else(|| first_line(resolve(doc, schema).get("description"))),
                        options: schema_options(doc, schema, 0),
                        value: value.as_ref().map(plain),
                        guess: plain(&generate(doc, schema, 0)),
                    };
                    params.retain(|p| !(p.name == name && p.location == location));
                    params.push(param);
                }
                if let Some(request_body) = operation.get("requestBody") {
                    body = request_body_example(doc, resolve(doc, request_body)).or(body);
                }
                operations.push(Operation {
                    method: method.to_uppercase(),
                    path: path.clone(),
                    tag: text(operation.get("tags").and_then(|t| t.get(0))),
                    summary: text(operation.get("summary").or(operation.get("operationId"))),
                    description: first_line(operation.get("description")),
                    params,
                    body,
                });
            }
        }
        if operations.is_empty() {
            return Err("the spec has no operations".into());
        }

        // Untagged operations are grouped by path
        let untagged: Vec<usize> = (0..operations.len())
            .filter(|&i| operations[i].tag.is_empty())
            .collect();
        let paths: Vec<&str> = untagged
            .iter()
            .map(|&i| operations[i].path.as_str())
            .collect();
        for (index, group) in untagged.iter().zip(path_groups(&paths)) {
            operations[*index].tag = group;
        }
        let mut tags: Vec<(String, String)> = Vec::new();
        let declared = doc.get("tags").and_then(Value::as_array);
        for tag in declared.into_iter().flatten() {
            let name = text(tag.get("name"));
            if operations.iter().any(|op| op.tag == name) {
                tags.push((name, first_line(tag.get("description"))));
            }
        }
        for operation in &operations {
            if !tags.iter().any(|(name, _)| *name == operation.tag) {
                tags.push((operation.tag.clone(), String::new()));
            }
        }

        Ok(Spec {
            title: text(info.and_then(|i| i.get("title"))),
            version: text(info.and_then(|i| i.get("version"))),
            base_url: base_url(doc, origin, server)?,
            auth: auth_schemes(doc),
            tags,
            operations,
        })
    }
}

/// The first JSON example of a request body, or one generated from its schema
fn request_body_example(doc: &Value, body: &Value) -> Option<(String, Vec<u8>)> {
    let content = body.get("content")?.as_object()?;
    let (content_type, media) = content
        .iter()
        .find(|(t, _)| t.contains("json"))
        .or_else(|| content.iter().next())?;
    let example = media
        .get("example")
        .cloned()
        .or_else(|| {
            let examples = media.get("examples")?.as_object()?;
            resolve(doc, examples.values().next()?)
                .get("value")
                .cloned()
        })
        .unwrap_or_else(|| generate(doc, media.get("schema").unwrap_or(&Value::Null), 0));
    let bytes = if content_type.contains("json") {
        example.to_string().into_bytes()
    } else if content_type.contains("x-www-form-urlencoded") {
        example
            .as_object()
            .map(|fields| {
                fields
                    .iter()
                    .map(|(k, v)| format!("{}={}", encode(k), encode(&plain(v))))
                    .collect::<Vec<_>>()
                    .join("&")
            })
            .unwrap_or_default()
            .into_bytes()
    } else {
        plain(&example).into_bytes()
    };
    Some((content_type.clone(), bytes))
}

fn base_url(doc: &Value, origin: Option<&str>, server: Option<&str>) -> Result<String, String> {
    let origin = origin.and_then(|o| reqwest::Url::parse(o).ok());
    let found = if let Some(server) = server {
        server.to_string()
    } else if let Some(first) = doc.get("servers").and_then(|s| s.get(0)) {
        // Fill `{variables}` with their defaults
        let mut url = text(first.get("url"));
        for (name, variable) in first
            .get("variables")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            url = url.replace(
                &format!("{{{name}}}"),
                &plain(variable.get("default").unwrap_or(&Value::Null)),
            );
        }
        url
    } else if let Some(host) = doc.get("host").and_then(Value::as_str) {
        // Swagger 2
        let scheme = doc
            .get("schemes")
            .and_then(|s| s.get(0))
            .and_then(Value::as_str)
            .unwrap_or("https");
        format!("{scheme}://{host}{}", text(doc.get("basePath")))
    } else {
        String::new()
    };

    // No server, or a relative one: it's where the spec was served from
    let absolute = if found.contains("://") {
        found
    } else if let Some(origin) = &origin {
        origin
            .join(if found.is_empty() { "/" } else { &found })
            .map_err(|e| format!("server URL {found:?}: {e}"))?
            .to_string()
    } else {
        return Err("the spec names no server; pass --server https://…".into());
    };
    Ok(absolute.trim_end_matches('/').to_string())
}

fn auth_schemes(doc: &Value) -> Vec<AuthScheme> {
    let schemes = doc
        .pointer("/components/securitySchemes")
        .or_else(|| doc.get("securityDefinitions"))
        .and_then(Value::as_object);
    let mut out = Vec::new();
    for (name, scheme) in schemes.into_iter().flatten() {
        let scheme = resolve(doc, scheme);
        let kind = match (
            scheme.get("type").and_then(Value::as_str),
            scheme
                .get("scheme")
                .and_then(Value::as_str)
                .map(str::to_lowercase)
                .as_deref(),
        ) {
            (Some("http"), Some("basic")) | (Some("basic"), _) => AuthKind::Basic,
            (Some("http"), _) => AuthKind::Bearer,
            (Some("apiKey"), _) => AuthKind::ApiKey {
                name: text(scheme.get("name")),
                location: match scheme.get("in").and_then(Value::as_str) {
                    Some("query") => In::Query,
                    Some("cookie") => In::Cookie,
                    _ => In::Header,
                },
            },
            (Some(other), _) => AuthKind::Token(other.to_string()),
            (None, _) => continue,
        };
        out.push(AuthScheme {
            name: name.clone(),
            kind,
        });
    }
    out
}

// ─── Credentials ─────────────────────────────────────────────────────────────

/// What stands in for a credential's value wherever one would be shown
pub const MASK: &str = "REDACTED";

/// Credentials from `--auth`, placed where the API expects them
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Credentials {
    /// Sent with every request
    pub headers: Vec<(String, String)>,
    /// Added to every URL
    pub query: Vec<(String, String)>,
}

impl Credentials {
    /// Parse `--auth` values: `bearer:TOKEN`, `basic:USER:PASSWORD`,
    /// `apikey:VALUE` or `apikey:NAME=VALUE`, `header:NAME=VALUE`,
    /// `query:NAME=VALUE`
    pub fn parse(args: &[String], spec: &Spec) -> Result<Credentials, String> {
        let mut out = Credentials::default();
        for arg in args {
            let (kind, rest) = arg
                .split_once(':')
                .ok_or_else(|| format!("--auth {arg}: expected KIND:VALUE, e.g. bearer:TOKEN"))?;
            match kind.to_lowercase().as_str() {
                "bearer" => out.headers.push(("Authorization".into(), format!("Bearer {rest}"))),
                "basic" => out
                    .headers
                    .push(("Authorization".into(), format!("Basic {}", crate::curl::base64(rest.as_bytes())))),
                "header" | "query" => {
                    let (name, value) = rest
                        .split_once('=')
                        .ok_or_else(|| format!("--auth {arg}: expected {kind}:NAME=VALUE"))?;
                    let pair = (name.to_string(), value.to_string());
                    if kind.eq_ignore_ascii_case("query") {
                        out.query.push(pair);
                    } else {
                        out.headers.push(pair);
                    }
                }
                "apikey" => {
                    // The spec says where the key goes; NAME=VALUE overrides its name
                    let declared = spec.auth.iter().find_map(|s| match &s.kind {
                        AuthKind::ApiKey { name, location } => Some((name.clone(), *location)),
                        _ => None,
                    });
                    // A key may contain `=` itself (base64 padding), so
                    // only a spec without an API key reads NAME=VALUE
                    let (name, value, location) = match declared {
                        Some((name, location)) => {
                            let named = format!("{name}=");
                            let value = match rest.get(..named.len()) {
                                Some(start) if start.eq_ignore_ascii_case(&named) => {
                                    &rest[named.len()..]
                                }
                                _ => rest,
                            };
                            (name, value, location)
                        }
                        None => {
                            let (name, value) = rest.split_once('=').ok_or_else(|| {
                                format!(
                                    "--auth {arg}: the spec declares no API key; use apikey:NAME=VALUE"
                                )
                            })?;
                            (name.to_string(), value, In::Header)
                        }
                    };
                    match location {
                        In::Query => out.query.push((name, value.to_string())),
                        In::Cookie => out.headers.push(("Cookie".into(), format!("{name}={value}"))),
                        _ => out.headers.push((name, value.to_string())),
                    }
                }
                other => {
                    return Err(format!(
                        "--auth {arg}: unknown kind {other:?}; use bearer, basic, apikey, header or query"
                    ))
                }
            }
        }
        // Said here, rather than when the run starts
        for (name, value) in &out.headers {
            if reqwest::header::HeaderName::from_bytes(name.as_bytes()).is_err() {
                return Err(format!("--auth: {name:?} can't be a header name"));
            }
            if reqwest::header::HeaderValue::from_str(value).is_err() {
                return Err(format!(
                    "--auth: the value for {name} has characters a header can't carry"
                ));
            }
        }
        Ok(out)
    }

    pub fn is_empty(&self) -> bool {
        self.headers.is_empty() && self.query.is_empty()
    }

    /// The same credentials with every value replaced by `MASK`, for
    /// showing where they go without showing them
    pub fn masked(&self) -> Credentials {
        let mask = |pairs: &[(String, String)]| {
            pairs
                .iter()
                .map(|(name, _)| (name.clone(), MASK.to_string()))
                .collect()
        };
        Credentials {
            headers: mask(&self.headers),
            query: mask(&self.query),
        }
    }
}

// ─── Plan ────────────────────────────────────────────────────────────────────

/// What narrows and fills the plan, from flags
#[derive(Debug, Clone, Default)]
pub struct PlanOptions {
    /// Keep only endpoints matching one of these ("GET /pets*", "/pets/*")
    pub only: Vec<String>,
    /// Keep only endpoints with one of these tags
    pub tags: Vec<String>,
    pub skip: Vec<String>,
    /// Parameter values: name, then one or more values to rotate through
    pub set: Vec<(String, Vec<String>)>,
    /// Switch every endpoint of the plan on
    pub all: bool,
    /// Let POST, PUT, PATCH and DELETE be switched on by the flags
    pub include_writes: bool,
}

impl PlanOptions {
    /// Parse `--set name=v1,v2`
    pub fn parse_set(args: &[String]) -> Result<Vec<(String, Vec<String>)>, String> {
        args.iter()
            .map(|arg| {
                let (name, values) = arg
                    .split_once('=')
                    .ok_or_else(|| format!("--set {arg}: expected NAME=VALUE[,VALUE…]"))?;
                Ok((name.to_string(), split_values(values)))
            })
            .collect()
    }

    /// The flags name endpoints, which switches those on. Without them
    /// nothing is on until it's picked.
    fn selects(&self) -> bool {
        self.all || !self.only.is_empty() || !self.tags.is_empty()
    }
}

/// "a, b,c" as its values
pub fn split_values(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .collect()
}

/// One parameter of an endpoint, and what's sent for it
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub name: String,
    pub location: In,
    pub required: bool,
    pub kind: String,
    pub description: String,
    pub options: Vec<String>,
    /// The spec's own example or default
    pub suggestion: Option<String>,
    /// What's sent. Several values of a list parameter go together in each
    /// request; of any other they're rotated through. None: left out.
    pub values: Vec<String>,
    /// The value is made up from the type, because the spec gives none
    pub guessed: bool,
}

impl Field {
    /// An array parameter: its values are sent together
    pub fn is_list(&self) -> bool {
        self.kind.ends_with("[]")
    }

    /// Required, and without a real value
    pub fn is_missing(&self) -> bool {
        self.required && (self.guessed || self.values.is_empty())
    }

    pub fn set(&mut self, values: Vec<String>) {
        self.values = values;
        self.guessed = false;
    }
}

/// One operation and the requests it becomes
#[derive(Debug, Clone, PartialEq)]
pub struct Endpoint {
    /// "GET /pets/{id}"
    pub label: String,
    pub method: String,
    pub path: String,
    pub tag: String,
    pub summary: String,
    pub description: String,
    pub fields: Vec<Field>,
    /// Of the body the spec describes, when it describes one
    pub content_type: Option<String>,
    pub body: Option<Vec<u8>>,
    pub enabled: bool,
    pub weight: u32,
}

impl Endpoint {
    pub fn is_write(&self) -> bool {
        !matches!(self.method.as_str(), "GET" | "HEAD" | "OPTIONS")
    }

    /// Required parameters that have no real value yet
    pub fn missing(&self) -> Vec<&str> {
        self.fields
            .iter()
            .filter(|f| f.is_missing())
            .map(|f| f.name.as_str())
            .collect()
    }

    /// One URL per combination of the values that rotate, up to a limit
    pub fn urls(&self, base_url: &str, credentials: &Credentials) -> Vec<String> {
        let mut urls = vec![(format!("{base_url}{}", self.path), Vec::<String>::new())];
        for field in &self.fields {
            if field.values.is_empty() {
                continue;
            }
            match field.location {
                In::Path => {
                    let values = if field.is_list() {
                        vec![field.values.join(",")]
                    } else {
                        field.values.clone()
                    };
                    urls = urls
                        .iter()
                        .flat_map(|(url, query)| {
                            values.iter().map(move |v| {
                                (
                                    url.replace(&format!("{{{}}}", field.name), &encode(v)),
                                    query.clone(),
                                )
                            })
                        })
                        .take(MAX_VARIANTS)
                        .collect();
                }
                In::Query if field.is_list() => {
                    for (_, query) in &mut urls {
                        query.extend(
                            field
                                .values
                                .iter()
                                .map(|v| format!("{}={}", encode(&field.name), encode(v))),
                        );
                    }
                }
                In::Query => {
                    urls = urls
                        .iter()
                        .flat_map(|(url, query)| {
                            field.values.iter().map(move |v| {
                                let mut query = query.clone();
                                query.push(format!("{}={}", encode(&field.name), encode(v)));
                                (url.clone(), query)
                            })
                        })
                        .take(MAX_VARIANTS)
                        .collect();
                }
                In::Header | In::Cookie => {}
            }
        }
        urls.into_iter()
            .map(|(url, mut query)| {
                query.extend(
                    credentials
                        .query
                        .iter()
                        .map(|(k, v)| format!("{}={}", encode(k), encode(v))),
                );
                if query.is_empty() {
                    url
                } else {
                    format!("{url}?{}", query.join("&"))
                }
            })
            .collect()
    }

    /// Headers for this endpoint only: its parameters and content type
    pub fn headers(&self) -> Vec<(String, String)> {
        let mut headers = Vec::new();
        let mut cookies = Vec::new();
        for field in self.fields.iter().filter(|f| !f.values.is_empty()) {
            let value = if field.is_list() {
                field.values.join(",")
            } else {
                field.values[0].clone()
            };
            match field.location {
                In::Header => headers.push((field.name.clone(), value)),
                In::Cookie => cookies.push(format!("{}={value}", field.name)),
                In::Path | In::Query => {}
            }
        }
        if !cookies.is_empty() {
            headers.push(("Cookie".into(), cookies.join("; ")));
        }
        if let (Some(content_type), Some(_)) = (&self.content_type, &self.body) {
            headers.push(("Content-Type".into(), content_type.clone()));
        }
        headers
    }
}

/// The operations of the spec that the flags leave in, in the spec's
/// order. They're on only when the flags name them (--all, --tag, --only).
pub fn plan(spec: &Spec, options: &PlanOptions) -> Vec<Endpoint> {
    spec.operations
        .iter()
        .filter(|op| {
            let label = format!("{} {}", op.method, op.path);
            let named = |patterns: &[String]| patterns.iter().any(|p| matches(p, &label, &op.path));
            let tagged = options
                .tags
                .iter()
                .any(|t| wildcard(&t.to_lowercase(), &op.tag.to_lowercase()));
            (options.only.is_empty() || named(&options.only))
                && (options.tags.is_empty() || tagged)
                && !named(&options.skip)
        })
        .map(|op| endpoint(op, options))
        .collect()
}

fn endpoint(op: &Operation, options: &PlanOptions) -> Endpoint {
    let fields = op
        .params
        .iter()
        .map(|param| {
            let set = options
                .set
                .iter()
                .find(|(name, _)| *name == param.name)
                .map(|(_, values)| values.clone());
            let (values, guessed) = match (set, &param.value, param.required) {
                (Some(values), _, _) => (values, false),
                (None, Some(value), true) => (vec![value.clone()], false),
                (None, None, true) => (vec![param.guess.clone()], true),
                // Optional parameters are left out until they're set
                (None, _, false) => (Vec::new(), false),
            };
            Field {
                name: param.name.clone(),
                location: param.location,
                required: param.required,
                kind: param.kind.clone(),
                description: param.description.clone(),
                options: param.options.clone(),
                suggestion: param.value.clone(),
                values,
                guessed,
            }
        })
        .collect();
    let mut endpoint = Endpoint {
        label: format!("{} {}", op.method, op.path),
        method: op.method.clone(),
        path: op.path.clone(),
        tag: op.tag.clone(),
        summary: op.summary.clone(),
        description: op.description.clone(),
        fields,
        content_type: op
            .body
            .as_ref()
            .map(|(content_type, _)| content_type.clone()),
        body: op.body.as_ref().map(|(_, bytes)| bytes.clone()),
        enabled: false,
        weight: 1,
    };
    endpoint.enabled = options.selects()
        && endpoint.missing().is_empty()
        && (!endpoint.is_write() || options.include_writes);
    endpoint
}

/// `*` wildcards, case-insensitive; a pattern without a method matches the
/// path alone
fn matches(pattern: &str, label: &str, path: &str) -> bool {
    let subject = if pattern.contains(' ') { label } else { path };
    wildcard(&pattern.to_lowercase(), &subject.to_lowercase())
}

fn wildcard(pattern: &str, text: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == text,
        Some((head, rest)) => text.strip_prefix(head).is_some_and(|tail| {
            (0..=tail.len())
                .filter(|&i| tail.is_char_boundary(i))
                .any(|i| wildcard(rest, &tail[i..]))
        }),
    }
}

/// Percent-encode a path segment or query value
fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for &b in value.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const PETSTORE: &str = r##"
openapi: 3.1.0
info: { title: Petstore, version: 1.2.0 }
servers:
  - url: "{scheme}://api.example.com/v1/"
    variables: { scheme: { default: https } }
security: [ { ApiKeyAuth: [] } ]
components:
  securitySchemes:
    ApiKeyAuth: { type: apiKey, in: header, name: X-API-Key }
    Bearer: { type: http, scheme: bearer }
  parameters:
    Limit:
      name: limit
      in: query
      required: true
      schema: { type: integer, default: 20 }
  schemas:
    Pet:
      type: object
      properties:
        name: { type: string, example: Rex }
        age: { type: [integer, "null"] }
        tags: { type: array, items: { type: string, enum: [good, bad] } }
        owner: { $ref: "#/components/schemas/Owner" }
    Owner:
      type: object
      properties:
        email: { type: string, format: email }
paths:
  /pets:
    get:
      summary: List pets
      parameters:
        - $ref: "#/components/parameters/Limit"
        - { name: status, in: query, schema: { type: string, enum: [available, sold] } }
    post:
      operationId: createPet
      requestBody:
        content:
          application/json:
            schema: { $ref: "#/components/schemas/Pet" }
  /pets/{id}:
    parameters:
      - { name: id, in: path, required: true, schema: { type: string, format: uuid } }
    get:
      parameters:
        - { name: X-Trace, in: header, required: true, example: t1 }
    delete: {}
  /orders/{orderId}:
    get:
      parameters:
        - { name: orderId, in: path, required: true, schema: { type: integer }, example: 42 }
"##;

    fn petstore() -> Spec {
        Spec::parse(&parse_document(PETSTORE).unwrap(), None, None).unwrap()
    }

    fn find<'a>(plan: &'a [Endpoint], label: &str) -> &'a Endpoint {
        plan.iter()
            .find(|e| e.label == label)
            .unwrap_or_else(|| panic!("{label} not in plan"))
    }

    #[test]
    fn reads_yaml_servers_auth_and_operations() {
        let spec = petstore();
        assert_eq!(
            (spec.title.as_str(), spec.version.as_str()),
            ("Petstore", "1.2.0")
        );
        assert_eq!(
            spec.base_url, "https://api.example.com/v1",
            "variables filled, slash trimmed"
        );
        assert_eq!(spec.operations.len(), 5);
        assert_eq!(spec.auth.len(), 2);
        assert!(spec.auth.iter().any(|a| a.kind
            == AuthKind::ApiKey {
                name: "X-API-Key".into(),
                location: In::Header
            }));
        assert!(spec.auth.iter().any(|a| a.kind == AuthKind::Bearer));
    }

    #[test]
    fn plans_requests_from_examples_and_defaults() {
        let spec = petstore();
        let none = Credentials::default();
        let urls = |e: &Endpoint| e.urls(&spec.base_url, &none);
        let plan = plan(&spec, &PlanOptions::default());
        assert!(
            plan.iter().all(|e| !e.enabled),
            "nothing is on until it's picked"
        );

        // A $ref'd required query parameter with a default; the optional one is left out
        let list = find(&plan, "GET /pets");
        assert_eq!(urls(list), ["https://api.example.com/v1/pets?limit=20"]);
        assert!(list.missing().is_empty());
        assert_eq!(list.summary, "List pets");
        let status = &list.fields[1];
        assert_eq!(
            (status.name.as_str(), status.kind.as_str(), status.required),
            ("status", "string", false)
        );
        assert_eq!(status.options, ["available", "sold"]);
        assert!(status.values.is_empty());
        assert_eq!(list.fields[0].kind, "integer");

        // Path parameter from its example; required header parameter sent
        let order = find(&plan, "GET /orders/{orderId}");
        assert_eq!(urls(order), ["https://api.example.com/v1/orders/42"]);
        let pet = find(&plan, "GET /pets/{id}");
        assert_eq!(pet.headers(), [("X-Trace".to_string(), "t1".to_string())]);

        // No value in the spec: flagged, with a guess of the right shape
        assert_eq!(pet.missing(), ["id"]);
        assert_eq!(pet.fields[0].kind, "uuid");
        assert_eq!(
            urls(pet),
            ["https://api.example.com/v1/pets/00000000-0000-4000-8000-000000000000"]
        );

        // The body is generated from the schema, following $refs
        let create = find(&plan, "POST /pets");
        assert!(create.is_write());
        let body: Value = serde_json::from_slice(create.body.as_ref().unwrap()).unwrap();
        assert_eq!(
            body,
            serde_json::json!({"name": "Rex", "age": 1, "tags": ["good"], "owner": {"email": "user@example.com"}})
        );
        assert!(create
            .headers()
            .contains(&("Content-Type".into(), "application/json".into())));
    }

    #[test]
    fn operations_are_grouped_by_tag() {
        let doc: Value = serde_json::json!({
            "openapi": "3.0.0", "servers": [{"url": "https://api.demo.io"}],
            "tags": [{"name": "Billing", "description": "Invoices.\nAnd more."}, {"name": "Unused"}, {"name": "Users"}],
            "paths": {
                "/api/users": {"get": {"tags": ["Users", "Admin"]}},
                "/api/invoices": {"get": {"tags": ["Billing"]}},
                "/api/health": {"get": {}},
                "/api/reports/{id}": {"get": {"parameters": [{"name": "id", "in": "path", "required": true}]}},
                "/api/extra": {"get": {"tags": ["Extra"]}}
            }
        });
        let spec = Spec::parse(&doc, None, None).unwrap();
        // The spec's order, without tags nothing uses; then tags it doesn't
        // declare, and untagged operations by the part of the path that differs
        let names: Vec<&str> = spec.tags.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["Billing", "Users", "health", "reports", "Extra"]);
        assert_eq!(spec.tags[0].1, "Invoices.");
        let tag_of = |path: &str| {
            spec.operations
                .iter()
                .find(|op| op.path == path)
                .unwrap()
                .tag
                .clone()
        };
        assert_eq!(tag_of("/api/users"), "Users");
        assert_eq!(tag_of("/api/reports/{id}"), "reports");

        // --tag keeps a tag's endpoints and switches them on
        let options = PlanOptions {
            tags: vec!["billing".into(), "rep*".into()],
            ..Default::default()
        };
        let plan = plan(&spec, &options);
        let labels: Vec<(&str, bool)> =
            plan.iter().map(|e| (e.label.as_str(), e.enabled)).collect();
        assert_eq!(
            labels,
            [
                ("GET /api/invoices", true),
                ("GET /api/reports/{id}", false)
            ],
            "the one that needs a value stays off"
        );
    }

    #[test]
    fn list_parameters_go_together_and_others_rotate() {
        let doc: Value = serde_json::json!({
            "openapi": "3.0.0", "servers": [{"url": "https://api.demo.io"}],
            "paths": {"/ads": {"get": {"parameters": [
                {"name": "ids", "in": "query", "required": true, "schema": {"type": "array", "items": {"type": "string"}}},
                {"name": "live", "in": "query", "schema": {"anyOf": [{"type": "boolean"}, {"type": "null"}]}},
                {"name": "order", "in": "query", "schema": {"anyOf": [{"type": "string", "enum": ["newest", "oldest"]}, {"type": "null"}], "default": "newest"}}
            ]}}}
        });
        let spec = Spec::parse(&doc, None, None).unwrap();
        let mut plan = plan(&spec, &PlanOptions::default());
        let ads = &mut plan[0];
        let kinds: Vec<&str> = ads.fields.iter().map(|f| f.kind.as_str()).collect();
        assert_eq!(kinds, ["string[]", "boolean", "string"]);
        assert_eq!(ads.fields[1].options, ["true", "false"]);
        assert_eq!(ads.fields[2].options, ["newest", "oldest"]);
        assert_eq!(ads.fields[2].suggestion.as_deref(), Some("newest"));
        assert_eq!(ads.missing(), ["ids"]);

        ads.fields[0].set(split_values("a, b"));
        ads.fields[2].set(split_values("newest,oldest"));
        assert!(ads.missing().is_empty());
        assert_eq!(
            ads.urls(&spec.base_url, &Credentials::default()),
            [
                "https://api.demo.io/ads?ids=a&ids=b&order=newest",
                "https://api.demo.io/ads?ids=a&ids=b&order=oldest"
            ]
        );
        // A required parameter that's emptied is missing again
        ads.fields[0].set(Vec::new());
        assert_eq!(ads.missing(), ["ids"]);
    }

    #[test]
    fn set_values_filters_and_writes() {
        let spec = petstore();
        let options = PlanOptions {
            only: vec!["/pets*".into()],
            skip: vec!["delete *".into()],
            set: PlanOptions::parse_set(&["id=a b,c".into(), "status=sold".into()]).unwrap(),
            include_writes: true,
            ..Default::default()
        };
        let none = Credentials::default();
        let urls = |e: &Endpoint| e.urls(&spec.base_url, &none);
        let plan = plan(&spec, &options);
        let labels: Vec<&str> = plan.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, ["GET /pets", "POST /pets", "GET /pets/{id}"]);

        // --set fills a required parameter with several values, and an optional one
        let pet = find(&plan, "GET /pets/{id}");
        assert!(
            pet.missing().is_empty() && pet.enabled,
            "--only switches on"
        );
        assert_eq!(
            urls(pet),
            [
                "https://api.example.com/v1/pets/a%20b",
                "https://api.example.com/v1/pets/c"
            ]
        );
        assert_eq!(
            urls(find(&plan, "GET /pets")),
            ["https://api.example.com/v1/pets?limit=20&status=sold"]
        );
        assert!(find(&plan, "POST /pets").enabled, "--include-writes");

        // --all: everything with its values, writes only when asked for
        let all = PlanOptions {
            all: true,
            ..Default::default()
        };
        let on: Vec<String> = super::plan(&spec, &all)
            .into_iter()
            .filter(|e| e.enabled)
            .map(|e| e.label)
            .collect();
        assert_eq!(on, ["GET /pets", "GET /orders/{orderId}"]);
    }

    #[test]
    fn credentials_go_where_the_spec_says() {
        let spec = petstore();
        let creds =
            Credentials::parse(&["apikey:s3cret".into(), "bearer:tok".into()], &spec).unwrap();
        assert_eq!(
            creds.headers,
            [
                ("X-API-Key".to_string(), "s3cret".to_string()),
                ("Authorization".to_string(), "Bearer tok".to_string())
            ]
        );
        let creds =
            Credentials::parse(&["basic:ada:pw".into(), "query:key=v 1".into()], &spec).unwrap();
        assert_eq!(creds.headers[0].1, "Basic YWRhOnB3");
        let plan = plan(&spec, &PlanOptions::default());
        assert_eq!(
            find(&plan, "GET /pets").urls(&spec.base_url, &creds),
            ["https://api.example.com/v1/pets?limit=20&key=v%201"]
        );

        assert!(Credentials::parse(&["token".into()], &spec).is_err());
        assert!(Credentials::parse(&["magic:x".into()], &spec).is_err());
    }

    #[test]
    fn api_keys_keep_their_equals_signs_and_are_checked() {
        let spec = petstore();
        // base64 padding isn't NAME=VALUE; the spec's own name may be given
        for arg in [
            "apikey:dGVzdA==",
            "apikey:X-API-Key=dGVzdA==",
            "apikey:x-api-key=dGVzdA==",
        ] {
            let creds = Credentials::parse(&[arg.into()], &spec).unwrap();
            assert_eq!(
                creds.headers,
                [("X-API-Key".to_string(), "dGVzdA==".to_string())],
                "{arg}"
            );
        }
        // Without an API key in the spec, the name has to be given
        let bare: Value = serde_json::json!({"openapi": "3.0.0", "servers": [{"url": "https://x.io"}], "paths": {"/a": {"get": {}}}});
        let bare = Spec::parse(&bare, None, None).unwrap();
        assert!(Credentials::parse(&["apikey:abc".into()], &bare).is_err());
        let creds = Credentials::parse(&["apikey:X-Key=a=b".into()], &bare).unwrap();
        assert_eq!(creds.headers, [("X-Key".to_string(), "a=b".to_string())]);

        // What can't be sent is refused here, not when the run starts
        assert!(Credentials::parse(&["header:Bad Name=x".into()], &spec)
            .unwrap_err()
            .contains("header name"));
        assert!(Credentials::parse(&["bearer:a\nb".into()], &spec).is_err());

        let creds = Credentials::parse(&["bearer:s3cret".into(), "query:k=s3cret".into()], &spec)
            .unwrap()
            .masked();
        assert_eq!(
            creds.headers[0],
            ("Authorization".to_string(), MASK.to_string())
        );
        assert_eq!(creds.query[0], ("k".to_string(), MASK.to_string()));
    }

    #[test]
    fn schemas_that_refer_to_themselves_end() {
        let doc: Value = serde_json::json!({
            "openapi": "3.0.0", "servers": [{"url": "https://x.io"}],
            "components": {"schemas": {
                "Pet": {"oneOf": [{"$ref": "#/components/schemas/Cat"}]},
                "Cat": {"allOf": [{"$ref": "#/components/schemas/Pet"}, {"type": "object", "properties": {"name": {"type": "string"}}}]},
                "Tree": {"type": "array", "items": {"$ref": "#/components/schemas/Tree"}},
                "Node": {"type": "object", "properties": {"next": {"$ref": "#/components/schemas/Node"}}}
            }},
            "paths": {"/pets": {"post": {
                "parameters": [
                    {"name": "tree", "in": "query", "required": true, "schema": {"$ref": "#/components/schemas/Tree"}},
                    {"name": "pet", "in": "query", "schema": {"$ref": "#/components/schemas/Pet"}}
                ],
                "requestBody": {"content": {"application/json": {"schema": {"type": "object", "properties": {
                    "pet": {"$ref": "#/components/schemas/Pet"},
                    "node": {"$ref": "#/components/schemas/Node"}
                }}}}}
            }}}
        });
        let spec = Spec::parse(&doc, None, None).unwrap();
        let op = &spec.operations[0];
        assert_eq!(op.params.len(), 2);
        assert!(op.params[0].kind.ends_with("[]"));
        assert!(op.body.is_some());
    }

    #[test]
    fn base_url_falls_back_to_where_the_spec_came_from() {
        let doc: Value = serde_json::json!({"openapi": "3.1.0", "paths": {"/a": {"get": {}}}});
        let from = Some("https://public.api.example.co/openapi.json");
        assert_eq!(
            Spec::parse(&doc, from, None).unwrap().base_url,
            "https://public.api.example.co"
        );
        assert_eq!(
            Spec::parse(&doc, from, Some("http://localhost:3000/"))
                .unwrap()
                .base_url,
            "http://localhost:3000"
        );
        assert!(Spec::parse(&doc, None, None)
            .unwrap_err()
            .contains("--server"));

        // A relative server resolves against the spec's URL
        let doc: Value = serde_json::json!({"openapi": "3.0.0", "servers": [{"url": "/api/v2"}], "paths": {"/a": {"get": {}}}});
        assert_eq!(
            Spec::parse(&doc, from, None).unwrap().base_url,
            "https://public.api.example.co/api/v2"
        );

        // Swagger 2
        let doc: Value = serde_json::json!({"swagger": "2.0", "host": "h.io", "basePath": "/v1", "schemes": ["http"],
            "paths": {"/a": {"post": {"parameters": [{"in": "body", "name": "b", "schema": {"type": "object", "properties": {"n": {"type": "integer"}}}}]}}}});
        let spec = Spec::parse(&doc, None, None).unwrap();
        assert_eq!(spec.base_url, "http://h.io/v1");
        assert_eq!(spec.operations[0].body.as_ref().unwrap().1, br#"{"n":1}"#);
    }

    #[test]
    fn the_document_itself_is_a_source() {
        // A path or a URL is one line; the spec has many, or opens an object
        assert!(!is_document("openapi.yaml"));
        assert!(!is_document("https://api.x.io/openapi.json"));
        assert!(!is_document("  specs/petstore.yml\n"));
        assert!(is_document(PETSTORE));
        assert!(is_document(r#"{"openapi": "3.0.0", "paths": {}}"#));
        assert_eq!(
            origin("https://api.x.io/openapi.json"),
            Some("https://api.x.io/openapi.json")
        );
        assert_eq!(
            origin(PETSTORE),
            None,
            "a pasted spec has server URLs in it, but came from nowhere"
        );
        assert_eq!(origin("openapi.yaml"), None);

        let doc = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(load(PETSTORE))
            .unwrap();
        assert_eq!(Spec::parse(&doc, None, None).unwrap().title, "Petstore");
        let err = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(load("not: [a spec\nat: all"))
            .unwrap_err();
        assert!(err.contains("not valid JSON or YAML"), "{err}");
    }

    #[test]
    fn rejects_what_is_not_a_spec() {
        assert!(parse_document("{not json: [").is_err());
        let doc = parse_document(r#"{"hello": 1}"#).unwrap();
        assert!(Spec::parse(&doc, None, None).unwrap_err().contains("paths"));
    }

    #[test]
    fn wildcards() {
        assert!(matches("GET /pets*", "GET /pets/{id}", "/pets/{id}"));
        assert!(matches("/pets/*", "GET /pets/{id}", "/pets/{id}"));
        assert!(matches("delete *", "DELETE /x", "/x"));
        assert!(!matches("/pets", "GET /pets/{id}", "/pets/{id}"));
        assert!(matches("*orders*", "GET /v1/orders/1", "/v1/orders/1"));
    }
}
