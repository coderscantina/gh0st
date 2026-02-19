# gh0st

> **Phantom-fast web crawling and SEO analysis in your terminal** 👻

[![CI](https://github.com/yourusername/gh0st/workflows/CI/badge.svg)](https://github.com/yourusername/gh0st/actions/workflows/ci.yml)
[![Release](https://github.com/yourusername/gh0st/workflows/Release/badge.svg)](https://github.com/yourusername/gh0st/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust Version](https://img.shields.io/badge/rust-1.70%2B-blue.svg)](https://www.rust-lang.org)

A powerful TUI (Terminal User Interface) web crawler and SEO analyzer powered by the Spider library. Crawl websites, analyze SEO metrics, and export comprehensive reports in real-time.

## Features

- 🕷️ **Fast Concurrent Crawling** - Multi-threaded web crawling with configurable concurrency
- 📊 **Real-time TUI** - Interactive terminal interface with live crawl statistics
- 🔍 **SEO Analysis** - Comprehensive SEO scoring and issue detection
- 🌐 **WebDriver Support** - Optional browser automation for JavaScript-heavy sites
- 📁 **Multiple Export Formats** - CSV and JSON output with live streaming
- 🔄 **Review Mode** - Load and review previous crawl results
- 🎯 **Flexible Targeting** - Control crawl scope with subdomains, TLD, and depth options
- 🤖 **robots.txt Support** - Optional robots.txt compliance
- 🗺️ **Sitemap Discovery** - Automatic sitemap parsing for seed URLs

## Installation

> **Note**: Replace `yourusername` with your actual GitHub username in all URLs and configuration files.

### Quick Install (Recommended)

#### Linux / macOS

```bash
curl -fsSL https://raw.githubusercontent.com/yourusername/gh0st/main/install.sh | bash
```

Or download and run manually:

```bash
wget https://raw.githubusercontent.com/yourusername/gh0st/main/install.sh
chmod +x install.sh
./install.sh
```

#### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/yourusername/gh0st/main/install.ps1 | iex
```

Or download and run manually:

```powershell
Invoke-WebRequest -Uri https://raw.githubusercontent.com/yourusername/gh0st/main/install.ps1 -OutFile install.ps1
.\install.ps1
```

### Binary Release

Download the latest release for your platform from the [releases page](https://github.com/yourusername/gh0st/releases).

Available platforms:

- Linux (x86_64, aarch64, musl)
- macOS (Intel, Apple Silicon)
- Windows (x86_64)

### From Source

```bash
git clone https://github.com/yourusername/gh0st.git
cd gh0st
cargo install --path .
```

### Using Cargo

```bash
cargo install gh0st
```

### Docker

Build and run using Docker:

```bash
# Build the image
docker build -t gh0st .

# Run a crawl
docker run --rm -v $(pwd)/output:/data gh0st https://example.com -o /data/results.csv --no-tui
```

Or use docker-compose:

```bash
# Edit docker-compose.yml to set your target URL and options
docker-compose up
```

With WebDriver support:

```bash
# Start services including Firefox WebDriver
docker-compose up firefox gh0st

# Run crawl with WebDriver
docker run --rm \
  --link gh0st-webdriver:webdriver \
  -v $(pwd)/output:/data \
  gh0st https://example.com \
  --webdriver \
  --webdriver-url http://webdriver:4444 \
  -o /data/results.csv \
  --no-tui
```

## Quick Start

### Basic Crawl

```bash
gh0st https://example.com
```

### Crawl with CSV Output

```bash
gh0st https://example.com -o crawl-results.csv
```

### Crawl with JSON Output

```bash
gh0st https://example.com -o results.json --format json
```

### Review Previous Crawl

```bash
gh0st --review crawl-results.csv
```

### Headless Mode (No TUI)

```bash
gh0st https://example.com -o results.csv --no-tui
```

## Usage

```
Usage: gh0st [OPTIONS] [URL]

Arguments:
  [URL]  Target URL to crawl

Options:
  -o, --output <FILE>                    Output file path
      --format <FORMAT>                  Output format [default: csv] [possible values: csv, json]
      --review <FILE>                    Review mode - load and analyze previous crawl results
      --subdomains                       Include subdomains in crawl scope
      --tld                              Include all TLD variants in crawl scope
      --respect-robots                   Respect robots.txt rules
      --full-resources                   Crawl all resources (images, CSS, JS, etc.)
      --seed-sitemap                     Discover and use sitemap URLs as seeds [default: true]
      --channel-capacity <N>             Internal channel capacity [default: 4096]
      --retry-missing <N>                Number of retry attempts for failed URLs [default: 3]
      --retry-5xx <N>                    Number of 5xx requeue rounds [default: 2]
      --fetch-concurrency <N>            Number of concurrent fetch operations [default: 12]
      --depth <N>                        Maximum crawl depth
      --delay-ms <MS>                    Delay between requests in milliseconds
      --user-agent <UA>                  Custom User-Agent string
      --auto-close                       Automatically close when crawl completes
      --no-tui                           Run without TUI (headless mode)

WebDriver Options:
      --webdriver                        Enable WebDriver for JavaScript rendering
      --webdriver-url <URL>              WebDriver endpoint URL [default: http://localhost:4444]
      --webdriver-allowed-ips <IPS>      Allowed IP addresses for WebDriver
      --webdriver-required               Require WebDriver for all pages
      --webdriver-fallback               Fallback to WebDriver if normal fetch fails
      --webdriver-binary <PATH>          Path to WebDriver binary
      --no-webdriver-autostart           Disable automatic WebDriver startup
      --webdriver-start-timeout-ms <MS>  WebDriver startup timeout [default: 12000]
      --webdriver-browser <BROWSER>      Browser to use with WebDriver [default: firefox]
                                         [possible values: chrome, firefox, edge, safari]
      --webdriver-headless               Run browser in headless mode

  -h, --help                             Print help
  -V, --version                          Print version
```

## TUI Controls

### Navigation

- **Tab** / **Shift+Tab** - Switch between panes (table/details, issue lists)
- **↑** / **↓** - Navigate table rows
- **←** / **→** - Switch between sub-panels
- **Page Up** / **Page Down** - Scroll quickly through results
- **Home** / **End** - Jump to first/last row

### Actions

- **Enter** - View page details (in Pages panel)
- **o** - Open URL in default browser
- **r** - Cycle sort mode (Latest, Status, Lowest SEO Score, Highest Response Time)
- **d** - Toggle sort direction (Ascending/Descending)
- **+ / -** - Increase/decrease fetch concurrency live
- **R** - Open retry prompt (failed only or complete refresh)
- **t** - Retry selected URL entry
- **f** - Apply filter by status code or issue type
- **/** - Search (when implemented)

Filter supports status queries such as `status:404`, `4xx`, `5xx`, and `not_retrieved`.

### General

- **q** / **Ctrl+C** - Quit application
- **r** - Refresh display
- **Esc** - Return to table view (from details)

## Output Format

### CSV Columns

The CSV export includes the following columns:

- URL, Status Code, MIME Type, Retrieval Status, Indexability
- Title, Title Length, Meta Description, Meta Description Length
- H1, Canonical URL, Word Count, Page Size (bytes)
- Response Time (ms), Last Modified, Redirect URL, Redirect Type
- Link Count, Internal Links, External Links
- H1 Count, H2 Count, Image Count, Images Missing Alt
- Structured Data Count, SEO Score
- Issue Count, Issues (comma-separated), Outgoing Links (JSON array)
- Crawl Timestamp, Crawl Quality Bucket

### JSON Format

JSON export contains an array of page objects with the same fields as CSV, properly typed.

## SEO Analysis

### Metrics Tracked

- **Title** - Presence, length (30-60 chars optimal)
- **Meta Description** - Presence, length (120-160 chars optimal)
- **H1 Tags** - Presence and uniqueness (exactly 1 recommended)
- **Canonical URL** - Presence of canonical link
- **Word Count** - Content volume (300+ words recommended)
- **Alt Text** - Image accessibility
- **Structured Data** - Schema.org markup detection
- **External Links** - Link profile analysis

### SEO Score

Each page receives a score from 0-100 based on:

- Technical accessibility (retrievability, status codes)
- On-page optimization (title, meta, headings)
- Content quality (word count)
- Link structure (internal/external balance)

### Issue Detection

The tool automatically identifies:

- Missing or suboptimal titles/meta descriptions
- Missing H1 or multiple H1 tags
- Low word count
- Images without alt text
- HTTP errors (4xx, 5xx)
- Noindex directives
- Missing canonical tags

## WebDriver Mode

For crawling JavaScript-heavy websites that require browser rendering:

### Firefox (Default)

```bash
gh0st https://example.com --webdriver
```

The tool will automatically download and manage geckodriver and Firefox if needed.

### Chrome

```bash
gh0st https://example.com --webdriver --webdriver-browser chrome
```

### Custom WebDriver Instance

```bash
# Start your own WebDriver instance
chromedriver --port=9515

# Connect to it
gh0st https://example.com --webdriver --webdriver-url http://localhost:9515 --no-webdriver-autostart
```

## Advanced Examples

### Deep Crawl with Subdomains

```bash
gh0st https://example.com --subdomains --depth 10 -o deep-crawl.csv
```

### Respectful Crawling

```bash
gh0st https://example.com --respect-robots --delay-ms 1000 --fetch-concurrency 5
```

### JavaScript Site with Headless Chrome

```bash
gh0st https://spa-site.com --webdriver --webdriver-browser chrome --webdriver-headless
```

### Export for Analysis

```bash
# Crawl and export
gh0st https://example.com -o results.csv --auto-close --no-tui

# Review in TUI
gh0st --review results.csv
```

## Architecture

- **Core**: Built on the [spider](https://crates.io/crates/spider) library for efficient crawling
- **TUI**: Uses [ratatui](https://crates.io/crates/ratatui) for the terminal interface
- **WebDriver**: Integrates with Selenium WebDriver protocol for browser automation
- **Export**: Streaming CSV/JSON writers for memory-efficient output
- **Async**: Tokio-based async runtime for concurrent operations

## Performance Tips

1. **Adjust Concurrency** - Increase `--fetch-concurrency` for faster crawls on high-bandwidth connections
2. **Use Delays** - Add `--delay-ms` to be respectful to target servers
3. **Limit Depth** - Set `--depth` to avoid over-crawling large sites
4. **Skip Resources** - Don't use `--full-resources` unless you need CSS/JS/images
5. **Headless Mode** - Use `--no-tui` for faster performance when you don't need interactive monitoring

## Docker Usage

### Building the Image

```bash
docker build -t gh0st:latest .
```

### Running a Crawl

Basic crawl without TUI:

```bash
docker run --rm -v $(pwd)/output:/data gh0st:latest \
  https://example.com \
  -o /data/results.csv \
  --no-tui
```

With WebDriver (using docker-compose):

```bash
# Start WebDriver service
docker-compose up -d firefox

# Run crawl
docker run --rm \
  --network container:gh0st-webdriver \
  -v $(pwd)/output:/data \
  gh0st:latest \
  https://example.com \
  --webdriver \
  --webdriver-url http://localhost:4444 \
  -o /data/results.csv \
  --no-tui

# Stop services
docker-compose down
```

### Environment Variables

- `RUST_BACKTRACE=1` - Enable detailed error traces
- `RUST_LOG=debug` - Enable debug logging

## Troubleshooting

### WebDriver Issues

**Problem**: WebDriver fails to start

- **Solution**: Ensure you have internet connectivity for automatic downloads, or manually install geckodriver/chromedriver

**Problem**: Port already in use

- **Solution**: The tool will automatically find a free port, or specify a custom `--webdriver-url`

### Memory Usage

**Problem**: High memory usage on large crawls

- **Solution**: Use `--no-tui` mode and stream to disk, or limit crawl scope with `--depth`

### Crawl Speed

**Problem**: Crawl is too slow

- **Solution**: Increase `--fetch-concurrency` and disable `--webdriver` if not needed

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

MIT License - Copyright (c) 2026 Michael Wallner

See [LICENSE](LICENSE) file for details.

## Acknowledgments

- [spider](https://github.com/spider-rs/spider) - The core crawling engine
- [ratatui](https://github.com/ratatui-org/ratatui) - Terminal UI framework
- [tokio](https://github.com/tokio-rs/tokio) - Async runtime

## Support

For issues, questions, or contributions, please visit the [GitHub repository](https://github.com/yourusername/gh0st).
