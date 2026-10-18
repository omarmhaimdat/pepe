# Webhooks

Say what a run found where your team will see it: a Slack, Discord, Teams or Google Chat channel, or any URL that takes JSON. One flag is enough; a `[[webhook]]` table says more.

```bash
pepe -z 30s -c 50 --fail-if 'p99 > 300ms' https://api.example.com/health \
  --webhook "$SLACK_WEBHOOK_URL"
```

When the run ends, the channel gets the verdict: passed or failed (and which `--fail-if` was crossed), the requests, requests a second, p50, p95 and p99, the error rate, how long it ran, what pepe found, and the command that runs it again. `pepe ramp` sends its capacity and the concurrency it held; `pepe ping` each target's latency and loss.

## Slack, live

```bash
pepe slack connect                       # once per machine, all from the terminal
pepe -z 10m -c 50 --slack https://api.example.com/health
```

With `--slack` a run is one message in the channel, kept up to date, with Pepe the pepper as its face:

- **While it runs** (edited every 10 seconds, and at once when something changes or someone steers it): what is sent, how hard, from which machine and by whom; a progress bar with the time left; throughput with its peak and which way it leans lately; p50 · p95 · p99 with a sparkline; errors by cause with the first body the server sent; where the time goes, waiting for the first byte against reading the body; requests and bytes in; what's in flight and at what rate, and who changed it. Under them, **What changed**: each jump in latency, fall in throughput or rise in errors, a line each with its time (`` `1m26s` 📉 Throughput dropped 52% · 5.9k req/s ``), in the message itself: the thread stays quiet.
- **When it ends** the message becomes the verdict: a sentence that says how much was sent, how fast, what failed and the first thing that changed; latency from p50 to the max; sparklines over the whole run; where the time went (waiting, connecting, reading); the answers by status; each `--fail-if` with ✓ or ✗ and what was measured; for a saved test, how it compares with its last recorded run; and what pepe found, worst first, with the first body of each kind of failure. A colour down its side says passed, warned or failed.
- **Under it**, one line in the thread, shown in the channel too: the result at a glance where the channel is now, with Pepe proud, worried or on fire, and the `mention` when the run failed (an edited message notifies no one). The verdict has the run's **What changed** too.
- **Ask it in the thread.** With `pepe slack serve` running, `@pepe` in the thread of a run it started answers there: *p99*, *errors* (what failed, what the server said, when it began), *slowest*, *where does the time go*, *codes*, *compare* (against the test's last run), *status*, *help*. While the run goes, the run answers from its live numbers; after, the recorded report does. Nothing else is posted in the thread.

`--slack=#another` posts somewhere else; in a file it's `webhook = "slack:"` or a `[[webhook]]` table with `url = "slack:#load-tests"`, which takes `on`, `every` and `mention` like any other.

`pepe slack connect` needs no form filled in and nothing hosted: the app is yours, in your workspace. At a terminal it's a checklist that ticks itself off:

1. **Create the app.** Slack's "create an app" page opens with everything already said (name, permissions); with no browser to open, the link is copied to the clipboard. Pick the workspace, then **Create**, **Install to Workspace**, **Allow**.
2. **Bot token.** Paste the Bot User OAuth Token (`xoxb-…`); it shows as dots, and is checked with Slack straight away.
3. **Channel.** Pick it with the arrow keys from the channels pepe can see, the busiest first, filtered as you type, with their members and whether pepe is in them already (a private one needs `/invite @pepe` first).
4. **Test message.** pepe joins the channel, posts there, and gives you the link to it.

The token and the channel are kept in `slack.toml` beside the settings, readable by you alone, with Pepe's face as `pepe-icon.png` next to it: Slack lets only you set an app's icon, so drag it into the app's Basic Information. Run anywhere but a terminal, or with `--token` and `--channel`, `connect` asks nothing and says each step on a line.

The same steps are on the [webhook page](#from-the-screens): `S` there connects Slack without leaving the run, and adds the hook. `pepe slack status` checks the connection, `pepe slack test` posts a test message, `pepe slack forget` removes the file, and `pepe slack manifest` prints the app's manifest, for Slack's "From a manifest" if the link doesn't fill the page in. In CI, `PEPE_SLACK_TOKEN` stands in for the file: `PEPE_SLACK_TOKEN=xoxb-… pepe --slack=#load-tests …`.

An app made by an earlier pepe lacks something: Pepe's faces need `chat:write.customize` (0.47), the buttons Socket Mode (0.48), and questions in the thread the `app_mention` event (0.51). `pepe slack connect` again makes an app that has them all.

## Buttons and /pepe

```bash
pepe slack serve          # in the project, left running: the buttons and /pepe answer here
```

With the buttons set up (step 5 of `pepe slack connect`: an app-level token), a run started from Slack can be steered from its message while it goes, and a saved test's verdict has **↻ Run again**, **✎ Edit and run** and **History**.

- **While it runs**: **⏸ Pause** (the message turns grey and says who paused it; **▶ Resume** picks it up), **− 1** and **+ 1** to change the concurrency by one at once (each button says what it changes it to; a ramp sets its own, so it has only pause), and **■ Stop**, asked twice since it ends the run as ctrl-c would: the message becomes the verdict of what it measured. Each change is in the message straight away, with who made it.
- **Run again** runs the test as it is saved, its message in the channel the button was pressed in, saying who started it. Runs from Slack are recorded, so the test's history and trend keep up.
- **Edit and run** opens a form filled with the test: concurrency, the duration or a number of requests, the rate, `--fail-if` conditions, and whether to keep the settings in the test. A value that won't do is said beside its field.
- **History** lists the test's recorded runs, a line each: how it went, when, requests a second, p99 and errors.

`/pepe` answers only the one who asked:

| Command | Answer |
| --- | --- |
| `/pepe` or `/pepe list` | The saved tests in a dropdown, and what's running with its Pause and Stop. Picking a test shows its card in place: what it sends, its last run, its trend over the last runs, and Run, Edit and run, History |
| `/pepe NAME` | That test's card |
| `/pepe run NAME` | Runs it; `/pepe run` alone gives the dropdown |
| `/pepe status` | What's running, with its controls |
| `/pepe stop` · `/pepe stop NAME` | Stops what this listener runs, or that test |
| `/pepe history NAME` | Its recorded runs |

Slack sends every click of an app to one of its open connections, not to the run that posted the message, so one listener answers them all: `pepe slack serve`, left running on a machine (a server, a CI runner, your laptop). It runs the saved tests of the directory it was started in, each as `pepe --load NAME --slack=#channel`, so that directory's `pepe.toml` guardrails hold for them, and it says each click, run and result in its terminal. Run one per app.

Only the people step 6 named may use them (`pepe slack allow @maya @omar` changes who, `*` lets anyone in the channel); anyone else is told so, and no one else sees it. Nothing can be run from Slack that isn't a saved test in that directory. In CI, `PEPE_SLACK_APP_TOKEN` stands in for the file's app token.

## Where it goes

The URL says how the message is laid out:

| URL | Message |
| --- | --- |
| `https://hooks.slack.com/…` | Slack: a header, the numbers as fields, the findings, the command |
| `https://discord.com/api/webhooks/…` | Discord: an embed coloured by the outcome |
| `https://….webhook.office.com/…`, a Teams workflow (`….logic.azure.com`) | Teams: an Adaptive Card |
| `https://chat.googleapis.com/…` | Google Chat: a few lines of text |
| anything else | JSON: the event, the run, and the whole [report](output.md) |

`format` in a table picks one instead: `auto`, `json`, `slack`, `discord`, `teams`, `google-chat` or `text`.

## When it is sent

| Event | Sent |
| --- | --- |
| `start` | When the run starts: what it sends, to where |
| `progress` | Every `every` (a minute unless said): a bar, the requests so far, requests a second, p50, p99, errors |
| `anomaly` | When something changes during the run: "p99 jumped 4.5× to 45ms at 26s" |
| `end` | When the run ends: the verdict. What a webhook is sent on when nothing is said |
| `fail` | When the run ends and a `--fail-if` was crossed (a ping: its `--slo` broken, or nothing answered) |

`--webhook-on start,fail` says it for the `--webhook` URLs; `on = [...]` for a table. A hook on `fail` alone stays quiet until something is wrong: the one to page someone with.

## In pepe.toml

```toml
# one URL: the verdict at the end
webhook = "${SLACK_WEBHOOK_URL}"

# or as many as you need, each said in full
[[webhook]]
url = "${SLACK_WEBHOOK_URL}"
on = ["start", "progress", "end"]
every = "30s"                  # progress every 30 seconds (5s at least)

[[webhook]]
url = "${PAGER_WEBHOOK_URL}"
on = ["fail"]
mention = "<!here>"            # before a failed run's message

[[webhook]]
url = "https://ci.example.com/hooks/pepe"
headers = ["Authorization: Bearer ${CI_TOKEN}"]
body = '{"status": "{{status}}", "p99": "{{p99}}", "rps": "{{rps}}", "test": "{{test}}"}'
```

`${NAME}` in a URL, a header, a body or a mention is read from the environment when the run starts, and a name that isn't set is an error before anything is sent, so a secret stays out of the file. The same keys go in the [settings](install.md#settings) under `[defaults]`, for every project on the machine; a project's `pepe.toml`, a saved test, and `--webhook` on the command line each win over what's under them. `--keep` and ctrl-s keep a test's webhooks with it.

## Your own body

`body` replaces the message with your own, its `{{holes}}` filled in: JSON is sent as `application/json`, anything else as text.

| Hole | Is |
| --- | --- |
| `{{event}}` | start, progress, anomaly, end, fail (test, from the webhook page) |
| `{{status}}` | running, passed or failed |
| `{{level}}` | healthy, degraded or failing, at the end |
| `{{title}}`, `{{text}}` | The message's title, and the message as one line |
| `{{mode}}`, `{{target}}`, `{{test}}`, `{{command}}` | What ran |
| `{{requests}}`, `{{failed}}`, `{{rps}}`, `{{errors}}`, `{{duration}}`, `{{progress}}` | The numbers |
| `{{p50}}`, `{{p95}}`, `{{p99}}` | Latency |
| `{{verdict}}`, `{{findings}}` | The first finding, and all of them a line each |
| `{{report}}` | The whole JSON report, at the end |

A hole pepe doesn't fill is an error when the run starts.

## From the screens

F3 opens the webhook page over any screen, each webhook a card with where it goes and how its last message went (sent, and how long ago, or what went wrong). On the setup form it sets the hooks of the run you're about to start; while a run goes, a hook added or changed there is sent to from then on, so a long soak can be pointed at a channel after it started. Paste or type a URL on the last line, or press `S` for Slack (connected right there if it isn't yet); on each hook, 1 to 5 tick the events, ← → change the format, enter edits the URL, the interval and the mention, `t` sends a test message and says what came back, del removes it. What the page leaves stays for the rest of the session: a restart, ctrl-s and the command printed at the end keep it.

## Delivery

A message never holds the run up. Each webhook has its own queue, sent in order; a dropped connection, a 429 or a 5xx is tried again (three times in all, after the `Retry-After` when there is one). At the end pepe waits up to 15 seconds for the last messages, and says on stderr what couldn't be sent; the exit code is the run's either way. A `--dry-run` lists the webhooks and sends none. Reports and the JSON keep a webhook's URL masked, `https://hooks.slack.com/••••`, since its path is its secret.
