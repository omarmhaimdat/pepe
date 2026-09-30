//! Turns a curl command into the request pepe sends.
//!
//! Two steps: split the command into words the way a shell would (quotes,
//! escapes, line continuations, bash `$'...'`, Windows `^` escaping), then
//! read curl's options with curl's own rules for methods, bodies and
//! headers.

use std::path::Path;

/// A curl command, as the request pepe will send
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CurlRequest {
    pub url: String,
    pub method: String,
    /// "Name: value", in the order curl would send them
    pub headers: Vec<String>,
    pub body: Option<Vec<u8>>,
    pub user_agent: Option<String>,
    pub proxy: Option<String>,
    pub insecure: bool,
    pub follow_redirects: bool,
    pub timeout_secs: Option<u32>,
    pub no_keepalive: bool,
    /// Parts of the command pepe understood but can't honor, for the user
    pub notes: Vec<String>,
}

/// Parse a whole curl command given as text
pub fn parse_command(command: &str) -> Result<CurlRequest, String> {
    parse_words(&split(command)?)
}

// ─── Shell words ─────────────────────────────────────────────────────────────

/// Split a command into words like a POSIX shell (without expansions).
/// Windows cmd-style commands (`^"` quoting, `^` line endings) follow cmd's
/// rules instead.
pub fn split(input: &str) -> Result<Vec<String>, String> {
    if is_cmd_style(input) {
        return split_cmd(&unescape_cmd(input));
    }
    let chars: Vec<char> = input.chars().collect();
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            ' ' | '\t' | '\n' | '\r' => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
                i += 1;
            }
            // A comment runs to the end of the line
            '#' if !in_word => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '\\' => match (chars.get(i + 1), chars.get(i + 2)) {
                // Line continuation
                (Some('\n'), _) => i += 2,
                (Some('\r'), Some('\n')) => i += 3,
                (Some(&c), _) => {
                    word.push(c);
                    in_word = true;
                    i += 2;
                }
                (None, _) => i += 1,
            },
            '\'' => {
                in_word = true;
                i += 1;
                loop {
                    match chars.get(i) {
                        None => return Err("unterminated ' quote in the curl command".into()),
                        Some('\'') => break,
                        Some(&c) => word.push(c),
                    }
                    i += 1;
                }
                i += 1;
            }
            '"' => {
                in_word = true;
                i += 1;
                loop {
                    match (chars.get(i), chars.get(i + 1)) {
                        (None, _) => return Err("unterminated \" quote in the curl command".into()),
                        (Some('"'), _) => break,
                        // Inside double quotes a backslash only escapes these
                        (Some('\\'), Some(&c @ ('"' | '\\' | '$' | '`'))) => {
                            word.push(c);
                            i += 1;
                        }
                        (Some('\\'), Some('\n')) => i += 1,
                        (Some('\\'), Some('\r')) if chars.get(i + 2) == Some(&'\n') => i += 2,
                        (Some(&c), _) => word.push(c),
                    }
                    i += 1;
                }
                i += 1;
            }
            // bash ANSI-C quoting, as Chrome's "Copy as cURL" uses
            '$' if chars.get(i + 1) == Some(&'\'') => {
                in_word = true;
                i = ansi_c_quoted(&chars, i + 2, &mut word)?;
            }
            c => {
                word.push(c);
                in_word = true;
                i += 1;
            }
        }
    }
    if in_word {
        words.push(word);
    }
    Ok(words)
}

/// Read a `$'...'` string starting at `i` (just past the opening quote) into
/// `out`; returns the index after the closing quote
fn ansi_c_quoted(chars: &[char], mut i: usize, out: &mut String) -> Result<usize, String> {
    let hex = |i: usize, max: usize| {
        let digits: String = chars[i..]
            .iter()
            .take(max)
            .take_while(|c| c.is_ascii_hexdigit())
            .collect();
        (u32::from_str_radix(&digits, 16).ok(), digits.len())
    };
    loop {
        let Some(&c) = chars.get(i) else {
            return Err("unterminated $' quote in the curl command".into());
        };
        i += 1;
        if c == '\'' {
            return Ok(i);
        }
        if c != '\\' {
            out.push(c);
            continue;
        }
        let Some(&e) = chars.get(i) else { continue };
        i += 1;
        let simple = match e {
            'a' => Some('\x07'),
            'b' => Some('\x08'),
            'e' | 'E' => Some('\x1b'),
            'f' => Some('\x0c'),
            'n' => Some('\n'),
            'r' => Some('\r'),
            't' => Some('\t'),
            'v' => Some('\x0b'),
            '\\' | '\'' | '"' | '?' => Some(e),
            _ => None,
        };
        if let Some(s) = simple {
            out.push(s);
            continue;
        }
        let (code, used) = match e {
            'x' => hex(i, 2),
            'u' => hex(i, 4),
            'U' => hex(i, 8),
            '0'..='7' => {
                let digits: String = std::iter::once(e)
                    .chain(
                        chars[i..]
                            .iter()
                            .copied()
                            .take(2)
                            .take_while(|c| ('0'..='7').contains(c)),
                    )
                    .collect();
                (u32::from_str_radix(&digits, 8).ok(), digits.len() - 1)
            }
            'c' => (chars.get(i).map(|c| (*c as u32) & 0x1f), 1),
            _ => {
                // Unknown escape: kept as written
                out.push('\\');
                out.push(e);
                continue;
            }
        };
        i += used;
        if let Some(c) = code.and_then(char::from_u32) {
            out.push(c);
        }
    }
}

/// Split a cmd command line (after `^` unescaping) the way Windows programs
/// read their arguments: only double quotes group, and backslashes are
/// literal unless they come before a quote, so paths like `C:\dir\file`
/// survive
fn split_cmd(input: &str) -> Result<Vec<String>, String> {
    let chars: Vec<char> = input.chars().collect();
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut quoted = false;
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            ' ' | '\t' | '\n' | '\r' if !quoted => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
                i += 1;
            }
            '\\' => {
                let run = chars[i..].iter().take_while(|&&c| c == '\\').count();
                i += run;
                if chars.get(i) == Some(&'"') {
                    // 2n backslashes and a quote: n backslashes, then the
                    // quote toggles; 2n+1: n backslashes and a literal quote
                    word.extend(std::iter::repeat_n('\\', run / 2));
                    if run % 2 == 1 {
                        word.push('"');
                        i += 1;
                    }
                } else {
                    word.extend(std::iter::repeat_n('\\', run));
                }
                in_word = true;
            }
            '"' => {
                quoted = !quoted;
                in_word = true;
                i += 1;
            }
            c => {
                word.push(c);
                in_word = true;
                i += 1;
            }
        }
    }
    if quoted {
        return Err("unterminated \" quote in the curl command".into());
    }
    if in_word {
        words.push(word);
    }
    Ok(words)
}

