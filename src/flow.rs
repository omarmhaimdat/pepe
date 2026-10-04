//! Flows: a run that is a sequence of requests, where a value from one
//! response feeds the next. Each worker walks the steps in order with its
//! own variables, like one user would; each step is a row on the
//! dashboard, as endpoints are in API mode.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use bytes::Bytes;
use reqwest::header::HeaderMap;
use serde::Deserialize;

use crate::ui::EndpointView;

/// Body bytes a step keeps for its captures; a longer body is cut there
pub const BODY_CAP: usize = 1024 * 1024;

/// A flow file, as written
#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct FlowFile {
    name: Option<String>,
    /// Values every chain starts with
    #[serde(default)]
    vars: BTreeMap<String, String>,
    #[serde(default)]
    step: Vec<StepFile>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct StepFile {
    name: Option<String>,
    method: Option<String>,
    url: String,
    #[serde(default)]
    headers: Vec<String>,
    body: Option<String>,
    /// Status the step must get; otherwise a 2xx
    expect: Option<u16>,
    /// Variables to take from the response: `token = "json:$.token"`,
    /// `session = "header:Set-Cookie"`, `csrf = "regex:name=\"csrf\" value=\"([^\"]+)\""`,
    /// `all = "body"`
    #[serde(default)]
    capture: BTreeMap<String, String>,
}

/// A flow, checked and ready to run
#[derive(Debug, Clone)]
pub struct Flow {
    pub name: String,
    pub vars: Vec<(String, String)>,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone)]
pub struct Step {
    pub name: String,
    pub method: String,
    pub url: Template,
    pub headers: Vec<(String, Template)>,
    pub body: Option<Template>,
    pub expect: Option<u16>,
    pub captures: Vec<(String, Capture)>,
}

/// Text with `{{name}}` holes
#[derive(Debug, Clone, PartialEq)]
pub struct Template {
    parts: Vec<Part>,
}

#[derive(Debug, Clone, PartialEq)]
enum Part {
    Text(String),
    Var(String),
}

impl Template {
    pub fn parse(text: &str) -> Template {
        let mut parts = Vec::new();
        let mut rest = text;
        while let Some(open) = rest.find("{{") {
            let Some(close) = rest[open..].find("}}") else {
                break;
            };
            if open > 0 {
                parts.push(Part::Text(rest[..open].to_string()));
            }
            parts.push(Part::Var(rest[open + 2..open + close].trim().to_string()));
            rest = &rest[open + close + 2..];
        }
        if !rest.is_empty() {
            parts.push(Part::Text(rest.to_string()));
        }
        Template { parts }
    }

    /// The names it needs
    pub fn vars(&self) -> impl Iterator<Item = &str> {
        self.parts.iter().filter_map(|p| match p {
            Part::Var(name) => Some(name.as_str()),
            Part::Text(_) => None,
        })
    }

    /// Fill the holes; Err names the first variable that has no value
    pub fn render(&self, vars: &HashMap<String, String>) -> Result<String, String> {
        let mut out = String::new();
        for part in &self.parts {
            match part {
                Part::Text(text) => out.push_str(text),
                Part::Var(name) => match vars.get(name) {
                    Some(value) => out.push_str(value),
                    None => return Err(name.clone()),
                },
            }
        }
        Ok(out)
    }

    /// As written, holes included
    pub fn source(&self) -> String {
        self.parts
            .iter()
            .map(|p| match p {
                Part::Text(text) => text.clone(),
                Part::Var(name) => format!("{{{{{name}}}}}"),
            })
            .collect()
    }
}

/// Where a step's variable comes from
#[derive(Debug, Clone)]
pub enum Capture {
    /// A value in a JSON body: `$.data.items[0].id`
    Json(Vec<Key>),
    /// A response header's value
    Header(String),
    /// The first group of a regular expression over the body
    Regex(regex_lite::Regex),
    /// The whole body, trimmed
    Body,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Key {
    Field(String),
    Index(usize),
}

impl Capture {
    pub fn parse(spec: &str) -> Result<Capture, String> {
        let spec = spec.trim();
        if spec == "body" {
            return Ok(Capture::Body);
        }
        let (kind, rest) = spec.split_once(':').ok_or_else(|| {
            format!("{spec:?}: expected json:$.path, header:Name, regex:pattern or body")
        })?;
        match kind.trim() {
            "json" => Ok(Capture::Json(json_path(rest.trim())?)),
            "header" => Ok(Capture::Header(rest.trim().to_string())),
            "regex" => regex_lite::Regex::new(rest.trim())
                .map(Capture::Regex)
                .map_err(|e| format!("regex {:?}: {e}", rest.trim())),
            other => Err(format!(
                "{other:?} is not a capture: expected json:$.path, header:Name, regex:pattern or body"
            )),
        }
    }

