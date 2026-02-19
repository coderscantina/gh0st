# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project uses [Calendar Versioning](https://calver.org/) with the format `YYYY.M.D`.

## [Unreleased]

## [2026.2.19] - 2026-02-19

### Added

- TUI (Terminal User Interface) web crawler powered by spider library
- Real-time crawl statistics and monitoring
- Comprehensive SEO analysis and scoring system
- Multiple export formats (CSV and JSON) with live streaming
- Review mode to load and analyze previous crawl results
- WebDriver integration for JavaScript-heavy websites
- Automatic browser automation (Chrome, Firefox, Edge, Safari)
- Configurable crawl scope (subdomains, TLD, depth)
- robots.txt compliance support
- Sitemap discovery and parsing for seed URLs
- Interactive TUI with multiple panels:
  - Pages panel with detailed metrics
  - Issues panel with SEO problem distribution
  - Summary panel with crawl statistics
- SEO issue detection:
  - Missing or suboptimal titles and meta descriptions
  - H1 tag issues
  - Low word count
  - Images without alt text
  - HTTP errors (4xx, 5xx)
  - Noindex directives
  - Missing canonical tags
- Flexible command-line options for crawl configuration
- Concurrent crawling with configurable concurrency
- Retry logic for failed requests
- Custom User-Agent support
- Delay between requests for respectful crawling
- Auto-close option for unattended operation
- Headless mode for CI/CD integration

### Technical

- Built with Rust for performance and safety
- Async runtime powered by Tokio
- TUI framework: ratatui
- WebDriver protocol integration
- Streaming CSV/JSON writers for memory efficiency
- Cross-platform support (Linux, macOS, Windows)

[Unreleased]: https://github.com/yourusername/gh0st/compare/v2026.2.19...HEAD
[2026.2.19]: https://github.com/yourusername/gh0st/releases/tag/v2026.2.19