/// Windows cmd copies quote with `^"` and end lines with ` ^`
fn is_cmd_style(input: &str) -> bool {
    input.contains("^\"") || input.lines().any(|l| l.trim_end().ends_with(" ^"))
}

/// Undo cmd's `^` escaping: `^X` is `X`, `^` at the end of a line joins it
/// with the next
fn unescape_cmd(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '^' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('\r') => {
                chars.next_if_eq(&'\n');
            }
            Some('\n') | None => {}
            Some(next) => out.push(next),
        }
    }
    out
}

// ─── curl options ────────────────────────────────────────────────────────────

/// Whether an option takes a value, and what pepe does with it
#[derive(Clone, Copy, PartialEq)]
enum Arg {
    /// A flag pepe acts on
    Flag,
    /// An option with a value pepe acts on
    Value,
    /// A flag with no effect on the request (output, verbosity, TLS setup)
    IgnoredFlag,
    /// An option with a value, with no effect on the request
    IgnoredValue,
}

use Arg::*;

/// curl's options: short name, long name, kind. Options pepe acts on come
/// first; the rest are accepted so real commands parse, and have no effect.
const OPTIONS: &[(Option<char>, &str, Arg)] = &[
    (Some('X'), "request", Value),
    (Some('H'), "header", Value),
    (Some('d'), "data", Value),
    (None, "data-ascii", Value),
    (None, "data-raw", Value),
    (None, "data-binary", Value),
    (None, "data-urlencode", Value),
    (None, "json", Value),
    (Some('F'), "form", Value),
    (None, "form-string", Value),
    (Some('G'), "get", Flag),
    (Some('I'), "head", Flag),
    (Some('T'), "upload-file", Value),
    (Some('u'), "user", Value),
    (None, "oauth2-bearer", Value),
    (Some('A'), "user-agent", Value),
    (Some('e'), "referer", Value),
    (Some('b'), "cookie", Value),
    (Some('r'), "range", Value),
    (Some('x'), "proxy", Value),
    (Some('k'), "insecure", Flag),
    (Some('L'), "location", Flag),
    (None, "location-trusted", Flag),
    (Some('m'), "max-time", Value),
    (None, "url", Value),
    (None, "url-query", Value),
    (None, "compressed", Flag),
    (None, "no-keepalive", Flag),
    (Some('g'), "globoff", Flag),
    // No effect on the request itself
    (Some('s'), "silent", IgnoredFlag),
    (Some('S'), "show-error", IgnoredFlag),
    (Some('v'), "verbose", IgnoredFlag),
    (Some('i'), "include", IgnoredFlag),
    (None, "show-headers", IgnoredFlag),
    (Some('f'), "fail", IgnoredFlag),
    (None, "fail-with-body", IgnoredFlag),
    (None, "fail-early", IgnoredFlag),
    (Some('O'), "remote-name", IgnoredFlag),
    (None, "remote-name-all", IgnoredFlag),
    (Some('J'), "remote-header-name", IgnoredFlag),
    (Some('#'), "progress-bar", IgnoredFlag),
    (None, "no-progress-meter", IgnoredFlag),
    (Some('N'), "no-buffer", IgnoredFlag),
    (Some('q'), "disable", IgnoredFlag),
    (None, "create-dirs", IgnoredFlag),
    (Some('R'), "remote-time", IgnoredFlag),
    (None, "raw", IgnoredFlag),
    (None, "tr-encoding", IgnoredFlag),
    (None, "path-as-is", IgnoredFlag),
    (Some('4'), "ipv4", IgnoredFlag),
    (Some('6'), "ipv6", IgnoredFlag),
    (None, "retry-all-errors", IgnoredFlag),
    (None, "retry-connrefused", IgnoredFlag),
    (Some('j'), "junk-session-cookies", IgnoredFlag),
    (Some('n'), "netrc", IgnoredFlag),
    (None, "netrc-optional", IgnoredFlag),
    (None, "basic", IgnoredFlag),
    (None, "digest", IgnoredFlag),
    (None, "ntlm", IgnoredFlag),
    (None, "ntlm-wb", IgnoredFlag),
    (None, "negotiate", IgnoredFlag),
    (None, "anyauth", IgnoredFlag),
    (None, "http0.9", IgnoredFlag),
    (Some('0'), "http1.0", IgnoredFlag),
    (None, "http1.1", IgnoredFlag),
    (None, "http2", IgnoredFlag),
    (None, "http2-prior-knowledge", IgnoredFlag),
    (None, "http3", IgnoredFlag),
    (None, "http3-only", IgnoredFlag),
    (Some('1'), "tlsv1", IgnoredFlag),
    (None, "tlsv1.0", IgnoredFlag),
    (None, "tlsv1.1", IgnoredFlag),
    (None, "tlsv1.2", IgnoredFlag),
    (None, "tlsv1.3", IgnoredFlag),
    (Some('2'), "sslv2", IgnoredFlag),
    (Some('3'), "sslv3", IgnoredFlag),
    (None, "ssl", IgnoredFlag),
    (None, "ssl-reqd", IgnoredFlag),
    (None, "ssl-no-revoke", IgnoredFlag),
    (None, "ssl-revoke-best-effort", IgnoredFlag),
    (None, "ssl-allow-beast", IgnoredFlag),
    (None, "ssl-auto-client-cert", IgnoredFlag),
    (None, "cert-status", IgnoredFlag),
    (None, "ca-native", IgnoredFlag),
    (None, "false-start", IgnoredFlag),
    (None, "tcp-nodelay", IgnoredFlag),
    (None, "tcp-fastopen", IgnoredFlag),
    (None, "styled-output", IgnoredFlag),
    (Some('l'), "list-only", IgnoredFlag),
    (Some('B'), "use-ascii", IgnoredFlag),
    (Some('a'), "append", IgnoredFlag),
    (Some('M'), "manual", IgnoredFlag),
    (Some('V'), "version", IgnoredFlag),
    (Some('Z'), "parallel", IgnoredFlag),
    (None, "parallel-immediate", IgnoredFlag),
    (None, "post301", IgnoredFlag),
    (None, "post302", IgnoredFlag),
    (None, "post303", IgnoredFlag),
    (None, "compressed-ssh", IgnoredFlag),
    (None, "ignore-content-length", IgnoredFlag),
    (None, "no-alpn", IgnoredFlag),
    (None, "no-npn", IgnoredFlag),
    (None, "no-sessionid", IgnoredFlag),
    (None, "xattr", IgnoredFlag),
    (Some('p'), "proxytunnel", IgnoredFlag),
    (None, "proxy-insecure", IgnoredFlag),
    (None, "proxy-basic", IgnoredFlag),
    (None, "proxy-digest", IgnoredFlag),
    (None, "proxy-ntlm", IgnoredFlag),
    (None, "proxy-negotiate", IgnoredFlag),
    (None, "proxy-anyauth", IgnoredFlag),
    (None, "proxy-ca-native", IgnoredFlag),
    (None, "proxy-ssl-allow-beast", IgnoredFlag),
    (None, "proxy-ssl-auto-client-cert", IgnoredFlag),
    (None, "socks5-gssapi-nec", IgnoredFlag),
    (None, "disallow-username-in-url", IgnoredFlag),
    (None, "doh-insecure", IgnoredFlag),
    (None, "doh-cert-status", IgnoredFlag),
    (None, "sasl-ir", IgnoredFlag),
    (None, "haproxy-protocol", IgnoredFlag),
    (None, "mptcp", IgnoredFlag),
    (None, "suppress-connect-headers", IgnoredFlag),
    (None, "trace-time", IgnoredFlag),
    (None, "trace-ids", IgnoredFlag),
    (None, "crlf", IgnoredFlag),
    (None, "ftp-create-dirs", IgnoredFlag),
    (None, "ftp-pasv", IgnoredFlag),
    (None, "ftp-skip-pasv-ip", IgnoredFlag),
    (None, "ftp-ssl-ccc", IgnoredFlag),
    (None, "ftp-pret", IgnoredFlag),
    (None, "ftp-ssl-control", IgnoredFlag),
    (None, "disable-eprt", IgnoredFlag),
    (None, "disable-epsv", IgnoredFlag),
    (None, "remove-on-error", IgnoredFlag),
    (Some('o'), "output", IgnoredValue),
    (None, "output-dir", IgnoredValue),
    (Some('w'), "write-out", IgnoredValue),
    (Some('D'), "dump-header", IgnoredValue),
    (None, "trace", IgnoredValue),
    (None, "trace-ascii", IgnoredValue),
    (None, "trace-config", IgnoredValue),
    (None, "stderr", IgnoredValue),
    (None, "retry", IgnoredValue),
    (None, "retry-delay", IgnoredValue),
    (None, "retry-max-time", IgnoredValue),
    (None, "keepalive-time", IgnoredValue),
    (None, "keepalive-cnt", IgnoredValue),
    (Some('c'), "cookie-jar", IgnoredValue),
    (None, "limit-rate", IgnoredValue),
    (None, "cacert", IgnoredValue),
    (None, "capath", IgnoredValue),
    (Some('E'), "cert", IgnoredValue),
    (None, "key", IgnoredValue),
    (None, "cert-type", IgnoredValue),
    (None, "key-type", IgnoredValue),
    (None, "pass", IgnoredValue),
    (None, "pinnedpubkey", IgnoredValue),
    (None, "crlfile", IgnoredValue),
    (None, "ciphers", IgnoredValue),
    (None, "tls13-ciphers", IgnoredValue),
    (None, "curves", IgnoredValue),
    (None, "tls-max", IgnoredValue),
    (None, "tlsuser", IgnoredValue),
    (None, "tlspassword", IgnoredValue),
    (None, "tlsauthtype", IgnoredValue),
    (None, "resolve", IgnoredValue),
    (None, "connect-to", IgnoredValue),
    (None, "interface", IgnoredValue),
    (None, "local-port", IgnoredValue),
    (None, "dns-servers", IgnoredValue),
    (None, "dns-interface", IgnoredValue),
    (None, "dns-ipv4-addr", IgnoredValue),
    (None, "dns-ipv6-addr", IgnoredValue),
    (None, "doh-url", IgnoredValue),
    (Some('Y'), "speed-limit", IgnoredValue),
    (Some('y'), "speed-time", IgnoredValue),
    (None, "noproxy", IgnoredValue),
    (None, "preproxy", IgnoredValue),
    (Some('U'), "proxy-user", IgnoredValue),
    (None, "proxy-header", IgnoredValue),
    (None, "proxy-cacert", IgnoredValue),
    (None, "proxy-capath", IgnoredValue),
    (None, "proxy-cert", IgnoredValue),
    (None, "proxy-key", IgnoredValue),
    (None, "proxy-cert-type", IgnoredValue),
    (None, "proxy-key-type", IgnoredValue),
    (None, "proxy-pass", IgnoredValue),
    (None, "proxy-ciphers", IgnoredValue),
    (None, "proxy-tls13-ciphers", IgnoredValue),
    (None, "proxy-service-name", IgnoredValue),
    (None, "proxy-pinnedpubkey", IgnoredValue),
    (None, "proxy-crlfile", IgnoredValue),
    (None, "proxy-tlsuser", IgnoredValue),
    (None, "proxy-tlspassword", IgnoredValue),
    (None, "proxy-tlsauthtype", IgnoredValue),
    (None, "proxy1.0", IgnoredValue),
    (None, "socks4", IgnoredValue),
    (None, "socks4a", IgnoredValue),
    (None, "socks5", IgnoredValue),
    (None, "socks5-hostname", IgnoredValue),
    (None, "socks5-gssapi-service", IgnoredValue),
    (None, "service-name", IgnoredValue),
    (None, "unix-socket", IgnoredValue),
    (None, "abstract-unix-socket", IgnoredValue),
    (Some('K'), "config", IgnoredValue),
    (None, "netrc-file", IgnoredValue),
    (None, "max-redirs", IgnoredValue),
    (None, "max-filesize", IgnoredValue),
    (Some('z'), "time-cond", IgnoredValue),
    (None, "expect100-timeout", IgnoredValue),
    (None, "happy-eyeballs-timeout-ms", IgnoredValue),
    (None, "connect-timeout", IgnoredValue),
    (Some('C'), "continue-at", IgnoredValue),
    (None, "libcurl", IgnoredValue),
    (Some('P'), "ftp-port", IgnoredValue),
    (Some('Q'), "quote", IgnoredValue),
    (Some('t'), "telnet-option", IgnoredValue),
    (None, "mail-from", IgnoredValue),
    (None, "mail-rcpt", IgnoredValue),
    (None, "mail-auth", IgnoredValue),
    (None, "login-options", IgnoredValue),
    (None, "sasl-authzid", IgnoredValue),
    (None, "delegation", IgnoredValue),
    (None, "hsts", IgnoredValue),
    (None, "alt-svc", IgnoredValue),
    (None, "etag-save", IgnoredValue),
    (None, "etag-compare", IgnoredValue),
    (None, "parallel-max", IgnoredValue),
    (None, "rate", IgnoredValue),
    (None, "variable", IgnoredValue),
    (None, "request-target", IgnoredValue),
    (None, "krb", IgnoredValue),
    (None, "ftp-account", IgnoredValue),
    (None, "ftp-alternative-to-user", IgnoredValue),
    (None, "ftp-method", IgnoredValue),
    (None, "ftp-ssl-ccc-mode", IgnoredValue),
    (None, "tftp-blksize", IgnoredValue),
    (None, "ip-tos", IgnoredValue),
    (None, "vlan-priority", IgnoredValue),
    (None, "ech", IgnoredValue),
];

