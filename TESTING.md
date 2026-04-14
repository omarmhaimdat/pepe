# Quick Testing Checklist

## ⚡ FASTEST WAY: Run Automated Tests

```bash
cd /Users/omarmhaimdat/pepe

# Simple auto-quitting tests (recommended - 30 seconds)
chmod +x test_simple.sh
./test_simple.sh

# OR comprehensive tests with more detail (2-3 minutes)
chmod +x test_improvements.sh
./test_improvements.sh
```

Both scripts auto-quit the TUI so they won't hang! ✨

---

## 1. **Duration Feature** (30 seconds)

```bash
# Build first
cargo build --release

# Test 5-second load test (press 'q' to quit)
./target/release/pepe https://httpbin.org/get -z 5s -c 5

# Test 1-minute load test
./target/release/pepe https://httpbin.org/get --duration 1m -c 10

# Invalid duration (should error)
./target/release/pepe https://httpbin.org/get -z 0s
```

**Expected**: 
- Test runs for specified duration
- Shows live metrics dashboard
- Stops after time expires

---

## 2. **JSON Export** (1 minute)

```bash
# Single request, show JSON (press 'q' to quit, outputs JSON)
./target/release/pepe https://httpbin.org/get -n 10 -c 2 --json

# Save to file
./target/release/pepe https://httpbin.org/get -n 50 -c 5 --json > results.json

# Parse with jq
./target/release/pepe https://httpbin.org/get -n 20 -c 3 --json | jq '.summary'
```

**Expected**:
- JSON printed after pressing 'q'
- Contains `summary` and `requests` fields
- Summary includes latency stats (min, max, avg, median, p95, p99)

---

## 3. **POST Requests** (30 seconds)

```bash
# POST with JSON body
./target/release/pepe https://httpbin.org/post \
  -m POST \
  -d '{"key":"value"}' \
  -H "Content-Type: application/json" \
  -n 5 -c 2

# With JSON output
./target/release/pepe https://httpbin.org/post \
  -m POST \
  -d '{"test":"data"}' \
  -n 3 -c 1 --json
```

**Expected**:
- Requests succeed (200 status)
- Body is included in requests

---

## 4. **Timeout Handling** (30 seconds)

```bash
# Endpoint that delays 10 seconds, but timeout is 5 seconds
./target/release/pepe https://httpbin.org/delay/10 \
  -n 3 -c 1 -t 5

# Should see timeout errors in the UI
```

**Expected**:
- Some requests timeout after 5 seconds
- Metrics show 0 status code or timeout count

---

## 5. **Combined Features** (1 minute)

```bash
# Duration + JSON
./target/release/pepe https://httpbin.org/get \
  --duration 5s -c 5 --json

# Duration + High concurrency
./target/release/pepe https://httpbin.org/get \
  -z 10s -c 50
```

**Expected**:
- Duration works with JSON
- High concurrency completes successfully

---

## 6. **Help & Version** (10 seconds)

```bash
# Show version
./target/release/pepe --version

# Show help (should include new flags)
./target/release/pepe --help | grep -E "duration|json"
```

**Expected**:
- Version displays correctly
- `--duration` and `--json` flags appear in help

---

## 7. **Real-World Test** (5 minutes)

```bash
# Quick health check
./target/release/pepe https://api.github.com/zen -n 100 -c 10 --json > github_test.json

# View results
cat github_test.json | jq '.summary | {total: .total_requests, avg_latency: .latency.avg_ms, p99: .latency.p99_ms}'

# Expected output format:
# {
#   "total": 100,
#   "avg_latency": 45.23,
#   "p99": 120.5
# }
```

---

## 8. **Curl Integration** (optional, 1 minute)

```bash
# Test curl parsing with new features
./target/release/pepe --curl -n 10 -z 5s -- curl 'https://httpbin.org/get' -H 'User-Agent: pepe-test'
```

---

## 9. **Shell Script Fix Verification** (1 minute)

```bash
# Check install.sh has proper quoting
grep 'mkdir -p "$USER_BIN_DIR"' install.sh && echo "✓ Variable quoting fixed"
grep 'cd "$TEMP_DIR"' install.sh && echo "✓ Temp dir fixed"
grep 'EXPECTED_CHECKSUM=$(cat' install.sh && echo "✓ Checksum logic fixed"
```

---

## 10. **GitHub Actions Files** (instant)

```bash
# Verify new workflow file exists
ls -la .github/workflows/release.yaml && echo "✓ Release workflow created"

# Check Windows support in CI
grep 'x86_64-pc-windows-msvc' .github/workflows/CI.yaml && echo "✓ Windows support added"

# Verify files compiled
ls -lh src/json_report.rs && echo "✓ JSON report module exists"
```

---

## Automated Testing

Run the full test suite:

```bash
# Make executable
chmod +x test_improvements.sh

# Run all tests (will run ~2-3 min, uses httpbin.org as test server)
./test_improvements.sh
```

---

## Manual Testing Against Local Server

For better control, test against a local server:

```bash
# Terminal 1: Start a simple HTTP server
python3 -m http.server 8000

# Terminal 2: Run pepe tests
./target/release/pepe http://localhost:8000 -z 5s -c 10
./target/release/pepe http://localhost:8000 -n 50 -c 5 --json
```

---

## Performance Benchmarking

Compare performance with different settings:

```bash
# Baseline (1 concurrency)
./target/release/pepe https://httpbin.org/get -n 100 -c 1 --json > baseline.json

# High concurrency
./target/release/pepe https://httpbin.org/get -n 100 -c 50 --json > high_concurrency.json

# Duration-based
./target/release/pepe https://httpbin.org/get -z 10s -c 20 --json > duration_test.json

# Compare results
echo "Baseline RPS:"
cat baseline.json | jq '.summary.requests_per_second'
echo "High concurrency RPS:"
cat high_concurrency.json | jq '.summary.requests_per_second'
```

---

## Troubleshooting

### Binary not found
```bash
cargo build --release
# Binary will be at: ./target/release/pepe
```

### httpbin.org unreachable
```bash
# Use localhost instead (see "Manual Testing Against Local Server")
```

### JSON not showing
```bash
# Make sure to press 'q' or Enter to finish the test before JSON prints
```

### Duration not working
```bash
# Check format: must be like "10s", "5m", "1h"
# Not "10seconds" or "10" alone
```

---

## Success Criteria

✅ You'll know everything works when:

- [ ] Duration tests run for specified time
- [ ] JSON output displays after test completes
- [ ] Combined duration + JSON works
- [ ] Help shows new flags
- [ ] Shell scripts are properly formatted
- [ ] GitHub Actions files are in place
- [ ] No build errors

All green? You're ready to ship! 🚀
