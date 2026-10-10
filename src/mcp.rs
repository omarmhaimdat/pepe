//! `pepe mcp`: pepe's modes as tools for an agent, over stdio, in the
//! Model Context Protocol: JSON-RPC 2.0 messages, one a line, in and out.
//!
//! Each tool runs pepe itself with `--json` and hands back the report as
//! text, and as structured content when it parses. The guardrail flags
//! `pepe mcp` was started with (`--allow-host`, `--max-requests`,
//! `--max-rate`, `--max-concurrency`) go on every call, so the operator
//! who starts the server decides where it may send and how much; a tool
//! call can't loosen them.

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::cli::Cli;

/// The protocol versions this server speaks; the client's is echoed
/// when it is one of them
const VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

/// What an agent sees first
const INSTRUCTIONS: &str =
    "pepe load-tests and pings HTTP endpoints. Every tool returns pepe's JSON report \
(schema_version 1; `pepe schema NAME` has the schema). Start with `ping` to see where an \
endpoint's time goes, `run` to load it, `ramp` to find its capacity, `compare` to hold two \
reports against each other. Only send to what you are allowed to load; the server's \
guardrails refuse the rest with exit code 2.";

/// The flags `pepe mcp` was started with that go on every call
pub fn guard_args(cli: &Cli) -> Vec<String> {
    let mut out = Vec::new();
    for host in &cli.allow_host {
        out.push("--allow-host".into());
        out.push(host.clone());
    }
    if let Some(n) = cli.max_requests {
        out.push("--max-requests".into());
        out.push(n.to_string());
    }
    if let Some(r) = cli.max_rate {
        out.push("--max-rate".into());
        out.push(r.to_string());
    }
    if let Some(c) = cli.max_concurrency {
        out.push("--max-concurrency".into());
        out.push(c.to_string());
    }
    out
}

/// Serve on stdin and stdout until stdin ends
pub async fn serve(guard: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut out = tokio::io::stdout();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let message: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let reply = error(Value::Null, -32700, &format!("parse error: {e}"));
                out.write_all(format!("{reply}\n").as_bytes()).await?;
                out.flush().await?;
                continue;
            }
        };
        if let Some(reply) = handle(&guard, &message).await {
            out.write_all(format!("{reply}\n").as_bytes()).await?;
            out.flush().await?;
        }
    }
    Ok(())
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn result(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

/// One message in, at most one out: none for a notification
pub async fn handle(guard: &[String], message: &Value) -> Option<Value> {
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let id = message.get("id").cloned();
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    // A notification has no id and gets no reply
    let id = id?;
    Some(match method {
        "initialize" => {
            let asked = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or("");
            let version = VERSIONS
                .iter()
                .find(|v| **v == asked)
                .copied()
                .unwrap_or(VERSIONS[1]);
            result(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {"listChanged": false}},
                    "serverInfo": {"name": "pepe", "version": crate::utils::version()},
                    "instructions": INSTRUCTIONS,
                }),
            )
        }
        "ping" => result(id, json!({})),
        "tools/list" => result(id, json!({"tools": tools()})),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
            match command_for(name, &arguments) {
                Ok(argv) => result(id, call(guard, argv).await),
                Err(e) => result(
                    id,
                    json!({"content": [{"type": "text", "text": e}], "isError": true}),
                ),
            }
        }
        other => error(id, -32601, &format!("method not found: {other}")),
    })
}

/// Run pepe with `argv` and the guard, and say what it printed
async fn call(guard: &[String], argv: Vec<String>) -> Value {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => {
            return json!({"content": [{"type": "text", "text": format!("can't find pepe: {e}")}], "isError": true})
        }
    };
    let output = tokio::process::Command::new(exe)
        .args(&argv)
        .args(guard)
        .env("PEPE_NO_UPDATE_CHECK", "1")
        .stdin(std::process::Stdio::null())
        .output()
        .await;
    let output = match output {
        Ok(output) => output,
        Err(e) => {
            return json!({"content": [{"type": "text", "text": format!("can't run pepe: {e}")}], "isError": true})
        }
    };
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let code = output.status.code().unwrap_or(-1);
    // 4 is a limit crossed: the report says so, and is the answer
    let is_error = !(code == 0 || code == 4);
    let mut content = Vec::new();
    if !stdout.is_empty() {
        content.push(json!({"type": "text", "text": stdout}));
    }
    if code != 0 {
        content.push(json!({"type": "text", "text": format!(
            "pepe exited {code}{}{}",
            match code {
                1 => ": it couldn't start, or nothing answered",
                2 => ": a usage error, or a guardrail refused the run",
                4 => ": a limit was crossed (--fail-if, --slo); the report is above",
                _ => "",
            },
            if stderr.is_empty() { String::new() } else { format!("\n{stderr}") }
        )}));
    } else if content.is_empty() {
        content.push(json!({"type": "text", "text": "pepe printed nothing"}));
    }
    let mut reply = json!({"content": content, "isError": is_error});
    if let Ok(parsed) = serde_json::from_str::<Value>(&stdout) {
        if parsed.is_object() {
            reply["structuredContent"] = parsed;
        }
    }
    reply
}