fn by_long(name: &str) -> Option<(&'static str, Arg)> {
    OPTIONS
        .iter()
        .find(|(_, long, _)| *long == name)
        .map(|&(_, long, arg)| (long, arg))
}

fn by_short(c: char) -> Option<(&'static str, Arg)> {
    OPTIONS
        .iter()
        .find(|(short, _, _)| *short == Some(c))
        .map(|&(_, long, arg)| (long, arg))
}

/// One piece of request body, and whether it joins the previous one with `&`
struct Data {
    bytes: Vec<u8>,
    json: bool,
}

/// One `-F` field
struct FormPart {
    name: String,
    value: Vec<u8>,
    filename: Option<String>,
    content_type: Option<String>,
}

/// Everything the options said, before it's turned into a request
#[derive(Default)]
struct Options {
    urls: Vec<String>,
    method: Option<String>,
    /// (name, value); no value means "don't send this header"
    headers: Vec<(String, Option<String>)>,
    data: Vec<Data>,
    form: Vec<FormPart>,
    upload: Option<(Vec<u8>, String)>,
    queries: Vec<String>,
    get: bool,
    head: bool,
    user: Option<String>,
    bearer: Option<String>,
    user_agent: Option<String>,
    referer: Option<String>,
    cookies: Vec<String>,
    range: Option<String>,
    proxy: Option<String>,
    insecure: bool,
    follow: bool,
    timeout: Option<u32>,
    compressed: bool,
    no_keepalive: bool,
    globoff: bool,
    notes: Vec<String>,
}

