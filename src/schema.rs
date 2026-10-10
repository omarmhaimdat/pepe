//! `pepe schema`: the JSON Schema of each report, embedded from `schema/`
//! and published with every release, so a script or an agent can depend
//! on the field names while sections keep being added

pub const RUN: &str = include_str!("../schema/run.schema.json");
pub const RAMP: &str = include_str!("../schema/ramp.schema.json");
pub const PING: &str = include_str!("../schema/ping.schema.json");
pub const COMPARE: &str = include_str!("../schema/compare.schema.json");

/// Each schema by the name `pepe schema` takes
pub const ALL: [(&str, &str); 4] = [
    ("run", RUN),
    ("ramp", RAMP),
    ("ping", PING),
    ("compare", COMPARE),
];

/// Print the schema named, or the run report's
pub fn print(which: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let name = which.unwrap_or("run").trim().to_ascii_lowercase();
    match ALL.iter().find(|(n, _)| *n == name) {
        Some((_, text)) => {
            print!("{text}");
            Ok(())
        }
        None => {
            eprintln!(
                "error: no schema named {name:?}; there are {}",
                ALL.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")
            );
            std::process::exit(2);
        }
    }
}

/// What in `doc` doesn't fit `schema`: a required field missing, or a
/// value of another type than the one named. Enough of JSON Schema to
/// keep the files honest; not a validator for the world.
#[cfg(test)]
pub fn check(schema: &serde_json::Value, doc: &serde_json::Value) -> Vec<String> {
    let mut out = Vec::new();
    walk(schema, schema, doc, "$", &mut out);
    out
}

