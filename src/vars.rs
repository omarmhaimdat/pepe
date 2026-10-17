//! `--vars FILE`: values for the `{{holes}}` in a request, a row at a
//! time, from a CSV file whose first line names them. A run or a ramp
//! sends one request per row, in the file's order and round again; a flow
//! starts each chain with the next row's values.
//!
//! The rows are read once and made into requests before the run starts,
//! so a request costs the same with values as without (see `load`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::flow::Template;

/// Most rows a file may have: each is a request kept ready for the run
pub const MAX_ROWS: usize = 1_000_000;

/// A `--vars` file, read
#[derive(Debug, Clone)]
pub struct Vars {
    pub path: PathBuf,
    /// The first line: the holes' names
    pub names: Vec<String>,
    /// One value per name in each
    pub rows: Vec<Vec<String>>,
}

impl Vars {
    pub fn load(path: &Path) -> Result<Vars, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("--vars: couldn't read {}: {e}", path.display()))?;
        Vars::parse(&text, path).map_err(|e| format!("--vars {}: {e}", path.display()))
    }

    fn parse(text: &str, path: &Path) -> Result<Vars, String> {
        let mut records = csv(text.strip_prefix('\u{feff}').unwrap_or(text))?.into_iter();
        let Some((_, names)) = records.next() else {
            return Err(
                "the file is empty; its first line names the values, as in email,password".into(),
            );
        };
        let names: Vec<String> = names.into_iter().map(|n| n.trim().to_string()).collect();
        for (i, name) in names.iter().enumerate() {
            if name.is_empty() {
                return Err(format!(
                    "column {} of the first line has no name; the first line names the values, as in email,password",
                    i + 1
                ));
            }
            if names[..i].contains(name) {
                return Err(format!("{name:?} names two columns"));
            }
        }
        let mut rows = Vec::new();
        for (line, row) in records {
            if row.len() != names.len() {
                return Err(format!(
                    "line {line} has {} value{} where the first line names {}",
                    row.len(),
                    if row.len() == 1 { "" } else { "s" },
                    names.len()
                ));
            }
            if rows.len() == MAX_ROWS {
                return Err(format!(
                    "more than {MAX_ROWS} rows; each is a request kept ready, so take a sample"
                ));
            }
            rows.push(row);
        }
        if rows.is_empty() {
            return Err("no rows under the first line".into());
        }
        Ok(Vars {
            path: path.to_path_buf(),
            names,
            rows,
        })
    }

    /// Row `index`'s value for each name, round again past the last
    pub fn row(&self, index: usize) -> impl Iterator<Item = (&str, &str)> {
        let row = &self.rows[index % self.rows.len()];
        self.names
            .iter()
            .map(String::as_str)
            .zip(row.iter().map(String::as_str))
    }

    /// Err names the first hole that isn't a column, and the columns
    pub fn check<'a>(&self, holes: impl IntoIterator<Item = &'a str>) -> Result<(), String> {
        for hole in holes {
            if !self.names.iter().any(|n| n == hole) {
                return Err(format!(
                    "{{{{{hole}}}}} isn't a column of {} (it has {})",
                    self.path.display(),
                    self.names.join(", ")
                ));
            }
        }
        Ok(())
    }

    /// What `--dry-run` says of the file; `unit` is what takes a row
    pub fn describe(&self, unit: &str) -> String {
        format!(
            "values from {}: {} row{} of {}, a row a {unit} in turn",
            self.path.display(),
            self.rows.len(),
            if self.rows.len() == 1 { "" } else { "s" },
            self.names.join(", ")
        )
    }
}

/// A request with holes: its URL, `Name: value` headers and body
pub struct Fill {
    url: Template,
    headers: Vec<Template>,
    /// None when the body isn't text: it is sent as it is
    body: Option<Template>,
    raw_body: Option<Vec<u8>>,
}

/// A request with a row's values in its holes
pub struct Filled {
    pub url: String,
    pub headers: Vec<String>,
    pub body: Option<Vec<u8>>,
}

impl Fill {
    pub fn of(url: &str, headers: &[String], body: Option<Vec<u8>>) -> Fill {
        let text = body
            .as_deref()
            .and_then(|b| std::str::from_utf8(b).ok())
            .map(Template::parse);
        Fill {
            url: Template::parse(url),
            headers: headers.iter().map(|h| Template::parse(h)).collect(),
            raw_body: if text.is_none() { body } else { None },
            body: text,
        }
    }

    /// The names of its holes, in order, each once
    pub fn holes(&self) -> Vec<&str> {
        let mut holes: Vec<&str> = Vec::new();
        let all = self
            .url
            .vars()
            .chain(self.headers.iter().flat_map(|h| h.vars()))
            .chain(self.body.iter().flat_map(|b| b.vars()));
        for hole in all {
            if !holes.contains(&hole) {
                holes.push(hole);
            }
        }
        holes
    }