/// Parse curl's words (the leading `curl` is optional)
pub fn parse_words(words: &[String]) -> Result<CurlRequest, String> {
    let mut words = words.iter().map(String::as_str).peekable();
    // A pasted prompt, then the curl program itself
    words.next_if(|w| *w == "$");
    words.next_if(|w| {
        let w = w.to_ascii_lowercase();
        w == "curl" || w == "curl.exe" || w.ends_with("/curl") || w.ends_with("\\curl.exe")
    });

    let mut o = Options::default();
    let mut only_urls = false;
    while let Some(word) = words.next() {
        if only_urls || !word.starts_with('-') || word == "-" {
            o.urls.push(word.to_string());
            continue;
        }
        if word == "--" {
            only_urls = true;
            continue;
        }
        if let Some(long) = word.strip_prefix("--") {
            let (name, arg) = resolve_long(long, &mut o)?;
            let value = match arg {
                Value | IgnoredValue => Some(
                    words
                        .next()
                        .ok_or_else(|| format!("curl option --{long} needs a value"))?
                        .to_string(),
                ),
                _ => None,
            };
            apply(
                &mut o,
                name,
                arg,
                value,
                !long.starts_with("no-") || name.starts_with("no-"),
            )?;
            continue;
        }
        // Short options can be bunched (-sSL) and take their value attached
        // (-XPOST, -d@file) or as the next word
        let cluster: Vec<char> = word[1..].chars().collect();
        let mut i = 0;
        while i < cluster.len() {
            let c = cluster[i];
            let (name, arg) = by_short(c).ok_or_else(|| format!("unknown curl option -{c}"))?;
            let value = match arg {
                Value | IgnoredValue => {
                    let rest: String = cluster[i + 1..].iter().collect();
                    i = cluster.len();
                    Some(if rest.is_empty() {
                        words
                            .next()
                            .ok_or_else(|| format!("curl option -{c} needs a value"))?
                            .to_string()
                    } else {
                        rest
                    })
                }
                _ => {
                    i += 1;
                    None
                }
            };
            apply(&mut o, name, arg, value, true)?;
        }
    }
    build(o)
}

/// Find a long option; also handles `--no-<flag>` and curl 8's
/// `--expand-<option>` forms
fn resolve_long(long: &str, o: &mut Options) -> Result<(&'static str, Arg), String> {
    if let Some(found) = by_long(long) {
        return Ok(found);
    }
    if let Some(base) = long.strip_prefix("no-") {
        if let Some((name, arg @ (Flag | IgnoredFlag))) = by_long(base) {
            return Ok((name, arg));
        }
    }
    if let Some(base) = long.strip_prefix("expand-") {
        if let Some(found) = by_long(base) {
            o.notes.push(format!(
                "--{long}: curl variables aren't expanded, the value is used as written"
            ));
            return Ok(found);
        }
    }
    Err(format!("unknown curl option --{long}"))
}

