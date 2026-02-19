# Contributing to gh0st

Thank you for your interest in contributing to gh0st! This document provides guidelines and instructions for contributing to the project.

## Code of Conduct

By participating in this project, you agree to maintain a respectful and inclusive environment for all contributors.

## How to Contribute

### Reporting Bugs

If you find a bug, please create an issue with:

- A clear, descriptive title
- Steps to reproduce the issue
- Expected behavior vs actual behavior
- Your environment (OS, Rust version, etc.)
- Any relevant logs or screenshots

### Suggesting Features

Feature requests are welcome! Please create an issue with:

- A clear description of the feature
- The problem it solves or use case it addresses
- Any implementation ideas you might have
- Examples from similar tools (if applicable)

### Pull Requests

1. **Fork the repository** and create your branch from `main`
2. **Make your changes** following our coding standards
3. **Test your changes** thoroughly
4. **Update documentation** if needed
5. **Commit your changes** with clear, descriptive messages
6. **Push to your fork** and submit a pull request

#### Pull Request Guidelines

- Keep PRs focused on a single feature or fix
- Write clear commit messages following conventional commits format
- Update the CHANGELOG.md if applicable
- Ensure all tests pass and add new tests for new features
- Follow the existing code style (use `cargo fmt`)
- Address any clippy warnings (`cargo clippy`)

## Development Setup

### Prerequisites

- Rust 1.70 or later
- Cargo (comes with Rust)

### Building from Source

```bash
git clone https://github.com/yourusername/gh0st.git
cd gh0st
cargo build
```

### Running Tests

```bash
cargo test
```

### Running the Application

```bash
cargo run -- https://example.com
```

### Code Style

We use the standard Rust formatting:

```bash
# Format code
cargo fmt

# Check formatting
cargo fmt -- --check

# Run linter
cargo clippy -- -D warnings
```

## Project Structure

```
spider/
├── src/
│   └── main.rs          # Main application code
├── Cargo.toml           # Rust package manifest
├── README.md            # Project documentation
├── LICENSE              # MIT license
├── CHANGELOG.md         # Version history
└── .github/
    └── workflows/       # CI/CD workflows
```

## Coding Standards

### General Guidelines

- Write clear, self-documenting code
- Add comments for complex logic
- Keep functions focused and reasonably sized
- Use meaningful variable and function names
- Handle errors appropriately (don't panic in library code)

### Rust-Specific

- Prefer `Result` over `Option` for error handling
- Use `?` operator for error propagation
- Avoid `unwrap()` and `expect()` in production code paths
- Use strong types over primitives where it improves clarity
- Leverage Rust's type system for compile-time guarantees

### Testing

- Write unit tests for new functions
- Add integration tests for major features
- Test error conditions and edge cases
- Use descriptive test names

Example test:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seo_score_calculation_with_all_issues() {
        let issues = vec![
            SeoIssue::MissingTitle,
            SeoIssue::MissingH1,
            SeoIssue::LowWordCount,
        ];
        let score = compute_seo_score(&issues);
        assert!(score < 50, "Score should be low with multiple issues");
    }
}
```

## Documentation

- Update README.md for user-facing changes
- Add doc comments (`///`) for public APIs
- Update CHANGELOG.md for notable changes
- Include examples in documentation when helpful

## Commit Message Format

We follow the [Conventional Commits](https://www.conventionalcommits.org/) specification:

```
<type>(<scope>): <subject>

<body>

<footer>
```

### Types

- **feat**: New feature
- **fix**: Bug fix
- **docs**: Documentation changes
- **style**: Code style changes (formatting, etc.)
- **refactor**: Code refactoring
- **perf**: Performance improvements
- **test**: Adding or updating tests
- **chore**: Maintenance tasks

### Examples

```
feat(webdriver): add support for Edge browser

Add Edge browser support to WebDriver integration with automatic
binary detection and download.

Closes #123
```

```
fix(tui): prevent panic when terminal is too small

Add minimum terminal size check and display helpful message
when terminal is below minimum dimensions.

Fixes #456
```

## Release Process

Releases use Calendar Versioning (CalVer) with the format `YYYY.M.D`:

1. Update version in `Cargo.toml`
2. Update `CHANGELOG.md` with release notes
3. Commit changes: `git commit -m "chore: prepare release v2026.2.19"`
4. Create and push tag: `git tag v2026.2.19 && git push origin v2026.2.19`
5. GitHub Actions will automatically build and publish release artifacts

## Getting Help

- **Questions**: Open a discussion on GitHub
- **Bugs**: Create an issue with the bug template
- **Chat**: Join our community chat (if available)

## Recognition

Contributors will be recognized in:

- GitHub's contributor list
- Release notes for significant contributions
- README.md (for major features)

## License

By contributing to gh0st, you agree that your contributions will be licensed under the MIT License.

Thank you for contributing to gh0st! 🕷️