    /// The value, if the response has it
    pub fn extract(&self, headers: &HeaderMap, body: &[u8]) -> Option<String> {
        match self {
            Capture::Header(name) => headers
                .get(name)?
                .to_str()
                .ok()
                .map(|s| s.trim().to_string()),
            Capture::Body => Some(String::from_utf8_lossy(body).trim().to_string()),
            Capture::Regex(re) => {
                let text = String::from_utf8_lossy(body);
                let found = re.captures(&text)?;
                found
                    .get(1)
                    .or_else(|| found.get(0))
                    .map(|m| m.as_str().to_string())
            }
            Capture::Json(keys) => {
                let mut value: serde_json::Value = serde_json::from_slice(body).ok()?;
                for key in keys {
                    value = match key {
                        Key::Field(name) => value.get_mut(name)?.take(),
                        Key::Index(i) => value.get_mut(*i)?.take(),
                    };
                }
                Some(match value {
                    serde_json::Value::String(s) => s,
                    serde_json::Value::Null => return None,
                    other => other.to_string(),
                })
            }
        }
    }
}

/// `$.a.b[0].c`, `a.b[0]` or `$["a"]`: the keys to walk
fn json_path(path: &str) -> Result<Vec<Key>, String> {
    let mut keys = Vec::new();
    let path = path.strip_prefix('$').unwrap_or(path);
    let mut rest = path;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('.') {
            let end = after.find(['.', '[']).unwrap_or(after.len());
            if end == 0 {
                return Err(format!("json path {path:?}: empty name"));
            }
            keys.push(Key::Field(after[..end].to_string()));
            rest = &after[end..];
        } else if let Some(after) = rest.strip_prefix('[') {
            let end = after
                .find(']')
                .ok_or_else(|| format!("json path {path:?}: missing ]"))?;
            let inside = after[..end].trim();
            keys.push(match inside.trim_matches(['"', '\'']) {
                name if inside.starts_with(['"', '\'']) => Key::Field(name.to_string()),
                index => Key::Index(
                    index
                        .parse()
                        .map_err(|_| format!("json path {path:?}: {inside:?} is not an index"))?,
                ),
            });
            rest = &after[end + 1..];
        } else {
            // A bare first name: `token` means `$.token`
            let end = rest.find(['.', '[']).unwrap_or(rest.len());
            keys.push(Key::Field(rest[..end].to_string()));
            rest = &rest[end..];
        }
    }
    if keys.is_empty() {
        return Err(format!("json path {path:?} names nothing"));
    }
    Ok(keys)
}

pub fn load(path: &Path) -> Result<Flow, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("couldn't read {}: {e}", path.display()))?;
    let mut flow = parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if flow.name.is_empty() {
        flow.name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "flow".into());
    }
    Ok(flow)
}

