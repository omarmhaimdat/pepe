# 🧪 TESTING - THE QUICK VERSION

## TL;DR - Just Run This:

```bash
cd /Users/omarmhaimdat/pepe
./quick_check.sh
```

That's it! Takes 30 seconds, no hanging.

---

## What Happened Before

The issue was that pepe's TUI would hang waiting for keyboard input during tests. 

**Solution**: We added `(sleep 2 && echo "q")` to pipe a quit command, and `timeout` as a fallback.

---

## Three Test Scripts Available

### 1. `quick_check.sh` ⭐ (Recommended)
- **Fastest**: ~30 seconds
- **Simplest**: Just 8 checks
- **Perfect for**: Verifying everything works
```bash
./quick_check.sh
```

### 2. `test_simple.sh`
- **Medium**: ~1 minute
- **Thorough**: Tests actual functionality
- **Perfect for**: Detailed verification
```bash
./test_simple.sh
```

### 3. `test_improvements.sh`
- **Longest**: 2-3 minutes
- **Most detailed**: Multiple scenarios
- **Perfect for**: Full confidence check
```bash
./test_improvements.sh
```

---

## Manual Testing (If You Want to See Output)

All of these auto-quit, so they won't hang:

```bash
# Test duration feature
(sleep 2 && echo "q") | timeout 10 ./target/release/pepe https://httpbin.org/get -z 5s -c 3

# Test JSON export
(sleep 2 && echo "q") | timeout 15 ./target/release/pepe https://httpbin.org/get -n 20 -c 2 --json

# Test POST
(sleep 2 && echo "q") | timeout 10 ./target/release/pepe https://httpbin.org/post -m POST -d '{}' -n 5 -c 1
```

---

## Expected Results

### From `quick_check.sh`:
```
✅ [1/8] Version works
✅ [2/8] Duration flag exists
✅ [3/8] JSON flag exists
✅ [4/8] JSON module created
✅ [5/8] Release workflow created
✅ [6/8] Windows support added
✅ [7/8] Shell script fixed
✅ [8/8] Dependencies updated

========================================
Results: 8/8 checks passed
==========================================
✨ ALL CHECKS PASSED - Ready to deploy! 🚀
```

---

## What Gets Tested

| Test | What | Feature |
|------|------|---------|
| quick_check.sh | Build + 8 quick verifications | Everything |
| test_simple.sh | Build + integration tests | Duration, JSON, POST, etc |
| test_improvements.sh | Build + detailed scenarios | All edge cases |

---

## Didn't Work? Troubleshooting

### "Binary not found"
```bash
cargo build --release
./quick_check.sh
```

### Tests hung/timeout
- Network issue (try again)
- Or use localhost: `python3 -m http.server 8000` in another terminal

### Command not found
```bash
chmod +x quick_check.sh test_simple.sh test_improvements.sh
```

---

## Next Steps

If all tests pass ✅:

1. **Commit your changes**
   ```bash
   git add -A
   git commit -m "Add duration, JSON, Windows support"
   ```

2. **Create release tag**
   ```bash
   git tag v0.3.0
   git push origin v0.3.0
   ```

3. **Done!** GitHub Actions handles the rest

---

## Summary

| Need | Run This | Time |
|------|----------|------|
| Quick check | `./quick_check.sh` | 30s |
| More thorough | `./test_simple.sh` | 1m |
| Very detailed | `./test_improvements.sh` | 2-3m |

All scripts auto-quit the TUI, so you won't get stuck! ✨
