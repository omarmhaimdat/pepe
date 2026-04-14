# Quick Start Guide: New Features

## Duration-Based Testing

Run load test for a specific time instead of a fixed number of requests:

```bash
# 10-second test with 50 concurrent connections
pepe https://example.com --duration 10s -c 50

# 5-minute test
pepe https://example.com -z 5m -c 20

# 1-hour sustained test
pepe https://example.com --duration 1h -c 100
```

**Supported Formats**:
- `10s` or `10sec` - seconds
- `5m` or `5min` - minutes  
- `2h` or `2hour` - hours

---

## JSON Export

Export test results in JSON format for automated analysis and integration:

```bash
# Print JSON to stdout
pepe https://example.com -n 1000 -c 50 --json

# Save to file
pepe https://example.com -z 30s -c 20 --json > results.json

# Parse with jq
pepe https://example.com -n 100 -c 10 --json | jq '.summary'
```

**JSON Output Includes**:
- Total requests, successes, failures, timeouts
- Latency percentiles (min, max, avg, median, P95, P99)
- Requests per second
- Data transfer statistics
- HTTP status code breakdown
- Detailed per-request metrics

---

## Deployment Setup

### GitHub Secrets (Add these)
Go to: Settings → Secrets and variables → Actions

```
R2_ACCESS_KEY_ID = your-access-key
R2_SECRET_ACCESS_KEY = your-secret-key
HOMEBREW_TAP_TOKEN = your-github-token
```

### GitHub Variables (Add these)
Go to: Settings → Secrets and variables → Variables

```
R2_ENABLED = true
R2_BUCKET = pepe
R2_ENDPOINT = https://your-account.r2.cloudflarestorage.com
R2_REGION = auto
```

### Release Process
1. Create a new version tag:
   ```bash
   git tag v0.3.0
   git push origin v0.3.0
   ```

2. GitHub Actions automatically:
   - Builds for all platforms (macOS x86_64/ARM64, Linux x86_64/ARM64, Windows)
   - Generates checksums
   - Uploads to R2 (optional)
   - Creates GitHub Release
   - Updates Homebrew tap
   - Updates install script

---

## Bug Fixes Applied

✅ Fixed shell script variable quoting in `install.sh`
✅ Fixed checksum verification logic
✅ Fixed PATH updates in install script
✅ Added Windows build support
✅ Added serde_json dependency

---

## Testing Locally

```bash
# Build in release mode
cargo build --release

# Test duration feature
./target/release/pepe https://httpbin.org/get --duration 5s -c 10

# Test JSON output
./target/release/pepe https://httpbin.org/get -n 50 -c 5 --json

# Test combined (duration + JSON)
./target/release/pepe https://httpbin.org/delay/1 -z 10s -c 3 --json
```

---

## Example Workflows

### Quick Performance Check
```bash
pepe https://api.example.com/health -n 100 -c 10 --json > report.json
```

### Sustained Load Test (5 min)
```bash
pepe https://api.example.com/data --duration 5m -c 50 --json
```

### Spike Test (30 seconds, high concurrency)
```bash
pepe https://api.example.com/endpoint -z 30s -c 200
```

### Export to CSV/Analysis
```bash
pepe https://api.example.com -n 1000 -c 50 --json | \
  jq '.requests[] | [.duration, .status_code, .content_length]' > data.csv
```

---

## Troubleshooting

**Duration too short**: Minimum duration must be > 0ms
```bash
# Invalid: pepe https://example.com -z 0s
# Valid: pepe https://example.com -z 1s
```

**JSON output not showing**: Make sure to press 'q' or Enter to finish test
```bash
# JSON only prints after test completes
pepe https://example.com -n 50 -c 5 --json
# Press 'q' to quit
```

**Windows binary missing from releases**: Check that GitHub Actions Windows job completed successfully in Actions tab

---

## Next Features to Consider

- [ ] Request scenarios (sequential requests with variable substitution)
- [ ] Response validation (assert status/body contains expected values)
- [ ] Configuration files (YAML/TOML for reproducible tests)
- [ ] Real-time metrics export (Prometheus endpoint)
- [ ] Distributed testing across multiple machines
- [ ] Docker image for containerized testing

See `IMPROVEMENTS.md` for complete details!