#[cfg(test)]
fn walk(
    root: &serde_json::Value,
    schema: &serde_json::Value,
    doc: &serde_json::Value,
    at: &str,
    out: &mut Vec<String>,
) {
    use serde_json::Value;
    let schema = match schema.get("$ref").and_then(Value::as_str) {
        Some(path) => {
            let mut found = root;
            for key in path.trim_start_matches("#/").split('/') {
                found = &found[key];
            }
            found
        }
        None => schema,
    };
    if let Some(kinds) = schema.get("type") {
        let kinds: Vec<&str> = match kinds {
            Value::String(s) => vec![s.as_str()],
            Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
            _ => vec![],
        };
        let fits = kinds.iter().any(|kind| match *kind {
            "object" => doc.is_object(),
            "array" => doc.is_array(),
            "string" => doc.is_string(),
            "integer" => doc.as_f64().is_some_and(|n| n.fract() == 0.0),
            "number" => doc.is_number(),
            "boolean" => doc.is_boolean(),
            "null" => doc.is_null(),
            _ => false,
        });
        if !fits {
            out.push(format!("{at}: {doc} isn't {}", kinds.join(" or ")));
            return;
        }
    }
    if doc.is_null() {
        return;
    }
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for name in required.iter().filter_map(Value::as_str) {
            if doc.get(name).is_none() {
                out.push(format!("{at}: {name} is required"));
            }
        }
    }
    if let (Some(properties), Some(object)) = (
        schema.get("properties").and_then(Value::as_object),
        doc.as_object(),
    ) {
        for (name, value) in object {
            if let Some(sub) = properties.get(name) {
                walk(root, sub, value, &format!("{at}.{name}"), out);
            } else if let Some(extra) = schema.get("additionalProperties") {
                if extra.is_object() {
                    walk(root, extra, value, &format!("{at}.{name}"), out);
                }
            }
        }
    }
    if let (Some(items), Some(array)) = (schema.get("items"), doc.as_array()) {
        for (i, value) in array.iter().enumerate() {
            walk(root, items, value, &format!("{at}[{i}]"), out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::Metrics;
    use crate::response::ResponseStats;
    use serde_json::Value;
    use std::time::Duration;

    fn schema(name: &str) -> Value {
        let text = ALL.iter().find(|(n, _)| *n == name).unwrap().1;
        serde_json::from_str(text).unwrap_or_else(|e| panic!("{name}: {e}"))
    }

    fn metrics() -> Metrics {
        let mut m = Metrics::default();
        for i in 0..20u64 {
            m.record(&ResponseStats {
                duration: Duration::from_millis(10 + i),
                status_code: Some(if i == 3 {
                    reqwest::StatusCode::BAD_GATEWAY
                } else {
                    reqwest::StatusCode::OK
                }),
                ttfb: Some(Duration::from_millis(8)),
                body_bytes: 100,
                ..Default::default()
            });
        }
        m
    }

    #[test]
    fn every_schema_is_json_schema() {
        for (name, text) in ALL {
            let value: Value = serde_json::from_str(text).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(value["type"], "object", "{name}");
            assert!(
                value["required"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|r| r == "schema_version"),
                "{name} requires schema_version"
            );
            assert_eq!(value["properties"]["schema_version"]["const"], 1, "{name}");
        }
    }

    #[test]
    fn the_checker_finds_what_is_wrong() {
        let schema = serde_json::json!({
            "type": "object",
            "required": ["a"],
            "properties": {
                "a": {"type": "integer"},
                "b": {"type": ["string", "null"]},
                "c": {"$ref": "#/$defs/thing"},
                "d": {"type": "array", "items": {"type": "number"}}
            },
            "$defs": {"thing": {"type": "object", "required": ["x"]}}
        });
        assert!(check(
            &schema,
            &serde_json::json!({"a": 1, "b": null, "c": {"x": 1}, "d": [1.5]})
        )
        .is_empty());
        let wrong = check(
            &schema,
            &serde_json::json!({"a": 1.5, "c": {}, "d": ["no"]}),
        );
        assert_eq!(
            wrong,
            [
                "$.a: 1.5 isn't integer",
                "$.c: x is required",
                "$.d[0]: \"no\" isn't number"
            ]
        );
        assert_eq!(check(&schema, &serde_json::json!({})), ["$: a is required"]);
    }

    #[test]
    fn a_run_report_fits_its_schema() {
        let m = metrics();
        let timeline = crate::timeline::Timeline::default();
        let report = crate::json_report::JsonReport::generate(&m, Duration::from_secs(2), false)
            .with_target("run", Some("GET"), "http://x.io/", 8)
            .with_generator(1, Some(12), Some((100.0, 0)))
            .with_timeline(&timeline)
            .with_snapshot(true);
        let doc: Value = serde_json::from_str(&report.to_json().unwrap()).unwrap();
        assert_eq!(doc["schema_version"], 1);
        assert_eq!(check(&schema("run"), &doc), Vec::<String>::new());
    }

    #[test]
    fn a_ramp_report_fits_its_schema() {
        use crate::ramp::{Ramp, RampPlan};
        let plan = RampPlan::from_args(&crate::cli::RampArgs {
            url: "http://x.io/".into(),
            from: 1,
            to: 2,
            step: 1,
            every: "1s".into(),
            until: vec!["p99 > 500ms".into()],
        })
        .unwrap();
        let ramp = Ramp::new(plan, std::time::Instant::now());
        let doc = crate::ramp::json(&ramp);
        assert_eq!(doc["schema_version"], 1);
        assert_eq!(check(&schema("ramp"), &doc), Vec::<String>::new());
    }

    #[test]
    fn a_compare_report_fits_its_schema() {
        use crate::compare;
        let dir = std::env::temp_dir().join(format!("pepe-schema-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.json");
        let report =
            crate::json_report::JsonReport::generate(&metrics(), Duration::from_secs(2), false)
                .with_target("run", Some("GET"), "http://x.io/", 8);
        std::fs::write(&path, report.to_json().unwrap()).unwrap();
        let side = compare::Side::read(&path).unwrap();
        let doc: Value =
            serde_json::from_str(&compare::compare(&side, &side).to_json().unwrap()).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(doc["schema_version"], 1);
        assert_eq!(check(&schema("compare"), &doc), Vec::<String>::new());
    }
}