/// Record one option. `on` is false for `--no-<flag>`.
fn apply(
    o: &mut Options,
    name: &str,
    arg: Arg,
    value: Option<String>,
    on: bool,
) -> Result<(), String> {
    if matches!(arg, IgnoredFlag | IgnoredValue) {
        return Ok(());
    }
    let v = value.unwrap_or_default();
    match name {
        "request" => o.method = Some(v),
        "header" => {
            if let Some(path) = v.strip_prefix('@') {
                for line in read_text(path)?.lines().filter(|l| !l.trim().is_empty()) {
                    header(o, line);
                }
            } else {
                header(o, &v);
            }
        }
        "data" | "data-ascii" => {
            let bytes = match v.strip_prefix('@') {
                // Like curl, newlines in a data file are dropped
                Some(path) => read_file(path)?
                    .into_iter()
                    .filter(|b| *b != b'\n' && *b != b'\r')
                    .collect(),
                None => v.into_bytes(),
            };
            o.data.push(Data { bytes, json: false });
        }
        "data-raw" => o.data.push(Data {
            bytes: v.into_bytes(),
            json: false,
        }),
        "data-binary" => {
            let bytes = match v.strip_prefix('@') {
                Some(path) => read_file(path)?,
                None => v.into_bytes(),
            };
            o.data.push(Data { bytes, json: false });
        }
        "data-urlencode" => o.data.push(Data {
            bytes: url_encode_field(&v)?.into_bytes(),
            json: false,
        }),
        "json" => {
            let bytes = match v.strip_prefix('@') {
                Some(path) => read_file(path)?,
                None => v.into_bytes(),
            };
            o.data.push(Data { bytes, json: true });
        }
        "form" => o.form.push(form_part(&v, true)?),
        "form-string" => o.form.push(form_part(&v, false)?),
        "get" => o.get = on,
        "head" => o.head = on,
        "upload-file" => {
            if v == "-" || v == "." {
                return Err("-T - (upload from stdin) isn't supported; give a file".into());
            }
            let name = Path::new(&v)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            o.upload = Some((read_file(&v)?, name));
        }
        "user" => o.user = Some(v),
        "oauth2-bearer" => o.bearer = Some(v),
        "user-agent" => o.user_agent = Some(v),
        "referer" => o.referer = Some(v.trim_end_matches(";auto").to_string()),
        "cookie" => {
            if v.contains('=') {
                o.cookies.push(v);
            } else {
                o.notes.push(format!(
                    "-b {v}: reading cookies from a file isn't supported, ignored"
                ));
            }
        }
        "range" => o.range = Some(v),
        "proxy" => o.proxy = Some(v),
        "insecure" => o.insecure = on,
        "location" | "location-trusted" => o.follow = on,
        "max-time" => {
            let secs: f64 = v
                .parse()
                .map_err(|_| format!("-m {v}: not a number of seconds"))?;
            let whole = secs.ceil().max(1.0);
            if whole > 120.0 {
                o.notes.push(format!(
                    "-m {v}: pepe's timeout is at most 120s, using 120s"
                ));
            }
            o.timeout = Some(whole.min(120.0) as u32);
        }
        "url" => o.urls.push(v),
        "url-query" => o.queries.push(match v.strip_prefix('+') {
            Some(raw) => raw.to_string(),
            None => url_encode_field(&v)?,
        }),
        "compressed" => o.compressed = on,
        "no-keepalive" => o.no_keepalive = true,
        "globoff" => o.globoff = on,
        _ => unreachable!("option {name} has no handler"),
    }
    Ok(())
}

/// `-H`: "Name: value" sends it, "Name:" stops curl sending its own,
/// "Name;" sends it empty
fn header(o: &mut Options, h: &str) {
    if let Some((name, value)) = h.split_once(':') {
        let value = value.trim();
        o.headers.push((
            name.trim().to_string(),
            (!value.is_empty()).then(|| value.to_string()),
        ));
    } else if let Some(name) = h.trim().strip_suffix(';') {
        o.headers
            .push((name.trim().to_string(), Some(String::new())));
    } else {
        o.notes.push(format!("-H {h:?}: not a header, ignored"));
    }
}

/// `--data-urlencode` forms: "content", "=content", "name=content",
/// "@file", "name@file"
fn url_encode_field(v: &str) -> Result<String, String> {
    if let Some(content) = v.strip_prefix('=') {
        return Ok(percent_encode(content.as_bytes()));
    }
    let at = v.find('@');
    let eq = v.find('=');
    match (at, eq) {
        (Some(a), e) if e.is_none_or(|e| a < e) => {
            let (name, path) = (&v[..a], &v[a + 1..]);
            let encoded = percent_encode(&read_file(path)?);
            Ok(if name.is_empty() {
                encoded
            } else {
                format!("{name}={encoded}")
            })
        }
        (_, Some(e)) => Ok(format!(
            "{}={}",
            &v[..e],
            percent_encode(&v.as_bytes()[e + 1..])
        )),
        _ => Ok(percent_encode(v.as_bytes())),
    }
}

