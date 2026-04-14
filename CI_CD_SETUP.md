# GitHub Actions CI/CD Setup Guide

## Overview

The new release workflow (`release.yaml`) automates building, testing, and releasing pepe for all platforms.

## Prerequisites

1. **Cloudflare R2** (optional but recommended):
   - R2 account and bucket
   - Access token and secret key
   - Custom domain (e.g., pepe.example.com)

2. **Homebrew Tap** (for macOS):
   - Existing tap repository: `omarmhaimdat/homebrew-pepe`
   - Personal Access Token with repo access

3. **GitHub**:
   - Admin access to omarmhaimdat/pepe repository
   - Admin access to homebrew-pepe repository

## Step-by-Step Setup

### 1. Add Secrets to GitHub

Navigate to: **Settings → Secrets and variables → Actions → New repository secret**

Add these secrets:

#### For R2 Storage (Optional)
```
Name: R2_ACCESS_KEY_ID
Value: [your-cloudflare-r2-access-key]

Name: R2_SECRET_ACCESS_KEY
Value: [your-cloudflare-r2-secret-key]
```

#### For Homebrew Updates
```
Name: HOMEBREW_TAP_TOKEN
Value: [github-personal-access-token-with-repo-scope]
```

To create a GitHub token:
1. Go to Settings → Developer settings → Personal access tokens
2. Click "Generate new token"
3. Select scopes: `repo` (full control of private repositories)
4. Copy the token immediately (you won't see it again)

### 2. Add Variables to GitHub

Navigate to: **Settings → Secrets and variables → Variables → New repository variable**

Add these variables:

```
Name: R2_ENABLED
Value: true

Name: R2_BUCKET
Value: pepe

Name: R2_ENDPOINT
Value: https://[your-account-id].r2.cloudflarestorage.com

Name: R2_REGION
Value: auto
```

To find your account ID:
1. Go to Cloudflare dashboard
2. Navigate to R2
3. Copy the Account ID from the URL or Account Overview

### 3. Create a Release Tag

The workflow triggers on any tag starting with `v`:

```bash
git tag v0.3.0
git push origin v0.3.0
```

The workflow will automatically:
- Build for all platforms
- Create a GitHub Release
- Upload binaries and checksums
- Update Homebrew tap
- Update install script

### 4. Verify the Workflow

1. Go to **Actions** tab
2. Find the "Release" workflow
3. Click on your tag to see build logs
4. Monitor each job (create-release, build-and-upload, update-homebrew, update-install-script)

## Workflow Jobs Explained

### create-release
Creates a GitHub Release for the tag. This generates a unique `upload_url` used by other jobs.

### build-and-upload
Builds binaries for each platform:
- Ubuntu (x86_64 & ARM64)
- macOS (x86_64 & ARM64)
- Windows (x86_64)

For each platform:
1. Compiles the binary
2. Generates SHA256 checksum
3. Uploads binary to R2 (if enabled)
4. Uploads binary to GitHub Release
5. Uploads checksum to GitHub Release

### update-homebrew
Updates the Homebrew formula with:
- New version
- New binary URLs (from GitHub Releases)
- Updated checksums

Pushes changes to `homebrew-pepe` tap repository.

### update-install-script
Updates the `install.sh` script with the new version URL so users get the latest release.

## Testing the Workflow

Before full release, test with a beta tag:

```bash
git tag v0.3.0-beta.1
git push origin v0.3.0-beta.1
```

This creates a pre-release on GitHub without affecting Homebrew updates.

## Rollback Procedure

If something goes wrong:

1. **Delete the tag locally and remotely**:
   ```bash
   git tag -d v0.3.0
   git push origin --delete v0.3.0
   ```

2. **Delete the GitHub Release**:
   - Go to Releases
   - Find the release
   - Click "Delete"

3. **Revert Homebrew changes** (if needed):
   - Manually push old formula to `homebrew-pepe`

## Manual Release (Without CI/CD)

If you need to release manually:

```bash
# Build for all targets
cargo build --release --target x86_64-apple-darwin
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-unknown-linux-gnu
cargo build --release --target aarch64-unknown-linux-gnu
cargo build --release --target x86_64-pc-windows-msvc

# Upload to R2
aws s3 cp target/x86_64-apple-darwin/release/pepe \
  s3://pepe/v0.3.0/x86_64-apple-darwin/pepe \
  --endpoint-url $R2_ENDPOINT

# Update Homebrew tap
# (manual steps in homebrew-pepe repo)

# Create GitHub Release
# (manual on GitHub web interface)
```

## Troubleshooting

### Workflow not triggering
- Check that your tag starts with `v` (e.g., `v1.0.0`)
- Ensure you pushed the tag: `git push origin v1.0.0`
- Check Actions tab for any errors

### Build fails for specific platform
- Check the build logs in the specific job
- Common issues:
  - Missing cross-compilation toolchain
  - Dependency problems on that platform
  - See the CI.yaml for platform-specific setup

### Homebrew update fails
- Check that HOMEBREW_TAP_TOKEN is valid
- Verify homebrew-pepe repository exists
- Ensure token has `repo` scope

### R2 upload fails
- Verify credentials are correct
- Check bucket name matches R2_BUCKET variable
- Test credentials manually: `aws s3 ls s3://pepe --endpoint-url $R2_ENDPOINT`

### GitHub Release creation fails
- GitHub token (GITHUB_TOKEN) is auto-provided
- Check GitHub Actions permissions: Settings → Actions → Permissions

## Security Best Practices

1. **Rotate Credentials Regularly**
   - Cloudflare R2 tokens quarterly
   - GitHub tokens annually

2. **Never Commit Credentials**
   - Verified: credentials in build.sh/linux_build.sh are already removed
   - Use GitHub Secrets instead

3. **Limit Token Scopes**
   - HOMEBREW_TAP_TOKEN: only needs `repo` scope
   - Don't use admin tokens if not needed

4. **Monitor Releases**
   - Review binaries before publishing
   - Check checksums match
   - Sign tags with GPG for additional security

## Advanced Configuration

### Custom Build Flags
Edit `release.yaml` to add custom Rust flags:

```yaml
- name: Build project
  run: cargo build --release --target ${{ matrix.target }}
  env:
    RUSTFLAGS: "-C opt-level=3 -C lto=fat"
```

### Custom Platforms
Add new targets to the `build-and-upload` matrix:

```yaml
- os: ubuntu-latest
  target: aarch64-unknown-linux-musl  # Alpine Linux
  artifact_name: pepe
```

### Conditional Steps
Use `if:` conditions to run steps only for certain platforms:

```yaml
- name: Create Windows installer
  if: matrix.target == 'x86_64-pc-windows-msvc'
  run: |
    # MSI creation script
```

## Monitoring and Alerts

1. **GitHub Actions Email**: Enable in Settings → Notifications
2. **Slack Integration**: Connect via GitHub App
3. **Health Checks**: Monitor release frequency

## References

- [GitHub Actions Documentation](https://docs.github.com/en/actions)
- [Cargo Release Matrix](https://doc.rust-lang.org/cargo/build-cache/build-plan.html)
- [Cloudflare R2 API](https://developers.cloudflare.com/r2/)
- [Cross-compilation with Rust](https://rust-lang.github.io/rustup/cross-compilation.html)
