#!/bin/bash

set -e

# Ensure we're on the main branch
if [[ $(git rev-parse --abbrev-ref HEAD) != "main" ]]; then
    echo "Error: Not on main branch. Please checkout main before releasing."
    exit 1
fi

# Fetch the latest changes from origin
git fetch origin main

# Check if there are any incoming or outgoing changes
local_commit=$(git rev-parse HEAD)
remote_commit=$(git rev-parse origin/main)
base_commit=$(git merge-base HEAD origin/main)

if [[ $local_commit != $remote_commit ]]; then
    if [[ $local_commit == $base_commit ]]; then
        echo "Error: Local main branch is behind origin/main. Please pull latest changes."
        exit 1
    elif [[ $remote_commit != $base_commit ]]; then
        echo "Warning: Local main branch has diverged from origin/main."
        echo "Local and remote have different commits. Please make sure this is intended."
        read -p "Do you want to continue? (y/n) " -n 1 -r
        echo
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            exit 1
        fi
    fi
fi

# Check if working directory is clean
if [[ -n $(git status --porcelain) ]]; then
    echo "Error: Working directory is not clean. Please commit or stash your changes."
    exit 1
fi

# Generate new version based on date
current_date=$(date +"%Y.%-m.%-d")
short_hash=$(git rev-parse --short HEAD)
new_version="${current_date}"
new_tag="v${new_version}-${short_hash}"

echo "Preparing release: $new_tag"

# Find Cargo.toml file
cargo_toml="Cargo.toml"
if [[ ! -f "$cargo_toml" ]]; then
    echo "Error: Cargo.toml not found in current directory."
    exit 1
fi

# Update version in Cargo.toml
echo "Updating version in $cargo_toml to $new_version..."
if [[ "$OSTYPE" == "darwin"* ]]; then
    # macOS sed requires -i with empty string for in-place edit
    sed -i '' "s/^version = \".*\"/version = \"$new_version\"/" "$cargo_toml"
else
    # Linux sed
    sed -i "s/^version = \".*\"/version = \"$new_version\"/" "$cargo_toml"
fi

# Verify the change was made
if ! grep -q "version = \"$new_version\"" "$cargo_toml"; then
    echo "Error: Failed to update version in $cargo_toml"
    exit 1
fi

echo "Version updated successfully in $cargo_toml"

# Stage the Cargo.toml change
git add "$cargo_toml"

# Create gitmoji-based release commit
commit_message="🔖 Release $new_tag"
echo "Creating commit: $commit_message"
git commit -m "$commit_message"

# Create the git tag
echo "Creating tag: $new_tag"
git tag "$new_tag"

# Push changes
echo "Pushing commit and tag to origin/main..."
git push origin main
git push origin "$new_tag"

echo ""
echo "✅ Release completed successfully!"
echo "   Version: $new_version"
echo "   Tag: $new_tag"
echo "   Commit: $(git rev-parse HEAD)"