/// Form-encode as curl does: unreserved characters stay, a space becomes
/// `+`, everything else is percent-encoded
fn percent_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else if b == b' ' {
            out.push('+');
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `-F name=value`, `name=@file;type=...;filename=...`, `name=<file`
fn form_part(v: &str, special: bool) -> Result<FormPart, String> {
    let (name, rest) = v
        .split_once('=')
        .ok_or_else(|| format!("-F {v:?}: expected name=content"))?;
    let mut part = FormPart {
        name: name.to_string(),
        value: Vec::new(),
        filename: None,
        content_type: None,
    };
    if !special {
        part.value = rest.as_bytes().to_vec();
        return Ok(part);
    }
    // A quoted value may contain ';'; after it come ;type= and ;filename=
    let (value, params) = if let Some(quoted) = rest.strip_prefix('"') {
        let mut value = String::new();
        let mut chars = quoted.char_indices();
        let mut end = quoted.len();
        while let Some((i, c)) = chars.next() {
            match c {
                '\\' => value.extend(chars.next().map(|(_, c)| c)),
                '"' => {
                    end = i + 1;
                    break;
                }
                c => value.push(c),
            }
        }
        (value, &quoted[end..])
    } else {
        let (value, params) = rest.split_once(';').unwrap_or((rest, ""));
        (value.to_string(), params)
    };
    for param in params.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        if let Some(t) = param.strip_prefix("type=") {
            part.content_type = Some(t.to_string());
        } else if let Some(f) = param.strip_prefix("filename=") {
            part.filename = Some(f.trim_matches('"').to_string());
        }
    }
    if let Some(path) = value.strip_prefix('@') {
        part.value = read_file(path)?;
        part.filename.get_or_insert_with(|| {
            Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
        part.content_type
            .get_or_insert_with(|| guess_type(path).to_string());
    } else if let Some(path) = value.strip_prefix('<') {
        part.value = read_file(path)?;
    } else {
        part.value = value.into_bytes();
    }
    Ok(part)
}

fn guess_type(path: &str) -> &'static str {
    let ext = Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "json" => "application/json",
        "txt" | "log" => "text/plain",
        "html" | "htm" => "text/html",
        "xml" => "application/xml",
        "csv" => "text/csv",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

fn read_file(path: &str) -> Result<Vec<u8>, String> {
    if path == "-" {
        return Err("reading curl data from stdin (@-) isn't supported; give a file".into());
    }
    std::fs::read(path).map_err(|e| format!("can't read {path}: {e}"))
}

fn read_text(path: &str) -> Result<String, String> {
    Ok(String::from_utf8_lossy(&read_file(path)?).into_owned())
}

fn base64(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | ((b as u32) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Turn the options into the request curl would send
fn build(mut o: Options) -> Result<CurlRequest, String> {
    if o.urls.is_empty() {
        return Err("no URL in the curl command".into());
    }
    let mut url = o.urls.remove(0);
    for extra in &o.urls {
        o.notes.push(format!(
            "curl would also request {extra}; pepe loads the first URL only"
        ));
    }
    // curl's default scheme
    if !url.contains("://") {
        url = format!("http://{url}");
    }
    if !o.globoff && (url.contains('{') || url.contains("[") && !url.contains("://[")) {
        o.notes
            .push("curl would expand the URL's {…} or […] globs; pepe sends it as written".into());
    }
    if !o.form.is_empty() && !o.data.is_empty() {
        return Err("curl can't combine -F with -d/--data options".into());
    }

    // -G moves the data into the query string
    let mut queries = std::mem::take(&mut o.queries);
    if o.get && !o.data.is_empty() {
        queries.push(String::from_utf8_lossy(&join_data(&o.data)).into_owned());
        o.data.clear();
    }
    if !queries.is_empty() {
        let (base, fragment) = url
            .split_once('#')
            .map_or((url.as_str(), None), |(b, f)| (b, Some(f)));
        let sep = if base.contains('?') { "&" } else { "?" };
        url = format!(
            "{base}{sep}{}{}",
            queries.join("&"),
            fragment.map_or(String::new(), |f| format!("#{f}"))
        );
    }

    // Uploading to a URL ending in / names the file after the local one
    if let Some((_, name)) = &o.upload {
        if url.ends_with('/') {
            url.push_str(name);
        }
    }

    let json = o.data.iter().any(|d| d.json);
    let uploading = o.upload.is_some();
    let (body, content_type) = if o.head {
        (None, None)
    } else if !o.form.is_empty() {
        let boundary = format!("------------------------pepe{:016x}", boundary_seed());
        (
            Some(multipart(&o.form, &boundary)),
            Some(format!("multipart/form-data; boundary={boundary}")),
        )
    } else if let Some((bytes, _)) = o.upload.take() {
        (Some(bytes), None)
    } else if !o.data.is_empty() {
        let ct = if json {
            "application/json"
        } else {
            "application/x-www-form-urlencoded"
        };
        (Some(join_data(&o.data)), Some(ct.to_string()))
    } else {
        (None, None)
    };

    let method = o.method.clone().unwrap_or_else(|| {
        if o.head {
            "HEAD"
        } else if o.get {
            "GET"
        } else if uploading {
            "PUT"
        } else if body.is_some() {
            "POST"
        } else {
            "GET"
        }
        .to_string()
    });

    // Headers the user gave, then the ones curl adds for them; a header the
    // user set (or cleared with "Name:") replaces curl's own
    let mut implied: Vec<(String, String)> = Vec::new();
    if let Some(user) = &o.user {
        if !user.contains(':') {
            o.notes.push(
                "-u without a password: curl would prompt for one; using an empty password".into(),
            );
        }
        let creds = if user.contains(':') {
            user.clone()
        } else {
            format!("{user}:")
        };
        implied.push((
            "Authorization".into(),
            format!("Basic {}", base64(creds.as_bytes())),
        ));
    }
    if let Some(token) = &o.bearer {
        implied.push(("Authorization".into(), format!("Bearer {token}")));
    }
    if !o.cookies.is_empty() {
        implied.push(("Cookie".into(), o.cookies.join("; ")));
    }
    if let Some(referer) = &o.referer {
        implied.push(("Referer".into(), referer.clone()));
    }
    if let Some(range) = &o.range {
        implied.push(("Range".into(), format!("bytes={range}")));
    }
    if let Some(ct) = &content_type {
        let multipart = ct.starts_with("multipart/");
        // A user's multipart Content-Type still needs curl's boundary
        let user_ct = o
            .headers
            .iter_mut()
            .find(|(n, _)| n.eq_ignore_ascii_case("content-type"));
        match user_ct {
            Some((_, Some(v))) if multipart && !v.contains("boundary=") => {
                v.push_str(&ct["multipart/form-data".len()..]);
            }
            Some(_) => {}
            None => implied.push(("Content-Type".into(), ct.clone())),
        }
    }
    if json {
        implied.push(("Accept".into(), "application/json".into()));
    }
    if o.compressed {
        implied.push(("Accept-Encoding".into(), "deflate, gzip, br, zstd".into()));
        o.notes.push("--compressed: responses come back compressed and pepe measures them as they arrive, without decompressing".into());
    }

    let mut user_agent = o.user_agent.clone();
    let mut headers = Vec::new();
    for (name, value) in &o.headers {
        match value {
            // User-Agent is a client setting in pepe, so it can't be sent twice
            Some(v) if name.eq_ignore_ascii_case("user-agent") => user_agent = Some(v.clone()),
            Some(v) => headers.push(format!("{name}: {v}")),
            None if name.eq_ignore_ascii_case("user-agent") => o
                .notes
                .push("-H 'User-Agent:': pepe always sends a User-Agent".into()),
            None => {}
        }
    }
    let given = |name: &str| o.headers.iter().any(|(n, _)| n.eq_ignore_ascii_case(name));
    for (name, value) in implied {
        if !given(&name) {
            headers.push(format!("{name}: {value}"));
        }
    }

    let proxy = o.proxy.map(|p| {
        if p.contains("://") {
            p
        } else {
            format!("http://{p}")
        }
    });

    Ok(CurlRequest {
        url,
        method,
        headers,
        body,
        user_agent,
        proxy,
        insecure: o.insecure,
        follow_redirects: o.follow,
        timeout_secs: o.timeout,
        no_keepalive: o.no_keepalive,
        notes: o.notes,
    })
}

/// Data pieces joined as curl does: `&` between -d pieces, none for --json
fn join_data(data: &[Data]) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, d) in data.iter().enumerate() {
        if i > 0 && !d.json {
            out.push(b'&');
        }
        out.extend_from_slice(&d.bytes);
    }
    out
}

fn multipart(parts: &[FormPart], boundary: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for part in parts {
        out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        let mut disposition = format!("Content-Disposition: form-data; name=\"{}\"", part.name);
        if let Some(filename) = &part.filename {
            disposition.push_str(&format!("; filename=\"{filename}\""));
        }
        out.extend_from_slice(disposition.as_bytes());
        out.extend_from_slice(b"\r\n");
        if let Some(ct) = &part.content_type {
            out.extend_from_slice(format!("Content-Type: {ct}\r\n").as_bytes());
        }
        out.extend_from_slice(b"\r\n");
        out.extend_from_slice(&part.value);
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    out
}

fn boundary_seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(cmd: &str) -> CurlRequest {
        parse_command(cmd).unwrap_or_else(|e| panic!("{cmd}\n→ {e}"))
    }

    fn header<'a>(r: &'a CurlRequest, name: &str) -> Option<&'a str> {
        r.headers.iter().find_map(|h| {
            let (n, v) = h.split_once(": ")?;
            n.eq_ignore_ascii_case(name).then_some(v)
        })
    }

    fn body(r: &CurlRequest) -> String {
        String::from_utf8_lossy(r.body.as_deref().unwrap_or_default()).into_owned()
    }

    #[test]
    fn splits_like_a_shell() {
        let words = split(
            "curl 'a b' \"c \\\"d\\\" \\$e\" f\\ g $'h\\ni\\t\\x41\\u00e9' '' \\\n  -X POST # comment\n",
        )
        .unwrap();
        assert_eq!(
            words,
            [
                "curl",
                "a b",
                "c \"d\" $e",
                "f g",
                "h\ni\tAé",
                "",
                "-X",
                "POST"
            ]
        );
        assert!(split("curl 'open").is_err());
        assert!(split("curl \"open").is_err());
        assert_eq!(split("curl \\\r\n -I\r\n").unwrap(), ["curl", "-I"]);
        assert_eq!(
            split("a#b").unwrap(),
            ["a#b"],
            "# inside a word isn't a comment"
        );
    }

    #[test]
    fn windows_cmd_keeps_backslashes_in_paths() {
        let words = split(
            "curl ^\"https://x.io/up^\" ^\r\n  -F ^\"file=@C:\\Users\\ada\\it's.txt^\" ^\r\n  --data-raw ^\"^{^\\^\"a^\\^\":1^}^\"",
        )
        .unwrap();
        assert_eq!(
            words,
            [
                "curl",
                "https://x.io/up",
                "-F",
                r"file=@C:\Users\ada\it's.txt",
                "--data-raw",
                r#"{"a":1}"#
            ]
        );
        // cmd's argument rules for backslashes before quotes
        assert_eq!(split_cmd(r#"a\\"b c" d\"e"#).unwrap(), [r"a\b c", r#"d"e"#]);
        assert!(split_cmd("\"open").is_err());
    }

    #[test]
    fn plain_get_with_port_and_no_scheme() {
        let r = parse("curl localhost:8080/api/v1/items?id=4&sort=-name#top");
        assert_eq!(r.method, "GET");
        assert_eq!(
            r.url,
            "http://localhost:8080/api/v1/items?id=4&sort=-name#top"
        );
        assert!(r.body.is_none() && r.headers.is_empty());
        let r = parse("curl 'http://[::1]:3000/x' -I");
        assert_eq!(
            (r.method.as_str(), r.url.as_str()),
            ("HEAD", "http://[::1]:3000/x")
        );
    }

    #[test]
    fn chrome_copy_as_curl_bash() {
        let r = parse(
            r#"curl 'https://api.example.com/graphql' \
  -H 'accept: */*' \
  -H 'accept-language: en-US,en;q=0.9' \
  -H 'content-type: application/json' \
  -b 'session=abc123; theme=dark' \
  -H 'origin: https://example.com' \
  -H 'user-agent: Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)' \
  --data-raw $'{"query":"{ me { name } }","vars":"it\'s"}'"#,
        );
        assert_eq!(r.method, "POST");
        assert_eq!(body(&r), r#"{"query":"{ me { name } }","vars":"it's"}"#);
        assert_eq!(header(&r, "content-type"), Some("application/json"));
        assert_eq!(header(&r, "cookie"), Some("session=abc123; theme=dark"));
        assert_eq!(
            header(&r, "user-agent"),
            None,
            "moved to the user agent setting"
        );
        assert_eq!(
            r.user_agent.as_deref(),
            Some("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)")
        );
    }

    #[test]
    fn chrome_copy_as_curl_cmd() {
        let r = parse(
            "curl ^\"https://api.example.com/items?a=1^&b=2^\" ^\r\n  -H ^\"accept: application/json^\" ^\r\n  --data-raw ^\"^{^\\^\"name^\\^\":^\\^\"pepe^\\^\"^}^\" ^\r\n  --compressed",
        );
        assert_eq!(r.url, "https://api.example.com/items?a=1&b=2");
        assert_eq!(body(&r), r#"{"name":"pepe"}"#);
        assert_eq!(header(&r, "accept"), Some("application/json"));
        assert_eq!(
            header(&r, "accept-encoding"),
            Some("deflate, gzip, br, zstd")
        );
    }

    #[test]
    fn postman_and_insomnia_exports() {
        let r = parse(
            "curl --location --request PUT 'https://api.example.com/users/7' \\\n--header 'Authorization: Bearer t0k3n' \\\n--header 'Content-Type: application/json' \\\n--data '{\n    \"name\": \"Ada\"\n}'",
        );
        assert_eq!((r.method.as_str(), r.follow_redirects), ("PUT", true));
        assert_eq!(body(&r), "{\n    \"name\": \"Ada\"\n}");
        assert_eq!(header(&r, "content-type"), Some("application/json"));

        let r = parse("curl --request POST \\\n  --url https://api.example.com/login \\\n  --header 'content-type: application/x-www-form-urlencoded' \\\n  --data user=ada \\\n  --data pass=secret");
        assert_eq!(
            (r.method.as_str(), r.url.as_str()),
            ("POST", "https://api.example.com/login")
        );
        assert_eq!(body(&r), "user=ada&pass=secret");
    }

    #[test]
    fn bunched_and_attached_short_options() {
        let r = parse("curl -sSLkXPATCH -H'X-Id: 1' -d'a=1' -m2.5 https://x.io");
        assert_eq!(r.method, "PATCH");
        assert!(r.follow_redirects && r.insecure);
        assert_eq!(header(&r, "x-id"), Some("1"));
        assert_eq!(body(&r), "a=1");
        assert_eq!(r.timeout_secs, Some(3));
    }

    #[test]
    fn data_kinds_and_implied_methods() {
        let r = parse("curl -d a=1 -d 'b=2 3' https://x.io");
        assert_eq!(
            (r.method.as_str(), body(&r)),
            ("POST", "a=1&b=2 3".to_string())
        );
        assert_eq!(
            header(&r, "content-type"),
            Some("application/x-www-form-urlencoded")
        );

        let r =
            parse("curl --data-urlencode 'q=hello world&more' --data-urlencode =x/y https://x.io");
        assert_eq!(body(&r), "q=hello+world%26more&x%2Fy");

        let r = parse(r#"curl --json '{"a":1}' https://x.io"#);
        assert_eq!(
            (r.method.as_str(), body(&r)),
            ("POST", r#"{"a":1}"#.to_string())
        );
        assert_eq!(header(&r, "content-type"), Some("application/json"));
        assert_eq!(header(&r, "accept"), Some("application/json"));

        let r = parse("curl -G -d q=pepe -d page=2 'https://x.io/search?lang=en'");
        assert_eq!(r.method, "GET");
        assert_eq!(r.url, "https://x.io/search?lang=en&q=pepe&page=2");
        assert!(r.body.is_none());

        let r = parse("curl --url-query 'q=a b' --url-query +raw=%41 https://x.io/s");
        assert_eq!(r.url, "https://x.io/s?q=a+b&raw=%41");

        let r = parse("curl -X DELETE https://x.io/1");
        assert_eq!(r.method, "DELETE");
        let r = parse("curl -X OPTIONS -H 'Origin: https://a.io' https://x.io");
        assert_eq!(r.method, "OPTIONS");
    }

    #[test]
    fn files_as_data_form_and_upload() {
        let dir = std::env::temp_dir().join(format!("pepe-curl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let json = dir.join("body.json");
        std::fs::write(&json, "{\"a\":\n1}\n").unwrap();
        let png = dir.join("logo.png");
        std::fs::write(&png, [0x89, b'P', b'N', b'G', 0, 1]).unwrap();
        let (json, png) = (json.display().to_string(), png.display().to_string());

        // Quoted, as in a shell, so a Windows path keeps its backslashes
        let r = parse(&format!("curl -d '@{json}' https://x.io"));
        assert_eq!(body(&r), "{\"a\":1}", "-d drops newlines from files");
        let r = parse(&format!("curl --data-binary '@{json}' https://x.io"));
        assert_eq!(body(&r), "{\"a\":\n1}\n", "--data-binary keeps them");

        let r = parse(&format!(
            "curl -F name=Ada -F 'avatar=@{png};type=image/x-png' -F 'note=<{json}' https://x.io/up"
        ));
        assert_eq!(r.method, "POST");
        let ct = header(&r, "content-type").unwrap();
        let boundary = ct.strip_prefix("multipart/form-data; boundary=").unwrap();
        let sent = r.body.clone().unwrap();
        let text = String::from_utf8_lossy(&sent);
        assert!(text.starts_with(&format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"name\"\r\n\r\nAda\r\n"
        )));
        assert!(text.contains(
            "name=\"avatar\"; filename=\"logo.png\"\r\nContent-Type: image/x-png\r\n\r\n"
        ));
        assert!(
            sent.windows(6).any(|w| w == [0x89, b'P', b'N', b'G', 0, 1]),
            "binary kept intact"
        );
        assert!(text.contains("name=\"note\"\r\n\r\n{\"a\":\n1}\n\r\n"));
        assert!(text.ends_with(&format!("--{boundary}--\r\n")));

        let r = parse(&format!("curl -T '{png}' https://x.io/files/"));
        assert_eq!(
            (r.method.as_str(), r.url.as_str()),
            ("PUT", "https://x.io/files/logo.png")
        );

        let err = parse_command("curl -d @/no/such/file https://x.io").unwrap_err();
        assert!(err.contains("/no/such/file"), "{err}");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn auth_cookies_and_other_headers() {
        let r =
            parse("curl -u ada:s3cret -e 'https://ref.io;auto' -r 0-99 -A pepe-test https://x.io");
        assert_eq!(header(&r, "authorization"), Some("Basic YWRhOnMzY3JldA=="));
        assert_eq!(header(&r, "referer"), Some("https://ref.io"));
        assert_eq!(header(&r, "range"), Some("bytes=0-99"));
        assert_eq!(r.user_agent.as_deref(), Some("pepe-test"));

        let r = parse("curl --oauth2-bearer abc -b a=1 -b 'b=2' https://x.io");
        assert_eq!(header(&r, "authorization"), Some("Bearer abc"));
        assert_eq!(header(&r, "cookie"), Some("a=1; b=2"));

        // A header the user sets wins over curl's; "Name:" removes it; "Name;" is empty
        let r = parse(
            "curl -d x=1 -H 'Content-Type: text/plain' -H 'X-Empty;' -H 'Accept:' https://x.io",
        );
        assert_eq!(header(&r, "content-type"), Some("text/plain"));
        assert_eq!(
            r.headers
                .iter()
                .filter(|h| h.to_lowercase().starts_with("content-type"))
                .count(),
            1
        );
        assert_eq!(header(&r, "x-empty"), Some(""));
        assert_eq!(header(&r, "accept"), None);
    }

    #[test]
    fn output_and_tls_options_are_accepted() {
        let r = parse(
            "curl -o /dev/null -w '%{http_code}' --retry 3 --connect-timeout 5 --http2 --cacert ca.pem -v --no-progress-meter https://x.io",
        );
        assert_eq!(r.url, "https://x.io");
        assert!(r.notes.is_empty(), "{:?}", r.notes);
    }

    #[test]
    fn proxy_timeout_and_keepalive() {
        let r = parse("curl -x proxy.local:3128 -m 600 --no-keepalive https://x.io");
        assert_eq!(r.proxy.as_deref(), Some("http://proxy.local:3128"));
        assert_eq!(r.timeout_secs, Some(120));
        assert!(r.no_keepalive);
        assert!(r.notes.iter().any(|n| n.contains("120s")));
        let r = parse("curl -L --no-location https://x.io");
        assert!(!r.follow_redirects);
    }

    #[test]
    fn prompts_program_paths_and_double_dash() {
        let r = parse("$ /usr/bin/curl -s -- https://x.io/-weird");
        assert_eq!(r.url, "https://x.io/-weird");
        assert_eq!(parse("curl.exe https://x.io").url, "https://x.io");
        assert_eq!(
            parse("https://x.io -I").method,
            "HEAD",
            "curl itself is optional"
        );
    }

    #[test]
    fn clear_errors() {
        for (cmd, want) in [
            ("curl", "no URL"),
            (
                "curl --frobnicate https://x.io",
                "unknown curl option --frobnicate",
            ),
            ("curl -% https://x.io", "unknown curl option -%"),
            ("curl https://x.io -H", "needs a value"),
            ("curl -F a=1 -d b=2 https://x.io", "-F"),
            ("curl -m soon https://x.io", "not a number"),
        ] {
            let err = parse_command(cmd).unwrap_err();
            assert!(err.contains(want), "{cmd}: {err}");
        }
    }

    #[test]
    fn notes_for_what_pepe_cannot_do() {
        let r = parse("curl -u ada -b cookies.txt --expand-url 'https://x.io/{{id}}' https://y.io");
        let notes = r.notes.join("\n");
        assert!(notes.contains("password"), "{notes}");
        assert!(notes.contains("cookies.txt"), "{notes}");
        assert!(notes.contains("expanded"), "{notes}");
        assert!(notes.contains("https://y.io"), "{notes}");
    }

    #[test]
    fn base64_matches_known_values() {
        for (plain, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
        ] {
            assert_eq!(base64(plain.as_bytes()), encoded);
        }
    }
}