/// A flag with a value, when the argument is there
fn flag(argv: &mut Vec<String>, args: &Value, key: &str, flag: &str) {
    if let Some(v) = args.get(key) {
        let text = match v {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Bool(_) | Value::Null | Value::Array(_) | Value::Object(_) => return,
        };
        argv.push(flag.to_string());
        argv.push(text);
    }
}

/// A flag repeated for each string of an array
fn flags(argv: &mut Vec<String>, args: &Value, key: &str, flag: &str) {
    if let Some(items) = args.get(key).and_then(Value::as_array) {
        for item in items.iter().filter_map(Value::as_str) {
            argv.push(flag.to_string());
            argv.push(item.to_string());
        }
    }
}

fn switch(argv: &mut Vec<String>, args: &Value, key: &str, flag: &str) {
    if args.get(key).and_then(Value::as_bool) == Some(true) {
        argv.push(flag.to_string());
    }
}

fn required<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("{key} is required"))
}

/// The request and load flags the load-testing tools share
fn load_flags(argv: &mut Vec<String>, args: &Value) {
    flag(argv, args, "requests", "-n");
    flag(argv, args, "duration", "-z");
    flag(argv, args, "concurrency", "-c");
    // A small count with the default concurrency (one per core) would be
    // refused as more in flight than requests; send them one at a time
    // per request instead
    if let (Some(n), None, None) = (
        args.get("requests").and_then(Value::as_u64),
        args.get("concurrency"),
        args.get("duration"),
    ) {
        let cores = u64::from(crate::utils::num_of_cores());
        if n < cores {
            argv.push("-c".into());
            argv.push(n.max(1).to_string());
        }
    }
    flag(argv, args, "rate", "--rate");
    flag(argv, args, "warmup", "--warmup");
    flag(argv, args, "threads", "--threads");
    flag(argv, args, "timeout", "-t");
    flags(argv, args, "fail_if", "--fail-if");
    request_flags(argv, args);
}

fn request_flags(argv: &mut Vec<String>, args: &Value) {
    flag(argv, args, "method", "-m");
    flags(argv, args, "headers", "-H");
    flag(argv, args, "body", "-d");
    flag(argv, args, "proxy", "-p");
    switch(argv, args, "insecure", "-k");
    switch(argv, args, "dry_run", "--dry-run");
}

