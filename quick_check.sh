#!/bin/bash

# ONE-LINE TESTING
# Just run: ./quick_check.sh

cd /Users/omarmhaimdat/pepe

echo "Building and running quick tests..."
echo ""

# Build
if ! cargo build --release 2>&1 | grep -q "Finished"; then
    echo "❌ Build failed"
    exit 1
fi

echo "✅ Build successful"
echo ""

# Quick verification
echo "Running quick checks..."
echo ""

checks_passed=0
checks_total=8

# Check 1: Version
if ./target/release/pepe --version > /dev/null 2>&1; then
    echo "✅ [1/8] Version works"
    ((checks_passed++))
else
    echo "❌ [1/8] Version failed"
fi

# Check 2: Duration flag
if ./target/release/pepe --help 2>&1 | grep -q "duration"; then
    echo "✅ [2/8] Duration flag exists"
    ((checks_passed++))
else
    echo "❌ [2/8] Duration flag missing"
fi

# Check 3: JSON flag
if ./target/release/pepe --help 2>&1 | grep -q "json"; then
    echo "✅ [3/8] JSON flag exists"
    ((checks_passed++))
else
    echo "❌ [3/8] JSON flag missing"
fi

# Check 4: JSON module
if [ -f "src/json_report.rs" ]; then
    echo "✅ [4/8] JSON module created"
    ((checks_passed++))
else
    echo "❌ [4/8] JSON module missing"
fi

# Check 5: Release workflow
if [ -f ".github/workflows/release.yaml" ]; then
    echo "✅ [5/8] Release workflow created"
    ((checks_passed++))
else
    echo "❌ [5/8] Release workflow missing"
fi

# Check 6: Windows support
if grep -q "x86_64-pc-windows-msvc" .github/workflows/CI.yaml; then
    echo "✅ [6/8] Windows support added"
    ((checks_passed++))
else
    echo "❌ [6/8] Windows support missing"
fi

# Check 7: Shell fixes
if grep -q 'mkdir -p "\$USER_BIN_DIR"' install.sh; then
    echo "✅ [7/8] Shell script fixed"
    ((checks_passed++))
else
    echo "❌ [7/8] Shell script not fixed"
fi

# Check 8: Serde JSON dependency
if grep -q "serde_json" Cargo.toml; then
    echo "✅ [8/8] Dependencies updated"
    ((checks_passed++))
else
    echo "❌ [8/8] Dependencies not updated"
fi

echo ""
echo "=========================================="
echo "Results: $checks_passed/$checks_total checks passed"
echo "=========================================="

if [ $checks_passed -eq $checks_total ]; then
    echo "✨ ALL CHECKS PASSED - Ready to deploy! 🚀"
    exit 0
else
    echo "⚠️  Some checks failed - review above"
    exit 1
fi
