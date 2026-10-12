//! The project's saved tests: `.pepe/tests/NAME.toml` next to the code,
//! each a `pepe.toml` under a name, so a test worked out once is kept,
//! committed with the code, and run again with `pepe --load NAME`.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use clap::{CommandFactory, Parser};

use crate::cli::Cli;
use crate::config::{self, Config};

/// The store's directory, in the directory pepe runs in
pub const DIR: &str = ".pepe";
/// Where the tests go under it
const TESTS: &str = "tests";
/// Longest name taken
const NAME_MAX: usize = 64;

/// The saved tests under one root
#[derive(Debug, Clone, PartialEq)]
pub struct Store {
    root: PathBuf,
}

/// One saved test, as the library lists it
#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    /// When the file was last written
    pub modified: Option<SystemTime>,
    /// The settings it runs with, or why the file can't be read
    pub cli: Result<Box<Cli>, String>,
}

/// The settings a saved test runs with on top of `base`: the file fills
/// in what the command line didn't say, and decides the mode when the
/// command line named none
fn on_top_of(config: &Config, base: &Cli) -> Cli {
    let mut cli = base.clone();
    let nothing_typed = Cli::command().get_matches_from(["pepe"]);
    config.apply_test(&mut cli, &nothing_typed);
    cli
}

/// The settings a saved test runs with, on pepe's defaults
pub fn settings_of(config: &Config) -> Cli {
    on_top_of(config, &Cli::parse_from(["pepe"]))
}

impl Store {
    /// The project's store: `.pepe` in the current directory
    pub fn local() -> Self {
        Store::at(DIR)
    }

    /// A store under `root`
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Store { root: root.into() }
    }

    /// Where the tests are
    pub fn tests_dir(&self) -> PathBuf {
        self.root.join(TESTS)
    }

    /// The file a name is kept in, as it's shown: `.pepe/tests/NAME.toml`
    pub fn path(&self, name: &str) -> PathBuf {
        self.tests_dir().join(format!("{name}.toml"))
    }

    /// Whether a test of this name is saved
    pub fn has(&self, name: &str) -> bool {
        validate_name(name).is_ok() && self.path(name).is_file()
    }

    /// The saved names, in order
    pub fn names(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(self.tests_dir()) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().is_file())
            .filter_map(|entry| {
                let file = entry.file_name();
                let file = file.to_str()?;
                let name = file.strip_suffix(".toml")?;
                validate_name(name).ok().map(|()| name.to_string())
            })
            .collect();
        names.sort();
        names
    }

    /// The test saved under `name`, ready to run or to edit, on top of
    /// what the command line gave
    pub fn open(&self, name: &str, base: &Cli) -> Result<Cli, String> {
        let (path, config) = self.load(name)?;
        let mut cli = on_top_of(&config, base);
        cli.config_loaded = Some(path);
        Ok(cli)
    }

    /// Every saved test, by name; a file that doesn't read is listed with
    /// its error rather than hidden
    pub fn entries(&self) -> Vec<Entry> {
        self.names()
            .into_iter()
            .map(|name| {
                let path = self.path(&name);
                let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                let cli = self
                    .load(&name)
                    .map(|(_, config)| Box::new(settings_of(&config)));
                Entry {
                    name,
                    modified,
                    cli,
                }
            })
            .collect()
    }

    /// The name a path in the store is saved under, if it's one
    pub fn name_of(&self, path: &Path) -> Option<String> {
        let name = path.file_name()?.to_str()?.strip_suffix(".toml")?;
        // The same file, however the path is spelled
        let same = match (path.canonicalize(), self.path(name).canonicalize()) {
            (Ok(a), Ok(b)) => a == b,
            _ => path.parent() == Some(self.tests_dir().as_path()),
        };
        (same && validate_name(name).is_ok()).then(|| name.to_string())
    }

    /// Save the settings as they stand under `name`, replacing what was
    /// there; the directory is made on the first save
    pub fn save(&self, name: &str, cli: &Cli) -> Result<PathBuf, String> {
        validate_name(name)?;
        let dir = self.tests_dir();
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("couldn't create {}: {e}", dir.display()))?;
        let path = self.path(name);
        let text = Config::from_cli(cli).to_toml_with(&format!(
            "# pepe load test `{name}` · run it with `pepe --load {name}` in this directory"
        ));
        std::fs::write(&path, text)
            .map_err(|e| format!("couldn't write {}: {e}", path.display()))?;
        Ok(path)
    }

    /// Read the test saved under `name`
    pub fn load(&self, name: &str) -> Result<(PathBuf, Config), String> {
        validate_name(name)?;
        let path = self.path(name);
        if !path.is_file() {
            let names = self.names();
            let saved = if names.is_empty() {
                format!("nothing is saved in {} yet", self.tests_dir().display())
            } else {
                format!("saved: {}", names.join(", "))
            };
            return Err(format!("no test named `{name}` ({saved})"));
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("couldn't read {}: {e}", path.display()))?;
        let config = config::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok((path, config))
    }
}

/// A name is a file name without surprises: letters, digits, `.`, `-`
/// and `_`, not starting with a dot, up to 64 characters
pub fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("give the test a name".into());
    }
    if name.starts_with('.') {
        return Err("a name can't start with a dot".into());
    }
    if name.chars().count() > NAME_MAX {
        return Err(format!("a name is at most {NAME_MAX} characters"));
    }
    if let Some(c) = name
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')))
    {
        return Err(format!(
            "a name takes letters, digits, `.`, `-` and `_`, not `{}`",
            if c.is_whitespace() {
                "space".to_string()
            } else {
                c.to_string()
            }
        ));
    }
    Ok(())
}