    /// The names of the headers with a hole in them
    pub fn filled_headers(&self) -> Vec<String> {
        self.headers
            .iter()
            .filter(|h| h.vars().next().is_some())
            .filter_map(|h| {
                h.source()
                    .split_once(':')
                    .map(|(n, _)| n.trim().to_string())
            })
            .collect()
    }

    /// With row `index`'s values; holes were checked against the columns
    pub fn row(&self, vars: &Vars, index: usize) -> Filled {
        let values: HashMap<String, String> = vars
            .row(index)
            .map(|(n, v)| (n.to_string(), v.to_string()))
            .collect();
        let fill = |t: &Template| t.render(&values).unwrap_or_else(|_| t.source());
        Filled {
            url: fill(&self.url),
            headers: self.headers.iter().map(fill).collect(),
            body: match &self.body {
                Some(body) => Some(fill(body).into_bytes()),
                None => self.raw_body.clone(),
            },
        }
    }
}

/// The records of a CSV file (RFC 4180: quotes around a value that has a
/// comma, a quote or a line break in it, a quote doubled inside them),
/// each with the line it starts on; blank lines are skipped
fn csv(text: &str) -> Result<Vec<(usize, Vec<String>)>, String> {
    let mut records = Vec::new();
    let mut record: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    // A field that was quoted, so an empty one still counts
    let mut was_quoted = false;
    let mut line = 1;
    let mut start = 1;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                '\n' => {
                    line += 1;
                    field.push(c);
                }
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() && !was_quoted => {
                quoted = true;
                was_quoted = true;
            }
            ',' => {
                record.push(std::mem::take(&mut field));
                was_quoted = false;
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' => {
                if !record.is_empty() || !field.is_empty() || was_quoted {
                    record.push(std::mem::take(&mut field));
                    records.push((start, std::mem::take(&mut record)));
                }
                was_quoted = false;
                line += 1;
                start = line;
            }
            _ => field.push(c),
        }
    }
    if quoted {
        return Err(format!("line {start}: a quote is never closed"));
    }
    if !record.is_empty() || !field.is_empty() || was_quoted {
        record.push(field);
        records.push((start, record));
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(text: &str) -> Result<Vars, String> {
        Vars::parse(text, Path::new("users.csv"))
    }

    #[test]
    fn a_csv_is_read_quotes_and_all() {
        let v =
            vars("\u{feff}email, id\r\nada@x.io,1\n\n\"lin, \"\"the\"\" one\",\"2\nb\"\n\"\",3")
                .unwrap();
        assert_eq!(v.names, ["email", "id"]);
        assert_eq!(
            v.rows,
            [
                vec!["ada@x.io".to_string(), "1".into()],
                vec!["lin, \"the\" one".into(), "2\nb".into()],
                vec!["".into(), "3".into()],
            ]
        );
        assert_eq!(
            v.row(4).collect::<Vec<_>>(),
            [("email", "lin, \"the\" one"), ("id", "2\nb")]
        );
    }

    #[test]
    fn a_bad_file_is_told_where() {
        assert!(vars("").unwrap_err().contains("empty"));
        assert!(vars("email\n").unwrap_err().contains("no rows"));
        assert!(vars("a,a\n1,2").unwrap_err().contains("two columns"));
        assert!(vars("a,\n1,2").unwrap_err().contains("column 2"));
        let short = vars("a,b\n1,2\n\n3\n").unwrap_err();
        assert!(short.contains("line 4 has 1 value where"), "{short}");
        assert!(vars("a\n\"1").unwrap_err().contains("line 2: a quote"));
    }

    #[test]
    fn a_request_is_filled_row_by_row() {
        let v = vars("id,token\n7,abc\n8,def").unwrap();
        let fill = Fill::of(
            "https://x.io/users/{{id}}",
            &[
                "Authorization: Bearer {{token}}".into(),
                "Accept: */*".into(),
            ],
            Some(b"{\"id\":{{ id }}}".to_vec()),
        );
        assert_eq!(fill.holes(), ["id", "token"]);
        assert!(v.check(fill.holes()).is_ok());
        let second = fill.row(&v, 1);
        assert_eq!(second.url, "https://x.io/users/8");
        assert_eq!(second.headers[0], "Authorization: Bearer def");
        assert_eq!(second.body.unwrap(), b"{\"id\":8}");
        assert_eq!(fill.row(&v, 2).url, "https://x.io/users/7", "round again");
        let missing = v.check(["email"]).unwrap_err();
        assert!(missing.contains("{{email}} isn't a column") && missing.contains("id, token"));
        // A body that isn't text goes as it is
        let binary = Fill::of("https://x.io/{{id}}", &[], Some(vec![0xff, 0x00]));
        assert_eq!(binary.row(&v, 0).body.unwrap(), [0xff, 0x00]);
    }
}
