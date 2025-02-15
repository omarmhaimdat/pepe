# Change log
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](http://keepachangelog.com/)
and this project adheres to [Semantic Versioning](http://semver.org/).

## [Unreleased] - ReleaseDate

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