/// A name to offer for a URL: its host and path, `api.example.com-health`;
/// `test` when the URL says nothing usable
pub fn suggest_name(url: &str) -> String {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    // The host without its port or any credentials
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host.rsplit('@').next().unwrap_or(host);
    let host = host.split(':').next().unwrap_or(host);
    let mut name = String::new();
    let path = path.chars().map(|c| if c == '/' { '-' } else { c });
    for c in host.chars().chain(std::iter::once('-')).chain(path) {
        let c = if c.is_ascii_alphanumeric() || matches!(c, '.' | '_') {
            c.to_ascii_lowercase()
        } else {
            '-'
        };
        if c == '-' && name.ends_with('-') {
            continue;
        }
        name.push(c);
    }
    let name: String = name
        .trim_matches(['-', '.'])
        .chars()
        .take(NAME_MAX)
        .collect();
    let name = name.trim_matches(['-', '.']).to_string();
    if validate_name(&name).is_ok() {
        name
    } else {
        "test".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn temp_store(tag: &str) -> Store {
        let root = std::env::temp_dir().join(format!("pepe-store-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        Store::at(root)
    }

    #[test]
    fn names_are_file_names_without_surprises() {
        assert!(validate_name("api-health").is_ok());
        assert!(validate_name("checkout_v2.staging").is_ok());
        assert!(validate_name("").unwrap_err().contains("name"));
        assert!(validate_name(".hidden").unwrap_err().contains("dot"));
        assert!(validate_name("a b").unwrap_err().contains("space"));
        assert!(validate_name("a/b").unwrap_err().contains("`/`"));
        assert!(validate_name(&"x".repeat(65)).unwrap_err().contains("64"));
    }

    #[test]
    fn a_name_is_offered_from_the_url() {
        assert_eq!(
            suggest_name("https://api.example.com/v1/health?x=1"),
            "api.example.com-v1-health"
        );
        assert_eq!(suggest_name("http://user:pw@localhost:8080/"), "localhost");
        assert_eq!(
            suggest_name("https://Example.com//a//b/"),
            "example.com-a-b"
        );
        assert_eq!(suggest_name(""), "test");
        assert_eq!(suggest_name("https://"), "test");
        assert!(suggest_name(&format!("https://x.io/{}", "a".repeat(100))).len() <= NAME_MAX);
    }

    #[test]
    fn a_saved_test_reads_back_and_is_listed() {
        let store = temp_store("round-trip");
        assert!(store.names().is_empty());
        let cli = Cli::parse_from([
            "pepe",
            "-c",
            "20",
            "-z",
            "30s",
            "-H",
            "Accept: application/json",
            "https://api.example.com/health",
        ]);
        let path = store.save("api-health", &cli).unwrap();
        assert_eq!(path, store.tests_dir().join("api-health.toml"));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("pepe --load api-health"), "{text}");
        assert!(store.has("api-health") && !store.has("other"));

        let (read, config) = store.load("api-health").unwrap();
        assert_eq!(read, path);
        assert_eq!(
            config.url.as_deref(),
            Some("https://api.example.com/health")
        );
        assert_eq!(config.concurrency, Some(20));
        assert_eq!(config.duration.as_deref(), Some("30s"));
        assert_eq!(
            config.headers,
            Some(vec!["Accept: application/json".to_string()])
        );

        store.save("zeta", &cli).unwrap();
        std::fs::write(store.tests_dir().join("notes.txt"), "not a test").unwrap();
        assert_eq!(store.names(), ["api-health", "zeta"]);
        assert_eq!(store.name_of(&path), Some("api-health".to_string()));
        assert_eq!(store.name_of(Path::new("pepe.toml")), None);
        let _ = std::fs::remove_dir_all(store.root);
    }

    #[test]
    fn entries_carry_the_settings_and_the_file_decides_the_mode() {
        let store = temp_store("entries");
        store
            .save(
                "plain",
                &Cli::parse_from(["pepe", "-c", "4", "https://x.io/a"]),
            )
            .unwrap();
        store
            .save(
                "climb",
                &Cli::parse_from(["pepe", "ramp", "--to", "40", "https://x.io/b"]),
            )
            .unwrap();
        std::fs::write(store.path("broken"), "nope = ").unwrap();
        let entries = store.entries();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["broken", "climb", "plain"]);
        assert!(entries[0].cli.as_ref().unwrap_err().contains("broken.toml"));
        let climb = entries[1].cli.as_ref().unwrap();
        match &climb.command {
            Some(crate::cli::Command::Ramp(ramp)) => {
                assert_eq!((ramp.to, ramp.url.as_str()), (40, "https://x.io/b"));
            }
            other => panic!("not a ramp: {other:?}"),
        }
        let plain = entries[2].cli.as_ref().unwrap();
        assert!(plain.command.is_none());
        assert_eq!(
            (plain.concurrency, plain.url.as_str()),
            (4, "https://x.io/a")
        );
        assert!(entries[2].modified.is_some());
        let _ = std::fs::remove_dir_all(store.root);
    }

    #[test]
    fn a_missing_name_says_what_is_saved() {
        let store = temp_store("missing");
        let e = store.load("nope").unwrap_err();
        assert!(e.contains("nothing is saved"), "{e}");
        let cli = Cli::parse_from(["pepe", "https://x.io"]);
        store.save("one", &cli).unwrap();
        let e = store.load("nope").unwrap_err();
        assert!(e.contains("saved: one"), "{e}");
        assert!(store.load("etc/passwd").unwrap_err().contains("`/`"));
        let _ = std::fs::remove_dir_all(store.root);
    }
}
