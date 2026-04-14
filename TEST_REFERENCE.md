# Testing Reference Card

## 🚀 RECOMMENDED: One-Command Test

```bash
cd /Users/omarmhaimdat/pepe
./test_simple.sh
```

This runs all tests automatically and quits the TUI without hanging. Takes ~30 seconds.

---

## Alternative: Full Test Suite

```bash
cd /Users/omarmhaimdat/pepe
./test_improvements.sh
```

More detailed testing (~2-3 minutes).

---

## Quickest Tests (copy & paste)

### Test 1: Duration Feature Works
```bash
cd /Users/omarmhaimdat/pepe
cargo build --release
# Auto-quits after 2 seconds (won't hang)
(sleep 2 && echo "q") | timeout 10 ./target/release/pepe https://httpbin.org/get -z 5s -c 3
# Should show dashboard running for ~5 seconds
```

### Test 2: JSON Export Works  
```bash
cd /Users/omarmhaimdat/pepe
# Auto-quits after 2 seconds (won't hang)
(sleep 2 && echo "q") | timeout 15 ./target/release/pepe https://httpbin.org/get -n 20 -c 2 --json
# Shows JSON output with "summary" and "requests" fields
```

### Test 3: Help Shows New Flags
```bash
cd /Users/omarmhaimdat/pepe
./target/release/pepe --help | head -50
# Look for:
# -z, --duration <DURATION>    Duration of the test, e.g. 10s, 5m, 2h
# --json                        Output results in JSON format
```

### Test 4: Install Script Fixed
```bash
cd /Users/omarmhaimdat/pepe
grep 'mkdir -p "\$USER_BIN_DIR"' install.sh && echo "✓ FIXED"
grep 'cd "\$TEMP_DIR"' install.sh && echo "✓ FIXED"
```

### Test 5: GitHub Actions Files Exist
```bash
cd /Users/omarmhaimdat/pepe
ls -lh .github/workflows/release.yaml && echo "✓ EXISTS"
ls -lh src/json_report.rs && echo "✓ EXISTS"
```

---

## What Each Test Validates

| Test | Feature | Expected Result |
|------|---------|-----------------|
| Duration | `-z 5s` flag | Test runs for ~5 seconds |
| JSON | `--json` flag | JSON output after test |
| Help | Documentation | New flags listed |
| Scripts | Shell fixes | No $(variable) syntax errors |
| Files | CI/CD setup | Both files present |

---

## Interactive Testing (If You Want More Control)

### Local Test Server
```bash
# Terminal 1
python3 -m http.server 8000

# Terminal 2
cd /Users/omarmhaimdat/pepe
./target/release/pepe http://localhost:8000 -z 3s -c 5
```

### Full Feature Test
```bash
cd /Users/omarmhaimdat/pepe

# 1. Duration test
echo "=== Test 1: Duration ==="
./target/release/pepe https://httpbin.org/get -z 3s -c 3

# 2. JSON test (press q immediately)
echo "=== Test 2: JSON ==="
timeout 10 ./target/release/pepe https://httpbin.org/get -n 10 -c 2 --json || true

# 3. POST test
echo "=== Test 3: POST ==="
./target/release/pepe https://httpbin.org/post -m POST -d '{"test":1}' -n 5 -c 1

# 4. Combined test
echo "=== Test 4: Duration + JSON ==="
timeout 10 ./target/release/pepe https://httpbin.org/get -z 5s -c 5 --json || true
```

---

## Automated Testing

```bash
cd /Users/omarmhaimdat/pepe
chmod +x test_improvements.sh
./test_improvements.sh
```

This runs all tests automatically (~3 minutes)

---

## Key Files to Verify

```bash
cd /Users/omarmhaimdat/pepe

# 1. Duration parsing in CLI
grep -n "pub fn parse_duration" src/cli.rs
# Should show function definition

# 2. JSON report generator
ls -lh src/json_report.rs
# Should be ~3KB file

# 3. UI methods for data export
grep -n "pub fn get_requests" src/ui.rs
# Should show method definition

# 4. Release workflow
ls -lh .github/workflows/release.yaml
# Should be ~5KB file

# 5. Windows in CI
grep "x86_64-pc-windows-msvc" .github/workflows/CI.yaml
# Should find match

# 6. Shell fixes
grep 'mkdir -p "\$USER_BIN_DIR"' install.sh
# Should find match (proper quoting)
```

---

## Compilation Check

```bash
cd /Users/omarmhaimdat/pepe

# Debug build (faster)
cargo build

# Release build (slower but optimized)
cargo build --release

# Check size
ls -lh target/release/pepe
# Should be ~3-4MB
```

---

## Known Limitations for Testing

1. **httpbin.org**: May be rate limited. Use `localhost:8000` if issues
2. **JSON Quitting**: Must press 'q' or Enter to see JSON output
3. **Duration Test**: Needs at least 1 second (e.g., `-z 1s`)
4. **Timeout Tests**: Require time to actually timeout

---

## Verification Checklist

Run these to verify all features work:

```bash
# ✓ Verify build works
cargo build --release && echo "BUILD: OK" || echo "BUILD: FAILED"

# ✓ Verify duration flag exists
./target/release/pepe --help | grep -q "duration" && echo "DURATION FLAG: OK" || echo "DURATION FLAG: MISSING"

# ✓ Verify JSON flag exists
./target/release/pepe --help | grep -q "json" && echo "JSON FLAG: OK" || echo "JSON FLAG: MISSING"

# ✓ Verify json_report module exists
ls src/json_report.rs > /dev/null && echo "JSON MODULE: OK" || echo "JSON MODULE: MISSING"

# ✓ Verify release workflow exists
ls .github/workflows/release.yaml > /dev/null && echo "RELEASE WORKFLOW: OK" || echo "RELEASE WORKFLOW: MISSING"

# ✓ Verify Windows in CI
grep -q "x86_64-pc-windows-msvc" .github/workflows/CI.yaml && echo "WINDOWS SUPPORT: OK" || echo "WINDOWS SUPPORT: MISSING"

# ✓ Verify shell fixes
grep -q 'mkdir -p "\$USER_BIN_DIR"' install.sh && echo "INSTALL SCRIPT: OK" || echo "INSTALL SCRIPT: NEEDS REVIEW"
```

---

## Quick Result: All Green ✅

If all checks pass, you have:

- ✅ Duration-based testing (-z flag)
- ✅ JSON export (--json flag)  
- ✅ Windows support in CI/CD
- ✅ Shell script fixes
- ✅ GitHub Actions automation
- ✅ Release workflow configured

**You're ready to deploy!** 🚀
