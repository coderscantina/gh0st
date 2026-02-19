# Release Process

This document describes the release process for gh0st using Calendar Versioning (CalVer).

## Versioning Scheme

We use Calendar Versioning with the format `YYYY.M.D`:

- `YYYY` - Full year (e.g., 2026)
- `M` - Month without zero-padding (e.g., 2 for February, 12 for December)
- `D` - Day without zero-padding (e.g., 19)

Example: `v2026.2.19` for a release on February 19, 2026

## Release Checklist

### 1. Prepare the Release

- [ ] Ensure all desired features/fixes are merged to `main`
- [ ] Run full test suite: `make test-all`
- [ ] Run linter: `make lint`
- [ ] Check for security vulnerabilities: `cargo audit`
- [ ] Update dependencies if needed: `cargo update`
- [ ] Build and test release binary: `make release`

### 2. Update Documentation

- [ ] Update `CHANGELOG.md` with all changes since last release
- [ ] Update version in `Cargo.toml`
- [ ] Update version references in `README.md` if needed
- [ ] Review and update `SECURITY.md` supported versions table
- [ ] Verify all documentation is accurate

### 3. Create Release Commit

```bash
# Set version variable
VERSION="2026.2.19"

# Update Cargo.toml version
sed -i 's/^version = ".*"/version = "'"$VERSION"'"/' Cargo.toml

# Commit changes
git add Cargo.toml CHANGELOG.md
git commit -m "chore: prepare release v$VERSION"
```

### 4. Create and Push Tag

```bash
# Create annotated tag
git tag -a "v$VERSION" -m "Release v$VERSION"

# Push commit and tag
git push origin main
git push origin "v$VERSION"
```

### 5. GitHub Actions Automation

Once the tag is pushed, GitHub Actions will automatically:

1. Create a GitHub Release
2. Build binaries for all supported platforms:
   - Linux (x86_64, aarch64, musl)
   - macOS (Intel, Apple Silicon)
   - Windows (x86_64)
3. Upload release artifacts
4. Generate checksums

### 6. Verify Release

- [ ] Check GitHub Actions workflow completed successfully
- [ ] Verify all platform binaries are attached to the release
- [ ] Download and test at least one binary
- [ ] Verify checksums match
- [ ] Test installation scripts work with new release

### 7. Announce Release

- [ ] Update release notes on GitHub if needed
- [ ] Announce on relevant channels (social media, mailing list, etc.)
- [ ] Update any external documentation or websites
- [ ] Close related issues and pull requests

## Quick Release Command

For a streamlined release process:

```bash
#!/bin/bash
# save as scripts/release.sh

set -e

VERSION=$(date +"%Y.%-m.%-d")

echo "Preparing release v$VERSION"

# Run checks
echo "Running tests..."
cargo test

echo "Running clippy..."
cargo clippy -- -D warnings

echo "Running audit..."
cargo audit

# Update version
echo "Updating version in Cargo.toml..."
sed -i '' "s/^version = \".*\"/version = \"$VERSION\"/" Cargo.toml

# Prompt for changelog
echo ""
echo "Please update CHANGELOG.md with release notes"
echo "Press Enter when ready to continue..."
read

# Commit and tag
git add Cargo.toml CHANGELOG.md
git commit -m "chore: prepare release v$VERSION"
git tag -a "v$VERSION" -m "Release v$VERSION"

echo ""
echo "Release prepared! To publish, run:"
echo "  git push origin main"
echo "  git push origin v$VERSION"
```

## Hotfix Process

For critical bug fixes between regular releases:

1. Create a hotfix branch from the release tag
2. Apply the minimal fix required
3. Update CHANGELOG.md with hotfix notes
4. Create a new release with incremented day number
5. Follow normal release process

Example:

```bash
# If current release is v2026.2.19
git checkout -b hotfix/v2026.2.20 v2026.2.19

# Make fixes
git commit -m "fix: critical security issue"

# Merge to main
git checkout main
git merge hotfix/v2026.2.20

# Tag new version
git tag -a v2026.2.20 -m "Hotfix release v2026.2.20"
git push origin main --tags
```

## Release Artifacts

Each release includes:

### Binaries

- `gh0st-linux-x86_64.tar.gz` - Linux x86_64
- `gh0st-linux-x86_64-musl.tar.gz` - Linux x86_64 (static, musl)
- `gh0st-linux-aarch64.tar.gz` - Linux ARM64
- `gh0st-macos-x86_64.tar.gz` - macOS Intel
- `gh0st-macos-aarch64.tar.gz` - macOS Apple Silicon
- `gh0st-windows-x86_64.zip` - Windows x86_64

### Checksums

- `checksums.txt` - SHA256 checksums for all binaries

## Post-Release Tasks

- [ ] Monitor for issues reported by users
- [ ] Update any deployment/installation documentation
- [ ] Create GitHub milestone for next release
- [ ] Triage and prioritize issues for next release

## Rollback Procedure

If a release has critical issues:

1. Document the issue in GitHub
2. Create a hotfix immediately or
3. Remove problematic release:

   ```bash
   # Delete tag locally and remotely
   git tag -d v2026.2.19
   git push origin :refs/tags/v2026.2.19

   # Delete GitHub release through UI
   ```

4. Communicate the issue to users
5. Release fixed version as soon as possible

## Calendar Versioning Benefits

- **Clear chronological ordering** - Easy to see release age
- **No semantic meaning** - Avoid debates about major/minor/patch
- **Predictable** - Easy to reference specific dates
- **Flexible** - Multiple releases per day if needed

## Troubleshooting

### GitHub Actions Fails

1. Check workflow logs in GitHub Actions tab
2. Common issues:
   - Credential problems (check GITHUB_TOKEN)
   - Build failures (test locally first)
   - Missing dependencies (update CI workflow)

### Binary Doesn't Work

1. Test on clean system without development environment
2. Check for missing dynamic libraries (use `ldd` on Linux)
3. Consider using musl build for better portability
4. Verify architecture matches target system

### Installation Script Fails

1. Test on fresh VM/container
2. Check for missing dependencies (curl, tar, etc.)
3. Verify download URLs are correct
4. Check GitHub API rate limits

## Resources

- [Calendar Versioning](https://calver.org/)
- [Keep a Changelog](https://keepachangelog.com/)
- [GitHub Actions Documentation](https://docs.github.com/en/actions)
- [Semantic Release](https://semver.org/) (for comparison)

## Contact

For release-related questions, contact the maintainers through GitHub issues or discussions.