pub fn parse(text: &str) -> Result<Flow, String> {
    let file: FlowFile = toml::from_str(text).map_err(|e| e.message().to_string())?;
    if file.step.is_empty() {
        return Err("a flow needs at least one [[step]]".into());
    }
    let mut known: Vec<String> = file.vars.keys().cloned().collect();
    let mut steps = Vec::new();
    for (i, step) in file.step.into_iter().enumerate() {
        let name = step.name.unwrap_or_else(|| format!("step {}", i + 1));
        let url = Template::parse(step.url.trim());
        if step.url.trim().is_empty() {
            return Err(format!("step {name:?} has no url"));
        }
        let headers = step
            .headers
            .iter()
            .map(|h| {
                h.split_once(':')
                    .map(|(n, v)| (n.trim().to_string(), Template::parse(v.trim())))
                    .ok_or_else(|| format!("step {name:?}: header {h:?} needs a colon"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let body = step.body.map(|b| Template::parse(&b));
        // Every hole must be filled by then: a starting value or an
        // earlier step's capture
        let needs = url
            .vars()
            .chain(headers.iter().flat_map(|(_, t)| t.vars()))
            .chain(body.iter().flat_map(|t| t.vars()));
        for var in needs {
            if !known.iter().any(|k| k == var) {
                return Err(format!(
                    "step {name:?} uses {{{{{var}}}}}, which no earlier step captures and [vars] doesn't set"
                ));
            }
        }
        let mut captures = Vec::new();
        for (var, spec) in step.capture {
            let capture =
                Capture::parse(&spec).map_err(|e| format!("step {name:?}, capture {var}: {e}"))?;
            captures.push((var.clone(), capture));
            known.push(var);
        }
        steps.push(Step {
            name,
            method: step.method.unwrap_or_else(|| "GET".into()).to_uppercase(),
            url,
            headers,
            body,
            expect: step.expect,
            captures,
        });
    }
    Ok(Flow {
        name: file.name.unwrap_or_default(),
        vars: file.vars.into_iter().collect(),
        steps,
    })
}

impl Flow {
    /// What the dashboard shows for each step
    pub fn views(&self) -> Vec<EndpointView> {
        self.steps
            .iter()
            .map(|step| EndpointView {
                label: step.name.clone(),
                method: step.method.clone(),
                url: step.url.source(),
                variants: 1,
                headers: step
                    .headers
                    .iter()
                    .map(|(n, t)| (n.clone(), t.source()))
                    .collect(),
                body: step.body.as_ref().map(|b| b.source().into_bytes()),
            })
            .collect()
    }

    /// A URL the clients can be built around: the first step's, with the
    /// starting values filled in where they reach
    pub fn base_url(&self) -> String {
        let vars: HashMap<String, String> = self.vars.iter().cloned().collect();
        self.steps[0]
            .url
            .render(&vars)
            .unwrap_or_else(|_| "http://flow.invalid/".into())
    }
}

impl Step {
    /// The step's request for this chain's values; Err names a hole
    /// nothing filled, which a checked flow never has
    pub fn build(&self, vars: &HashMap<String, String>) -> Result<reqwest::Request, String> {
        let url = self.url.render(vars)?;
        let url = reqwest::Url::parse(&url).map_err(|e| format!("url {url:?}: {e}"))?;
        let method = reqwest::Method::from_bytes(self.method.as_bytes())
            .map_err(|_| format!("method {:?}", self.method))?;
        let mut request = reqwest::Request::new(method, url);
        for (name, value) in &self.headers {
            let value = value.render(vars)?;
            if let (Ok(name), Ok(value)) = (
                reqwest::header::HeaderName::from_bytes(name.as_bytes()),
                reqwest::header::HeaderValue::from_str(&value),
            ) {
                request.headers_mut().append(name, value);
            }
        }
        if let Some(body) = &self.body {
            *request.body_mut() = Some(Bytes::from(body.render(vars)?).into());
        }
        Ok(request)
    }

    /// Take the step's variables from its response; Err names the first
    /// capture that found nothing
    pub fn capture(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        vars: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        for (name, capture) in &self.captures {
            match capture.extract(headers, body) {
                Some(value) => {
                    vars.insert(name.clone(), value);
                }
                None => return Err(name.clone()),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderValue;

    #[test]
    fn templates_fill_their_holes_and_name_what_is_missing() {
        let t = Template::parse("https://{{ host }}/users/{{id}}?x=1");
        assert_eq!(t.vars().collect::<Vec<_>>(), ["host", "id"]);
        let vars: HashMap<String, String> = [("host", "api.test"), ("id", "42")]
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
        assert_eq!(t.render(&vars).unwrap(), "https://api.test/users/42?x=1");
        assert_eq!(t.source(), "https://{{host}}/users/{{id}}?x=1");
        let fewer: HashMap<String, String> = [("host".to_string(), "h".to_string())].into();
        assert_eq!(t.render(&fewer), Err("id".into()));
        assert_eq!(Template::parse("plain").render(&fewer).unwrap(), "plain");
        assert_eq!(
            Template::parse("open {{ brace").render(&fewer).unwrap(),
            "open {{ brace"
        );
    }

    #[test]
    fn captures_read_json_headers_regexes_and_bodies() {
        let mut headers = HeaderMap::new();
        headers.insert("x-session", HeaderValue::from_static("  s-1 "));
        let body = br#"{"token":"abc","n":7,"ok":true,"data":{"items":[{"id":"first"},{"id":"second"}]},"none":null}"#;
        let get = |spec: &str| Capture::parse(spec).unwrap().extract(&headers, body);
        assert_eq!(get("json:$.token").as_deref(), Some("abc"));
        assert_eq!(get("json:token").as_deref(), Some("abc"));
        assert_eq!(get("json:$.n").as_deref(), Some("7"));
        assert_eq!(get("json:$.ok").as_deref(), Some("true"));
        assert_eq!(get("json:$.data.items[1].id").as_deref(), Some("second"));
        assert_eq!(
            get(r#"json:$["data"].items[0]"#).as_deref(),
            Some(r#"{"id":"first"}"#)
        );
        assert_eq!(get("json:$.missing"), None);
        assert_eq!(get("json:$.none"), None, "null is nothing");
        assert_eq!(get("header:X-Session").as_deref(), Some("s-1"));
        assert_eq!(get("header:X-Other"), None);
        assert_eq!(get(r#"regex:"token":"([a-z]+)""#).as_deref(), Some("abc"));
        assert_eq!(get("regex:zzz"), None);
        assert_eq!(
            get("body").as_deref(),
            Some(std::str::from_utf8(body).unwrap())
        );
        assert!(Capture::parse("xml:/a")
            .unwrap_err()
            .contains("not a capture"));
        assert!(Capture::parse("json:$.a[x]")
            .unwrap_err()
            .contains("not an index"));
        assert!(Capture::parse("regex:(").is_err());
    }

    #[test]
    fn a_flow_is_checked_as_it_is_read() {
        let flow = parse(
            r#"
            name = "checkout"
            [vars]
            host = "https://shop.test"
            [[step]]
            name = "login"
            method = "post"
            url = "{{host}}/login"
            body = '{"user":"u"}'
            capture = { token = "json:$.token" }
            [[step]]
            url = "{{host}}/me"
            headers = ["Authorization: Bearer {{token}}"]
            expect = 200
            "#,
        )
        .unwrap();
        assert_eq!(flow.name, "checkout");
        assert_eq!(flow.steps.len(), 2);
        assert_eq!(
            (flow.steps[0].method.as_str(), flow.steps[1].name.as_str()),
            ("POST", "step 2")
        );
        assert_eq!(flow.base_url(), "https://shop.test/login");
        let views = flow.views();
        assert_eq!(
            views[1].headers,
            [("Authorization".to_string(), "Bearer {{token}}".to_string())]
        );

        let err = parse("[[step]]\nurl = \"https://x/{{id}}\"\n").unwrap_err();
        assert!(
            err.contains("{{id}}") && err.contains("no earlier step"),
            "{err}"
        );
        assert!(parse("name = \"x\"\n")
            .unwrap_err()
            .contains("at least one"));
        assert!(
            parse("[[step]]\nurl = \"https://x\"\nheaders = [\"nocolon\"]\n")
                .unwrap_err()
                .contains("needs a colon")
        );
        assert!(
            parse("[[step]]\nurl = \"https://x\"\ncapture = { a = \"nope\" }\n")
                .unwrap_err()
                .contains("capture a")
        );
        assert!(parse("[[step]]\nurl = \"https://x\"\nbogus = 1\n")
            .unwrap_err()
            .contains("unknown field"));
    }

    #[test]
    fn a_step_builds_its_request_and_takes_its_values() {
        let flow = parse(
            r#"
            [[step]]
            url = "https://x.test/login"
            capture = { token = "json:$.t", sid = "header:X-Sid" }
            [[step]]
            method = "put"
            url = "https://x.test/me/{{sid}}"
            headers = ["Authorization: Bearer {{token}}"]
            body = "sid={{sid}}"
            "#,
        )
        .unwrap();
        let mut vars = HashMap::new();
        let mut headers = HeaderMap::new();
        headers.insert("x-sid", HeaderValue::from_static("9"));
        flow.steps[0]
            .capture(&headers, br#"{"t":"tok"}"#, &mut vars)
            .unwrap();
        let request = flow.steps[1].build(&vars).unwrap();
        assert_eq!(request.url().as_str(), "https://x.test/me/9");
        assert_eq!(request.method(), reqwest::Method::PUT);
        assert_eq!(
            request.headers().get("authorization").unwrap(),
            "Bearer tok"
        );
        assert_eq!(request.body().unwrap().as_bytes(), Some(&b"sid=9"[..]));
        // Captures are checked by name, so the first missing one is "sid"
        assert_eq!(
            flow.steps[0].capture(&HeaderMap::new(), b"{}", &mut vars),
            Err("sid".into())
        );
    }
}
