# Change log
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](http://keepachangelog.com/)
and this project adheres to [Semantic Versioning](http://semver.org/).

<!-- next-header -->
## [Unreleased] - ReleaseDate

## [0.3.0] - 2026-04-14

### Added
- **Duration-based testing**: New `--duration` / `-z` flag to run load tests for a specified time (e.g., `10s`, `5m`, `2h`) instead of a fixed request count
- **JSON export**: New `--json` flag to export detailed test results in JSON format with latency percentiles (P50, P95, P99) and comprehensive metrics
- **Windows binary support**: Added `x86_64-pc-windows-msvc` to CI/CD build matrix for native Windows `.exe` builds
- **Automated release workflow**: New GitHub Actions workflow (`release.yaml`) that automatically builds for all platforms, generates checksums, creates releases, and updates Homebrew tap
- **Testing scripts**: Added `quick_check.sh`, `test_simple.sh`, and `test_improvements.sh` for automated non-blocking testing
- **Comprehensive documentation**: Added IMPROVEMENTS.md, QUICK_START.md, TESTING_COMPLETE.md, and CI_CD_SETUP.md guides

### Fixed
- **Shell script security**: Fixed unquoted variable expansion in `install.sh` that could cause failures with paths containing spaces
- **Checksum verification**: Fixed checksum validation logic in install script
- **PATH management**: Improved `add_to_path_if_missing()` function with proper variable quoting and handling

### Changed
- Updated Cargo.toml with `serde_json` dependency for JSON serialization
- Enhanced CI workflow to use structured matrix for better platform management
- Improved error handling in duration parsing with detailed validation messages

### Technical
- Added `src/json_report.rs` module for JSON report generation
- Extended `src/response.rs` with Serialize trait for JSON output
- Updated `src/cli.rs` with duration parsing functions and JSON flag
- Modified `src/main.rs` to integrate duration-based testing logic
- Enhanced `src/ui.rs` with methods to export collected metrics

## [0.2.9] - 2025-02-22

## [0.2.8] - 2025-02-18

## [0.2.7] - 2025-02-15

## [0.2.6] - 2025-02-15

## [0.2.5] - 2025-02-15

## [0.2.4] - 2025-02-14

### Added
- System hostname display in header section
- Author information in CLI output
- Nginx-style log format for Recent Requests
- Redesigned progress bar UI

### Changed
- Updated header information layout
- Enhanced request log visualization
- Improved progress tracking display

## [0.2.3] - 2025-02-13

### Added
- Generated reqwest client from CLI configuration

### Changed
- Improved event handling structure
- Break large functions into smaller ones
- Use default_value_t instead of default_value
- Remove redundant short and long attributes in clap
- Replace #[clap] with #[arg]
- Improve validation by returning errors instead of exit

### Removed
- Redundant comments


<!-- next-url -->
[Unreleased]: https://github.com/omarmhaimdat/pepe/compare/v0.2.9...HEAD

[0.2.9]: https://github.com/omarmhaimdat/pepe/compare/v0.2.8...v0.2.9

[0.2.8]: https://github.com/omarmhaimdat/pepe/compare/v0.2.7...v0.2.8

[0.2.7]: https://github.com/omarmhaimdat/pepe/compare/v0.2.6...v0.2.7

[0.2.6]: https://github.com/omarmhaimdat/pepe/compare/v0.2.5...v0.2.6

[0.2.5]: https://github.com/omarmhaimdat/pepe/compare/v0.2.4...v0.2.5
[0.2.4]: https://github.com/omarmhaimdat/pepe/releases/tag/v0.2.4