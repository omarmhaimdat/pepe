<!--
The title becomes the changelog entry and decides whether a release is
made, so give it the conventional form:

  feat(ping): …      a feature, makes a minor release
  fix(logs): …       a fix, makes a patch release
  perf: …            faster or lighter, makes a patch release
  docs: … / chore: … rides along with the next release

The body below becomes the release note. Say what changed and why, the
way a user would read it. CONTRIBUTING.md has the build, the checks and
the generated files.
-->

## What

<!-- What changes, for whom. An example command and its output where it helps. -->

## Why

<!-- The problem, or the issue it closes: "Closes #123". -->

## Checked

<!--
- `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`
- Generated files, if a flag, a subcommand or a page changed:
  `UPDATE_CONTRIB=1 cargo test contrib` and `python3 site/build-docs.py`
- What you ran by hand, against what
-->
