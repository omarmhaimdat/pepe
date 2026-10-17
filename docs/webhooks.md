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

With `--slack` the run is one message in the channel, kept up to date: posted when it starts, edited every 10 seconds with a progress bar, requests, requests a second, p50, p99 and errors, each anomaly as a reply in its thread, and at the end the verdict in its place. When the run fails and the hook has a `mention`, the mention is a reply shown in the channel too, since an edited message notifies no one. `--slack=#another` posts somewhere else; in a file it's `webhook = "slack:"` or a `[[webhook]]` table with `url = "slack:#load-tests"`, which takes `on`, `every` and `mention` like any other.

`pepe slack connect` needs no form filled in and nothing hosted: the app is yours, in your workspace.

1. It opens Slack's "create an app" page with everything already said (name, permissions) in the link. You pick the workspace and click **Create**, then **Install to Workspace** and **Allow**.
2. You paste the **Bot User OAuth Token** (`xoxb-…`) it shows. pepe checks it with Slack straight away.
3. You say which channel, by name or part of one. pepe joins it (a private one needs `/invite @pepe` first), posts a test message there, and keeps the token and the channel in `slack.toml` beside the settings, readable by you alone.

The same steps are on the [webhook page](#from-the-screens): `S` there connects Slack without leaving the run, and adds the hook. `pepe slack status` checks the connection, `pepe slack test` posts a test message, `pepe slack forget` removes the file, and `pepe slack manifest` prints the app's manifest, for Slack's "From a manifest" if the link doesn't fill the page in. In CI, `PEPE_SLACK_TOKEN` stands in for the file: `PEPE_SLACK_TOKEN=xoxb-… pepe --slack=#load-tests …`.

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

F3 opens the webhook page over any screen. On the setup form it sets the hooks of the run you're about to start; while a run goes, a hook added or changed there is sent to from then on, so a long soak can be pointed at a channel after it started. Paste or type a URL on the last line, or press `S` for Slack (connected right there if it isn't yet); on each hook, 1 to 5 tick the events, ← → change the format, enter edits the URL, the interval and the mention, `t` sends a test message and says what came back, del removes it. What the page leaves stays for the rest of the session: a restart, ctrl-s and the command printed at the end keep it.

## Delivery

A message never holds the run up. Each webhook has its own queue, sent in order; a dropped connection, a 429 or a 5xx is tried again (three times in all, after the `Retry-After` when there is one). At the end pepe waits up to 15 seconds for the last messages, and says on stderr what couldn't be sent; the exit code is the run's either way. A `--dry-run` lists the webhooks and sends none. Reports and the JSON keep a webhook's URL masked, `https://hooks.slack.com/••••`, since its path is its secret.
