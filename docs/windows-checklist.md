# Windows checklist

CI builds and tests pepe on Windows, but it can't press keys, paste, or
look at a terminal. These are the things to try by hand on a Windows
machine before a release that touches the screens, the installers, or
anything to do with paths and files. Ten minutes, in this order.

Terminals to use: **Windows Terminal with PowerShell 7** (most people),
then **the legacy console host (`conhost`) with cmd.exe** (the worst
case: no bracketed paste, fewer colours, older fonts). Set the font to
Cascadia Mono or Consolas; both have the box-drawing and block
characters the dashboard draws with.

## Install

- [ ] `powershell -ExecutionPolicy Bypass -c "irm https://pepe.mhaimdat.com/install.ps1 | iex"` in a fresh PowerShell: ends with `pepe` on PATH in a **new** window, and "set up tab completion for PowerShell" printed.
- [ ] In that new window: `pepe --<Tab>` cycles through the flags.
- [ ] `pepe self-update --check` says the installed version is the latest (exit code 0).
- [ ] `pepe completions --install --dry-run` says every file is current.

## The dashboard

- [ ] `pepe -z 15s -c 20 https://httpbin.org/get`: the sprite, the big numbers, the heatmap and the charts draw with no stray characters. In conhost the colours are fewer but nothing is unreadable.
- [ ] `Tab`, `←` `→`, `1` `2` `3` switch views; `↑` `↓` and `PgUp` `PgDn` move in the request log; `Enter` opens a request and `Esc` closes it; `Space` pauses and the clock stops; `+` and `-` change concurrency and the footer notice appears; `?` shows the help; `q` quits and the verdict is printed in the shell.
- [ ] Resize the window while it runs: the layout follows, the small-window message appears below the minimum size and goes away again.
- [ ] `Ctrl-C` during a run: pepe exits and the prompt comes back with a working cursor and echo (type something).

## The setup screen

- [ ] `pepe` with no arguments opens the form. Type a URL; `↑` `↓` move; `←` `→` change a choice; `Backspace` and `Delete` edit; `Ctrl-T` sends once and shows the response; `Enter` starts.
- [ ] Paste a multi-line curl command copied from the browser's dev tools ("Copy as cURL (cmd)", the one with `^` continuations) into the form, in Windows Terminal **and** in conhost: every field is filled in. conhost has no bracketed paste, so the lines arrive as keystrokes; the form must still take them as one paste.
- [ ] Paste the same with "Copy as cURL (bash)" from a PowerShell window: `$'...'` quoting is understood.

## Files and paths

- [ ] `pepe -n 10 -d "@C:\path with spaces\body.json" -H "Content-Type: application/json" https://httpbin.org/post`: the body is read from the path with spaces.
- [ ] `pepe --json -n 20 https://httpbin.org/get > out.json` in PowerShell 7 **and** PowerShell 5: `out.json` is UTF-8 and parses (`Get-Content out.json | ConvertFrom-Json`). PowerShell 5 redirects as UTF-16 by default; the report must not break because of it.
- [ ] `pepe api https://petstore3.swagger.io/api/v3/openapi.json` loads the spec over the network; `pepe api .\openapi.yaml` loads it from a relative path with a backslash.

## Before pushing a change to paths, files or generated files

Three Windows-only failures reached CI during the project-health work,
none of which this machine's tests could have caught: a test that
assumed man pages exist on every platform; generated files compared
byte for byte that git had converted to CRLF on checkout; and a test
that chose a number equal to the runner's core count. The pattern:

- Anything written to or read from disk, compared byte for byte, or
  dependent on the machine, gets a `cfg!(windows)` thought and, where it
  matters, a `.gitattributes` rule.
- Run the suite on Windows before pushing when a change touches those
  areas. Without a Windows machine, open the PR as a draft and let the
  `Check (windows-latest)` job be the run; it takes about two minutes.