/// pepe's arguments for a tool call, `--json` included
pub fn command_for(name: &str, args: &Value) -> Result<Vec<String>, String> {
    let mut argv: Vec<String> = Vec::new();
    match name {
        "run" => {
            let url = required(args, "url")?;
            argv.push("--json".into());
            load_flags(&mut argv, args);
            argv.push(url.to_string());
        }
        "ping" => {
            let targets: Vec<&str> = args
                .get("targets")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            if targets.is_empty() {
                return Err("targets is required: one URL or host, or more".into());
            }
            argv.extend(["ping".to_string(), "--json".to_string()]);
            // A ping ends by count or by time; without either it never would
            if args.get("count").is_some() {
                flag(&mut argv, args, "count", "-n");
            } else if args.get("duration").is_some() {
                flag(&mut argv, args, "duration", "-z");
            } else {
                argv.extend(["-n".to_string(), "5".to_string()]);
            }
            flag(&mut argv, args, "every", "--every");
            flag(&mut argv, args, "slo", "--slo");
            flag(&mut argv, args, "timeout", "-t");
            switch(&mut argv, args, "keep_alive", "--keep-alive");
            switch(&mut argv, args, "all_ips", "--all-ips");
            switch(&mut argv, args, "tcp", "--tcp");
            request_flags(&mut argv, args);
            argv.extend(targets.iter().map(|t| t.to_string()));
        }
        "ramp" => {
            let url = required(args, "url")?;
            argv.extend(["ramp".to_string(), "--json".to_string()]);
            flag(&mut argv, args, "from", "--from");
            flag(&mut argv, args, "to", "--to");
            flag(&mut argv, args, "step", "--step");
            flag(&mut argv, args, "every", "--every");
            flags(&mut argv, args, "until", "--until");
            flag(&mut argv, args, "timeout", "-t");
            flags(&mut argv, args, "fail_if", "--fail-if");
            request_flags(&mut argv, args);
            argv.push(url.to_string());
        }
        "api" => {
            let spec = required(args, "spec")?;
            argv.extend(["api".to_string(), "--json".to_string()]);
            flag(&mut argv, args, "server", "--server");
            flags(&mut argv, args, "auth", "--auth");
            switch(&mut argv, args, "all", "--all");
            flags(&mut argv, args, "tag", "--tag");
            flags(&mut argv, args, "only", "--only");
            flags(&mut argv, args, "skip", "--skip");
            flags(&mut argv, args, "set", "--set");
            switch(&mut argv, args, "include_writes", "--include-writes");
            load_flags(&mut argv, args);
            argv.push(spec.to_string());
        }
        "flow" => {
            let file = required(args, "file")?;
            argv.extend(["flow".to_string(), "--json".to_string()]);
            load_flags(&mut argv, args);
            argv.push(file.to_string());
        }
        "replay" => {
            let log = required(args, "log")?;
            argv.extend(["replay".to_string(), "--json".to_string()]);
            flag(&mut argv, args, "base_url", "--base-url");
            switch(&mut argv, args, "include_writes", "--include-writes");
            load_flags(&mut argv, args);
            argv.push(log.to_string());
        }
        "compare" => {
            let before = required(args, "before")?;
            let after = required(args, "after")?;
            argv.extend(["compare".to_string(), "--json".to_string()]);
            switch(&mut argv, args, "gate", "--gate");
            flag(&mut argv, args, "svg", "--svg");
            argv.push(before.to_string());
            argv.push(after.to_string());
        }
        other => return Err(format!("no tool named {other:?}")),
    }
    Ok(argv)
}

