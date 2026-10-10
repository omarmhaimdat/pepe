# Security

pepe sends HTTP requests to whatever it is pointed at, as fast as it is
asked to. That is its purpose, and it is also why it has guardrails:
`--allow-host`, `--max-requests`, `--max-rate`, `--max-concurrency` and
`--dry-run`, which can be set once in a `pepe.toml` so a script or an
agent can't point it elsewhere or send more. Only load-test what you are
allowed to load.

## Reporting a vulnerability

If you find a vulnerability in pepe itself (the binary, the installers,
the action, the Docker image or the site), please don't open a public
issue. Use GitHub's private reporting at
https://github.com/omarmhaimdat/pepe/security/advisories/new, or email
omarmhaimdat@gmail.com with "pepe security" in the subject.

You'll get an answer within a few days, a fix as soon as one is right,
and credit in the release notes if you want it.

## What is in scope

- The `pepe` binary and what it does with the input it is given: URLs,
  headers, bodies, curl commands, OpenAPI specs, flow files, access logs,
  config files, and what targets answer with.
- The installers (`install.sh`, `install.ps1`, `pepe self-update`), the
  Homebrew formula, the Docker image and the release artifacts: every
  release ships SHA-256 checksums and signed build provenance
  (`gh attestation verify`).
- The GitHub Action (`action.yml`).
- The MCP server (`pepe mcp`), which runs pepe with the arguments a
  client gives it, under the guardrails it was started with.

## What is not

- Load that pepe sends because it was asked to. If you were load-tested
  without consent, that is between you and whoever ran it.
- The behaviour of targets, proxies or networks pepe talks to.

## Supported versions

The latest release. Older releases aren't patched; `pepe self-update`
installs the latest.
