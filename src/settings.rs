//! The settings kept on this machine: the theme, and the defaults a run
//! starts from when neither the command line nor a `pepe.toml` says
//! otherwise. One file, `settings.toml`, in the user's configuration
//! directory; the settings page (F2 on any screen) writes it, and it reads
//! as well by hand.

use std::path::PathBuf;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

use crate::config::Config;

/// Names the directory `settings.toml` is kept in, instead of the usual one
pub const DIR_ENV: &str = "PEPE_CONFIG_DIR";
const FILE: &str = "settings.toml";

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Settings {
    /// A theme's name; unset, the terminal decides. Kept as written, so a
    /// theme from a newer pepe isn't an error in an older one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    /// false: never look for a newer release
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_check: Option<bool>,
    /// What a `pepe.toml` can say, for every run on this machine
    #[serde(default, skip_serializing_if = "is_unset")]
    pub defaults: Config,
}

fn is_unset(config: &Config) -> bool {
    *config == Config::default()
}

static SETTINGS: RwLock<Option<Settings>> = RwLock::new(None);

/// Where the settings are kept: `$PEPE_CONFIG_DIR`, else `%APPDATA%\pepe`
/// on Windows, else `$XDG_CONFIG_HOME/pepe`, `~/.config/pepe` by default.
/// None when there is no home directory to keep them in.
pub fn path() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(DIR_ENV).filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir).join(FILE));
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    let dir = if cfg!(windows) {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .or_else(|| home.map(|h| PathBuf::from(h).join("AppData/Roaming")))?
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .filter(|d| !d.is_empty())
            .map(PathBuf::from)
            .or_else(|| home.map(|h| PathBuf::from(h).join(".config")))?
    };
    Some(dir.join("pepe").join(FILE))
}

pub fn parse(text: &str) -> Result<Settings, String> {
    toml::from_str(text).map_err(|e| e.message().to_string())
}

/// Read the settings for this run. No file is no settings; a file that
/// doesn't read is an error, since a cap it holds would otherwise be
/// dropped without a word.
pub fn init() -> Result<(), String> {
    let Some(path) = path() else { return Ok(()) };
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("couldn't read {}: {e}", path.display())),
    };
    let settings = parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    *SETTINGS.write().unwrap_or_else(|e| e.into_inner()) = Some(settings);
    Ok(())
}

/// The settings as they stand
pub fn get() -> Settings {
    SETTINGS
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_default()
}

/// Keep `settings` for the rest of this run and write them to the file
pub fn save(settings: &Settings) -> Result<PathBuf, String> {
    *SETTINGS.write().unwrap_or_else(|e| e.into_inner()) = Some(settings.clone());
    let path = path().ok_or("no home directory to keep the settings in")?;
    write(&path, settings)?;
    Ok(path)
}

impl Settings {
    /// The file's text, with a line on what it is
    pub fn to_toml(&self) -> String {
        let body = toml::to_string(self).unwrap_or_default();
        format!(
            "# pepe settings for this machine · F2 on any screen changes them\n\
             # flags and a pepe.toml win over [defaults]\n\n{body}"
        )
    }
}

/// Write `settings` to `path`, whole or not at all: a run started while
/// the page is saving never reads half a file
pub fn write(path: &std::path::Path, settings: &Settings) -> Result<(), String> {
    let failed = |e: std::io::Error| format!("couldn't write {}: {e}", path.display());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(failed)?;
    }
    let partial = path.with_extension("toml.partial");
    std::fs::write(&partial, settings.to_toml()).map_err(failed)?;
    std::fs::rename(&partial, path).map_err(|e| {
        let _ = std::fs::remove_file(&partial);
        failed(e)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn written_settings_read_back_the_same() {
        let settings = Settings {
            theme: Some("nord".into()),
            update_check: Some(false),
            defaults: Config {
                concurrency: Some(50),
                duration: Some("30s".into()),
                max_rate: Some(500.0),
                allow_host: Some(vec![".staging.example.com".into()]),
                ramp: Some(crate::config::RampConfig {
                    to: Some(200),
                    ..Default::default()
                }),
                ..Default::default()
            },
        };
        let text = settings.to_toml();
        assert!(text.starts_with("# pepe settings"), "{text}");
        assert!(
            text.contains("theme = \"nord\"") && text.contains("[defaults]"),
            "{text}"
        );
        assert!(text.contains("[defaults.ramp]"), "{text}");
        assert_eq!(parse(&text).unwrap(), settings);

        // Nothing chosen is a file with nothing in it but its heading
        let empty = Settings::default().to_toml();
        assert!(
            !empty.contains("\n[defaults]") && !empty.contains("theme ="),
            "{empty}"
        );
        assert_eq!(parse(&empty).unwrap(), Settings::default());
    }

    #[test]
    fn a_typo_is_named_and_a_new_theme_is_not_one() {
        let err = parse("[defaults]\nconcurency = 5\n").unwrap_err();
        assert!(err.contains("unknown field `concurency`"), "{err}");
        assert!(parse("them = \"nord\"\n").is_err());
        assert_eq!(
            parse("theme = \"vaporwave\"\n").unwrap().theme.as_deref(),
            Some("vaporwave")
        );
    }

    #[test]
    fn the_file_is_written_whole_into_a_directory_made_for_it() {
        let dir = std::env::temp_dir().join(format!("pepe-settings-{}", std::process::id()));
        let path = dir.join("nested").join(FILE);
        let settings = Settings {
            theme: Some("paper".into()),
            ..Default::default()
        };
        write(&path, &settings).unwrap();
        assert_eq!(
            parse(&std::fs::read_to_string(&path).unwrap()).unwrap(),
            settings
        );
        assert!(!path.with_extension("toml.partial").exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
