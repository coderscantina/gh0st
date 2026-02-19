# Makefile for gh0st
.PHONY: help build release install test clean fmt lint check run dev all

# Default target
help:
	@echo "gh0st - Makefile commands"
	@echo ""
	@echo "Development:"
	@echo "  make build      - Build debug binary"
	@echo "  make release    - Build optimized release binary"
	@echo "  make install    - Install binary to system"
	@echo "  make run        - Run with example arguments"
	@echo "  make dev        - Run in development mode with logging"
	@echo ""
	@echo "Testing:"
	@echo "  make test       - Run tests"
	@echo "  make test-all   - Run all tests including ignored"
	@echo "  make bench      - Run benchmarks"
	@echo ""
	@echo "Code Quality:"
	@echo "  make fmt        - Format code"
	@echo "  make lint       - Run clippy linter"
	@echo "  make check      - Check compilation without building"
	@echo "  make audit      - Security audit"
	@echo ""
	@echo "Cleanup:"
	@echo "  make clean      - Remove build artifacts"
	@echo "  make clean-all  - Remove all generated files"
	@echo ""
	@echo "Release:"
	@echo "  make dist       - Create distribution archives"
	@echo "  make version    - Show current version"
	@echo ""

# Build commands
build:
	cargo build

release:
	cargo build --release

install:
	cargo install --path .

# Development
run:
	cargo run -- --help

dev:
	RUST_LOG=debug cargo run --

# Testing
test:
	cargo test

test-all:
	cargo test -- --include-ignored

bench:
	cargo bench

# Code quality
fmt:
	cargo fmt

lint:
	cargo clippy --all-targets --all-features -- -D warnings

check:
	cargo check --all-targets --all-features

audit:
	cargo audit

# Cleanup
clean:
	cargo clean

clean-all: clean
	rm -rf target/
	rm -f Cargo.lock

# Distribution
dist: release
	@echo "Creating distribution archives..."
	@mkdir -p dist
	@cd target/release && tar czf ../../dist/gh0st-$$(uname -s | tr '[:upper:]' '[:lower:]')-$$(uname -m).tar.gz gh0st
	@echo "Distribution archive created in dist/"

# Version info
version:
	@grep '^version' Cargo.toml | sed 's/version = "\(.*\)"/Version: \1/'

# Pre-commit checks
pre-commit: fmt lint test
	@echo "✓ Pre-commit checks passed!"

# Build all targets
all: fmt lint test release
	@echo "✓ All tasks completed successfully!"

# Documentation
docs:
	cargo doc --no-deps --open

docs-build:
	cargo doc --no-deps

# Watch for changes (requires cargo-watch)
watch:
	cargo watch -x build

watch-test:
	cargo watch -x test

watch-run:
	cargo watch -x run

# Update dependencies
update:
	cargo update

# Check outdated dependencies (requires cargo-outdated)
outdated:
	cargo outdated

# Cross-compilation targets
build-linux-musl:
	cargo build --release --target x86_64-unknown-linux-musl

build-macos:
	cargo build --release --target x86_64-apple-darwin

build-windows:
	cargo build --release --target x86_64-pc-windows-msvc

# Release preparation
prepare-release: all
	@echo "Checking version..."
	@grep '^version' Cargo.toml
	@echo ""
	@echo "✓ Release preparation complete!"
	@echo ""
	@echo "Next steps:"
	@echo "1. Update CHANGELOG.md"
	@echo "2. Commit changes: git commit -m 'chore: prepare release vX.Y.Z'"
	@echo "3. Create tag: git tag vX.Y.Z"
	@echo "4. Push: git push origin main --tags"