/// The tools, as `tools/list` describes them
pub fn tools() -> Vec<Value> {
    let string = |d: &str| json!({"type": "string", "description": d});
    let integer = |d: &str| json!({"type": "integer", "description": d});
    let number = |d: &str| json!({"type": "number", "description": d});
    let boolean = |d: &str| json!({"type": "boolean", "description": d});
    let strings = |d: &str| json!({"type": "array", "items": {"type": "string"}, "description": d});
    let request = |props: &mut serde_json::Map<String, Value>| {
        props.insert("method".into(), string("HTTP method; GET by default"));
        props.insert("headers".into(), strings("Headers, each 'Name: value'"));
        props.insert("body".into(), string("The request body"));
        props.insert(
            "timeout".into(),
            integer("Seconds to wait for a response (20)"),
        );
        props.insert(
            "insecure".into(),
            boolean("Accept invalid TLS certificates"),
        );
        props.insert(
            "dry_run".into(),
            boolean("Say what would be sent and send nothing"),
        );
    };
    let load = |props: &mut serde_json::Map<String, Value>| {
        props.insert(
            "requests".into(),
            integer("Requests to send (100); or duration"),
        );
        props.insert(
            "duration".into(),
            string("Run for this long instead: '30s', '2m'"),
        );
        props.insert(
            "concurrency".into(),
            integer("Requests in flight at once (one per core)"),
        );
        props.insert("rate".into(), number("Start this many requests a second, spread evenly; concurrency is then the most in flight"));
        props.insert(
            "warmup".into(),
            string("Send for this long before measuring: '5s'"),
        );
        props.insert(
            "fail_if".into(),
            strings("Conditions that make the exit code 4: 'p99 > 300ms', 'errors > 1%'"),
        );
        request(props);
    };
    let schema = |required: &[&str], fill: &dyn Fn(&mut serde_json::Map<String, Value>)| {
        let mut props = serde_json::Map::new();
        fill(&mut props);
        json!({"type": "object", "properties": props, "required": required})
    };
    vec![
        json!({
            "name": "run",
            "description": "Load-test a URL: send requests at a concurrency, for a count or a duration, and report throughput, latency percentiles and phases, status codes, failures by cause and anomalies. The report's `summary` has the numbers; `generator.peak_busy_percent` near 100 means pepe was the limit.",
            "inputSchema": schema(&["url"], &|p| { p.insert("url".into(), string("The URL to send to")); load(p); }),
        }),
        json!({
            "name": "ping",
            "description": "Ping URLs over HTTP: a request every interval to each target, each split into DNS, connect, TLS, first byte and download, with loss, jitter, percentiles, the TLS session, the certificate's expiry, and findings that say what to look at. Five pings by default.",
            "inputSchema": schema(&["targets"], &|p| {
                p.insert("targets".into(), strings("URLs or hosts; several are reported apart"));
                p.insert("count".into(), integer("Pings per target (5)"));
                p.insert("duration".into(), string("Ping for this long instead: '30s'"));
                p.insert("every".into(), string("Time between pings: '1s', '200ms'"));
                p.insert("slo".into(), string("Limits in milliseconds, exit code 4 when broken: 'total=500,ttfb=200'"));
                p.insert("keep_alive".into(), boolean("Keep the connection between pings"));
                p.insert("all_ips".into(), boolean("Ping every address a name resolves to"));
                p.insert("tcp".into(), boolean("Only connect to the port; no request"));
                request(p);
            }),
        }),
        json!({
            "name": "ramp",
            "description": "Raise the concurrency step by step and find the level the target held, where throughput stopped following the load, where it broke, and the capacity estimate.",
            "inputSchema": schema(&["url"], &|p| {
                p.insert("url".into(), string("The URL to send to"));
                p.insert("from".into(), integer("Concurrency of the first step (10)"));
                p.insert("to".into(), integer("Concurrency of the last step (100)"));
                p.insert("step".into(), integer("Concurrency added at each step (10)"));
                p.insert("every".into(), string("How long each step is held: '10s'"));
                p.insert("until".into(), strings("End once a step crosses one: 'p99 > 500ms', 'errors > 1%'"));
                p.insert("fail_if".into(), strings("Conditions that make the exit code 4"));
                request(p);
            }),
        }),
        json!({
            "name": "api",
            "description": "Load-test the endpoints of an OpenAPI 3 or Swagger 2 spec, picked by tag or pattern; the report has an entry per endpoint.",
            "inputSchema": schema(&["spec"], &|p| {
                p.insert("spec".into(), string("The spec: a file path or a URL"));
                p.insert("server".into(), string("Send here instead of the spec's server"));
                p.insert("auth".into(), strings("Credentials: 'bearer:TOKEN', 'basic:USER:PASSWORD', 'apikey:VALUE', 'header:NAME=VALUE', 'query:NAME=VALUE'"));
                p.insert("all".into(), boolean("Run every endpoint that has the values it needs"));
                p.insert("tag".into(), strings("Run the endpoints with these tags"));
                p.insert("only".into(), strings("Run the endpoints matching these: 'GET /pets*'"));
                p.insert("skip".into(), strings("Leave out endpoints matching these"));
                p.insert("set".into(), strings("Parameter values: 'id=1,2,3'"));
                p.insert("include_writes".into(), boolean("Let all, tag and only switch on POST, PUT, PATCH and DELETE too"));
                load(p);
            }),
        }),
        json!({
            "name": "flow",
            "description": "Run a flow file: a sequence of requests where a value from one response feeds the next; the report has an entry per step and how many chains completed.",
            "inputSchema": schema(&["file"], &|p| { p.insert("file".into(), string("The flow file, TOML")); load(p); }),
        }),
        json!({
            "name": "replay",
            "description": "Send the URLs of an access log (nginx, Apache, Caddy, ALB, or one per line) in their real proportions; only GET, HEAD and OPTIONS unless include_writes.",
            "inputSchema": schema(&["log"], &|p| {
                p.insert("log".into(), string("The log file"));
                p.insert("base_url".into(), string("Send to this host instead: put in front of paths, in place of the host of URLs"));
                p.insert("include_writes".into(), boolean("Replay POST, PUT, PATCH and DELETE too"));
                load(p);
            }),
        }),
        json!({
            "name": "compare",
            "description": "Hold a pepe JSON report against an earlier one of the same test and say what moved: a verdict (faster, same, slower, better, worse), each number before and after with whether the change is beyond the usual spread, and findings. Run, ramp and ping reports all compare.",
            "inputSchema": schema(&["before", "after"], &|p| {
                p.insert("before".into(), string("Path of the earlier report"));
                p.insert("after".into(), string("Path of the later report"));
                p.insert("gate".into(), boolean("Exit 1 on a regression"));
                p.insert("svg".into(), string("Also draw the verdict as an SVG card at this path"));
            }),
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_calls_become_pepe_commands() {
        let argv = command_for(
            "run",
            &json!({"url": "https://x.test/", "requests": 50, "concurrency": 4, "headers": ["A: b", "C: d"], "fail_if": ["p99 > 1ms"], "dry_run": true}),
        )
        .unwrap();
        // A count under the core count gets a concurrency to match
        let few = command_for("run", &json!({"url": "https://x.test/", "requests": 2})).unwrap();
        assert_eq!(few, ["--json", "-n", "2", "-c", "2", "https://x.test/"]);
        assert_eq!(
            argv,
            [
                "--json",
                "-n",
                "50",
                "-c",
                "4",
                "--fail-if",
                "p99 > 1ms",
                "-H",
                "A: b",
                "-H",
                "C: d",
                "--dry-run",
                "https://x.test/"
            ]
        );
        let argv = command_for(
            "ping",
            &json!({"targets": ["a.test", "b.test"], "slo": "total=500"}),
        )
        .unwrap();
        assert_eq!(
            argv,
            [
                "ping",
                "--json",
                "-n",
                "5",
                "--slo",
                "total=500",
                "a.test",
                "b.test"
            ]
        );
        let argv = command_for(
            "ping",
            &json!({"targets": ["a.test"], "duration": "10s", "every": "200ms"}),
        )
        .unwrap();
        assert_eq!(
            argv,
            ["ping", "--json", "-z", "10s", "--every", "200ms", "a.test"]
        );
        let argv = command_for(
            "ramp",
            &json!({"url": "https://x.test/", "to": 50, "until": ["errors > 1%"]}),
        )
        .unwrap();
        assert_eq!(
            argv,
            [
                "ramp",
                "--json",
                "--to",
                "50",
                "--until",
                "errors > 1%",
                "https://x.test/"
            ]
        );
        let argv = command_for(
            "compare",
            &json!({"before": "a.json", "after": "b.json", "gate": true}),
        )
        .unwrap();
        assert_eq!(argv, ["compare", "--json", "--gate", "a.json", "b.json"]);
        let argv = command_for(
            "api",
            &json!({"spec": "openapi.yaml", "tag": ["Billing"], "all": false, "set": ["id=1,2"]}),
        )
        .unwrap();
        assert_eq!(
            argv,
            [
                "api",
                "--json",
                "--tag",
                "Billing",
                "--set",
                "id=1,2",
                "openapi.yaml"
            ]
        );
        assert!(command_for("run", &json!({}))
            .unwrap_err()
            .contains("url is required"));
        assert!(command_for("ping", &json!({"targets": []}))
            .unwrap_err()
            .contains("targets is required"));
        assert!(command_for("nope", &json!({}))
            .unwrap_err()
            .contains("no tool named"));
    }

    #[tokio::test]
    async fn the_protocol_answers_initialize_list_and_unknowns() {
        let guard = vec!["--allow-host".to_string(), ".x.test".to_string()];
        let init = handle(&guard, &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}}))
            .await
            .unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(init["result"]["serverInfo"]["name"], "pepe");
        let old = handle(&guard, &json!({"jsonrpc": "2.0", "id": 2, "method": "initialize", "params": {"protocolVersion": "1999-01-01"}}))
            .await
            .unwrap();
        assert_eq!(old["result"]["protocolVersion"], "2025-03-26");
        assert!(handle(
            &guard,
            &json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
        )
        .await
        .is_none());
        let list = handle(
            &guard,
            &json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list"}),
        )
        .await
        .unwrap();
        let names: Vec<&str> = list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            ["run", "ping", "ramp", "api", "flow", "replay", "compare"]
        );
        for tool in list["result"]["tools"].as_array().unwrap() {
            assert_eq!(tool["inputSchema"]["type"], "object", "{}", tool["name"]);
        }
        let unknown = handle(
            &guard,
            &json!({"jsonrpc": "2.0", "id": 4, "method": "nope"}),
        )
        .await
        .unwrap();
        assert_eq!(unknown["error"]["code"], -32601);
        let bad_call = handle(&guard, &json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "run", "arguments": {}}}))
            .await
            .unwrap();
        assert_eq!(bad_call["result"]["isError"], true);
        assert!(bad_call["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("url is required"));
        assert_eq!(
            handle(
                &guard,
                &json!({"jsonrpc": "2.0", "id": 6, "method": "ping"})
            )
            .await
            .unwrap()["result"],
            json!({})
        );
    }

    #[test]
    fn the_guard_goes_on_every_call() {
        use clap::Parser;
        let cli = Cli::parse_from([
            "pepe",
            "--allow-host",
            ".x.test",
            "--max-requests",
            "100",
            "--max-rate",
            "50",
            "--max-concurrency",
            "8",
            "mcp",
        ]);
        assert_eq!(
            guard_args(&cli),
            [
                "--allow-host",
                ".x.test",
                "--max-requests",
                "100",
                "--max-rate",
                "50",
                "--max-concurrency",
                "8"
            ]
        );
    }
}
