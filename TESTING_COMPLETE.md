# ✅ Complete Testing Guide

## The Problem with Manual Tests

Pepe uses a **TUI (Text User Interface)** that requires keyboard input. When you run it without interaction, it hangs waiting for 'q' or Enter.

## The Solution

We've created **two automated test scripts** that:
1. **Auto-quit** the TUI after collecting data
2. **Verify all features** without hanging
3. **Report results** clearly

---

## 🚀 RUN TESTS NOW

### Option 1: Simple Test (Recommended) ⭐
```bash
cd /Users/omarmhaimdat/pepe
./test_simple.sh
```
**What it does:**
- ✅ Verifies build
- ✅ Checks new flags exist
- ✅ Tests duration parsing
- ✅ Tests JSON export
- ✅ Tests POST requests
- ✅ Verifies all files
- ✅ Checks CI/CD config
- ✅ Validates shell fixes

**Time:** ~30 seconds
**Result:** Clear pass/fail for each test

### Option 2: Comprehensive Test
```bash
cd /Users/omarmhaimdat/pepe
./test_improvements.sh
```
**What it does:**
- More detailed testing
- Multiple scenarios for each feature
- Attempts jq parsing if available
- More thorough validation

**Time:** ~2-3 minutes
**Result:** Detailed output for each test

---

## Manual Testing (If You Want to See It Running)

### Duration Feature
```bash
cd /Users/omarmhaimdat/pepe
# Auto-quits after 2 seconds (no hanging)
(sleep 2 && echo "q") | timeout 10 ./target/release/pepe https://httpbin.org/get -z 5s -c 3
```

### JSON Export
```bash
cd /Users/omarmhaimdat/pepe
# Auto-quits after 2 seconds, shows JSON
(sleep 2 && echo "q") | timeout 15 ./target/release/pepe https://httpbin.org/get -n 20 -c 2 --json | head -50
```

### POST Request
```bash
cd /Users/omarmhaimdat/pepe
# Auto-quits after 2 seconds
(sleep 2 && echo "q") | timeout 10 ./target/release/pepe https://httpbin.org/post \
  -m POST -d '{"test":1}' -n 5 -c 1
```

---

## How the Auto-Quit Works

```bash
(sleep 2 && echo "q") | timeout 10 command
```

This means:
- `(sleep 2 && echo "q")` - Wait 2 seconds, then send 'q' keystroke
- `|` - Pipe that to the command
- `timeout 10` - Kill if it runs longer than 10 seconds
- Command receives 'q' as if you typed it → quits automatically

---

## Expected Output from `test_simple.sh`

```
================================================
  PEPE QUICK TESTS (Auto-quitting)
================================================

[1/8] Version Check
✓ Version check passed

[2/8] Help Flags Check
✓ Duration flag found
✓ JSON flag found

[3/8] Duration Parsing
  Testing: -z 5s -n 1
✓ Duration parsing OK

[4/8] JSON Export
  Testing: -n 3 -c 1 --json
✓ JSON output contains expected fields

[5/8] POST Request
  Testing: -m POST -n 2 -c 1
✓ POST request OK

[6/8] File Integrity
✓ json_report.rs exists
✓ release.yaml exists

[7/8] GitHub Actions Config
✓ Windows support in CI
✓ ARM64 Linux support in release workflow

[8/8] Shell Script Fixes
✓ Variable quoting fixed
✓ TEMP_DIR properly quoted

================================================
✨ ALL TESTS COMPLETE!
================================================

You're ready to deploy! 🚀
```

---

## What Each Test Validates

| Test | Validates | Feature |
|------|-----------|---------|
| Version Check | Binary works | Basic functionality |
| Help Flags | `-z` and `--json` flags | CLI implementation |
| Duration Parsing | `-z 5s` syntax | Duration feature |
| JSON Export | `--json` output format | JSON export feature |
| POST Request | `POST -d` body | HTTP methods |
| File Integrity | Files exist | File creation |
| GitHub Actions | Windows & ARM64 | CI/CD setup |
| Shell Fixes | Variable quoting | Installation script |

---

## Verification Checklist

After running tests, verify:

- [ ] All tests show ✓ (passed)
- [ ] Binary successfully built
- [ ] Duration flag recognized
- [ ] JSON flag recognized
- [ ] Files are in place
- [ ] CI/CD configured
- [ ] No errors shown

If all checks pass → **Ready to deploy!** 🚀

---

## Troubleshooting

### Tests hang/timeout
- Increase the sleep time: `sleep 5` instead of `sleep 2`
- Or press Ctrl+C to manually quit

### "Binary not found"
```bash
cargo build --release
# Then run tests again
```

### "Connection refused"
- Network issue with httpbin.org
- Try again or use localhost:8000 (see below)

### Tests passing but no output
- stdout may be captured
- Check exit code: `echo $?` (should be 0)

---

## Local Testing (Optional)

For testing without relying on external services:

```bash
# Terminal 1: Start local server
python3 -m http.server 8000

# Terminal 2: Run tests
cd /Users/omarmhaimdat/pepe
(sleep 2 && echo "q") | timeout 10 ./target/release/pepe http://localhost:8000 \
  -z 5s -c 3
```

---

## Next Steps After Testing

✅ All tests pass?

1. **Commit changes**
   ```bash
   git add -A
   git commit -m "Add duration, JSON export, and Windows support"
   ```

2. **Create release tag** (triggers GitHub Actions)
   ```bash
   git tag v0.3.0
   git push origin v0.3.0
   ```

3. **Watch CI/CD run**
   - Go to Actions tab on GitHub
   - Monitor build jobs for all platforms

4. **Verify release**
   - Check GitHub Releases
   - Download and test binaries

---

## Files Modified/Created

✅ **New Features**
- `src/json_report.rs` - JSON export
- `.github/workflows/release.yaml` - Release automation

✅ **Enhanced Files**
- `src/cli.rs` - Added --duration and --json
- `src/main.rs` - Integrated duration and JSON logic
- `src/ui.rs` - Data export methods
- `src/response.rs` - Serialization support
- `.github/workflows/CI.yaml` - Windows support
- `Cargo.toml` - Added serde_json
- `install.sh` - Fixed variable quoting

✅ **Testing Scripts**
- `test_simple.sh` - Quick automated tests
- `test_improvements.sh` - Comprehensive tests

---

## Summary

You now have:
- ✅ Automated testing that doesn't hang
- ✅ Clear pass/fail verification
- ✅ Manual testing instructions
- ✅ Troubleshooting guide
- ✅ Deployment checklist

**Ready to test and deploy!** 🚀
