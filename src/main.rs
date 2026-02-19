use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{self, File};
use std::io::{self, Stdout};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use chrono::Utc;
use clap::{ArgAction, Parser, ValueEnum};
use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Gauge, Paragraph, Row, Table, Wrap};
use scraper::{Html, Selector};
use serde::Deserialize;
use serde_json::{Value, json};
use spider::ClientBuilder;
use spider::page::Page;
use spider::website::Website;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinSet;
use url::Url;

#[derive(Debug, Parser, Clone)]
#[command(
    name = "spider-tui",
    version,
    about = "TUI crawler powered by spider with live CSV output"
)]
struct Cli {
    #[arg(value_name = "URL")]
    url: String,

    #[arg(short, long, value_name = "FILE", default_value = "crawl.csv")]
    output: String,

    #[arg(long, default_value_t = false)]
    subdomains: bool,

    #[arg(long, default_value_t = false)]
    tld: bool,

    #[arg(long, default_value_t = false)]
    respect_robots: bool,

    #[arg(long, default_value_t = false)]
    full_resources: bool,

    #[arg(long, default_value_t = true, action = ArgAction::Set)]
    seed_sitemap: bool,

    #[arg(long, value_name = "N", default_value_t = 4096)]
    channel_capacity: usize,

    #[arg(long, value_name = "N", default_value_t = 3)]
    retry_missing: usize,

    #[arg(long, value_name = "N", default_value_t = 12)]
    fetch_concurrency: usize,

    #[arg(long, default_value_t = false)]
    webdriver: bool,

    #[arg(long, value_name = "URL", default_value = "http://localhost:4444")]
    webdriver_url: String,

    #[arg(long, value_name = "IPS")]
    webdriver_allowed_ips: Option<String>,

    #[arg(long, default_value_t = false)]
    webdriver_required: bool,

    #[arg(long, default_value_t = false)]
    webdriver_fallback: bool,

    #[arg(long, value_name = "PATH")]
    webdriver_binary: Option<String>,

    #[arg(long, default_value_t = false)]
    no_webdriver_autostart: bool,

    #[arg(long, value_name = "MS", default_value_t = 12000)]
    webdriver_start_timeout_ms: u64,

    #[arg(long, value_enum, default_value_t = BrowserArg::Firefox)]
    webdriver_browser: BrowserArg,

    #[arg(long, default_value_t = false)]
    webdriver_headless: bool,

    #[arg(long, value_name = "N")]
    depth: Option<usize>,

    #[arg(long, value_name = "MS")]
    delay_ms: Option<u64>,

    #[arg(long, value_name = "UA")]
    user_agent: Option<String>,

    #[arg(long, default_value_t = false)]
    auto_close: bool,

    #[arg(long, default_value_t = false)]
    no_tui: bool,
}

#[derive(Debug, Copy, Clone, ValueEnum, PartialEq, Eq)]
enum BrowserArg {
    Chrome,
    Firefox,
    Edge,
    Safari,
}

#[derive(Debug, Clone)]
struct CrawlRow {
    url: String,
    status: u16,
    mime: String,
    retrieval_status: String,
    indexability: String,
    title: String,
    title_length: usize,
    meta: String,
    meta_length: usize,
    h1: String,
    canonical: String,
    word_count: usize,
    size: usize,
    response_time: u128,
    last_modified: String,
    redirect_url: String,
    redirect_type: String,
    link_count: usize,
    crawl_timestamp: String,
}

#[derive(Debug)]
enum CrawlEvent {
    Page {
        row: CrawlRow,
        discovered_links: Vec<String>,
    },
    Unretrieved {
        url: String,
        reason: String,
    },
    Stats {
        discovered: usize,
    },
    Finished,
    Error(String),
}

#[derive(Default)]
struct AppState {
    parsed: usize,
    discovered_targets: usize,
    rows: VecDeque<CrawlRow>,
    seen: HashSet<String>,
    discovered_seen: HashSet<String>,
    done: bool,
    errors: VecDeque<String>,
    status_counts: HashMap<u16, usize>,
}

impl AppState {
    fn push_row(&mut self, row: CrawlRow, discovered_links: Vec<String>) -> bool {
        for link in discovered_links {
            self.discovered_seen.insert(link);
        }

        let inserted = self.seen.insert(row.url.clone());
        if inserted {
            *self.status_counts.entry(row.status).or_insert(0) += 1;
            self.parsed += 1;
            self.rows.push_front(row);
            while self.rows.len() > 500 {
                self.rows.pop_back();
            }
        }

        inserted
    }

    fn push_error(&mut self, error: String) {
        self.errors.push_front(error);
        while self.errors.len() > 10 {
            self.errors.pop_back();
        }
    }
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let cli = Cli::parse();
    let auto_close = cli.auto_close;
    let no_tui = cli.no_tui;
    let (tx, mut rx) = mpsc::unbounded_channel::<CrawlEvent>();
    let output_path = cli.output.clone();

    let crawl_handle = tokio::spawn(run_crawler(cli, tx));
    let tui_result = if no_tui {
        run_headless(&output_path, &mut rx)
    } else {
        run_tui(&output_path, auto_close, &mut rx)
    };

    if let Err(e) = crawl_handle.await {
        eprintln!("crawler task join error: {e}");
    }

    tui_result
}

fn run_headless(output_path: &str, rx: &mut UnboundedReceiver<CrawlEvent>) -> io::Result<()> {
    let file = File::create(output_path)?;
    let mut writer = csv::Writer::from_writer(file);
    writer.write_record([
        "url",
        "status",
        "mime",
        "retrieval_status",
        "indexability",
        "title",
        "title_length",
        "meta",
        "meta_length",
        "h1",
        "canonical",
        "word count",
        "size",
        "response_time",
        "last_modified",
        "redirect url",
        "redirect type",
        "link count",
        "crawl timestamp",
    ])?;

    let mut state = AppState::default();
    loop {
        while let Ok(event) = rx.try_recv() {
            match event {
                CrawlEvent::Page {
                    row,
                    discovered_links,
                } => {
                    if state.push_row(row.clone(), discovered_links) {
                        writer.write_record([
                            row.url,
                            row.status.to_string(),
                            row.mime,
                            row.retrieval_status,
                            row.indexability,
                            row.title,
                            row.title_length.to_string(),
                            row.meta,
                            row.meta_length.to_string(),
                            row.h1,
                            row.canonical,
                            row.word_count.to_string(),
                            row.size.to_string(),
                            row.response_time.to_string(),
                            row.last_modified,
                            row.redirect_url,
                            row.redirect_type,
                            row.link_count.to_string(),
                            row.crawl_timestamp,
                        ])?;
                    }
                }
                CrawlEvent::Unretrieved { url, reason } => {
                    let row = unretrieved_row(url, reason);
                    if state.push_row(row.clone(), Vec::new()) {
                        writer.write_record([
                            row.url,
                            row.status.to_string(),
                            row.mime,
                            row.retrieval_status,
                            row.indexability,
                            row.title,
                            row.title_length.to_string(),
                            row.meta,
                            row.meta_length.to_string(),
                            row.h1,
                            row.canonical,
                            row.word_count.to_string(),
                            row.size.to_string(),
                            row.response_time.to_string(),
                            row.last_modified,
                            row.redirect_url,
                            row.redirect_type,
                            row.link_count.to_string(),
                            row.crawl_timestamp,
                        ])?;
                    }
                }
                CrawlEvent::Stats { discovered } => {
                    state.discovered_targets = state.discovered_targets.max(discovered);
                }
                CrawlEvent::Finished => state.done = true,
                CrawlEvent::Error(err) => {
                    eprintln!("{err}");
                    state.push_error(err);
                }
            }
        }

        writer.flush()?;
        if state.done {
            break;
        }
        std::thread::sleep(Duration::from_millis(120));
    }

    writer.flush()?;
    eprintln!(
        "finished crawl: parsed={} discovered={} output={}",
        state.parsed,
        state
            .discovered_targets
            .max(state.discovered_seen.len())
            .max(state.parsed),
        output_path
    );
    Ok(())
}

fn run_tui(
    output_path: &str,
    auto_close: bool,
    rx: &mut UnboundedReceiver<CrawlEvent>,
) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let tui_result = draw_loop(&mut terminal, output_path, auto_close, rx);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    tui_result
}

fn draw_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    output_path: &str,
    auto_close: bool,
    rx: &mut UnboundedReceiver<CrawlEvent>,
) -> io::Result<()> {
    let file = File::create(output_path)?;
    let mut writer = csv::Writer::from_writer(file);
    writer.write_record([
        "url",
        "status",
        "mime",
        "retrieval_status",
        "indexability",
        "title",
        "title_length",
        "meta",
        "meta_length",
        "h1",
        "canonical",
        "word count",
        "size",
        "response_time",
        "last_modified",
        "redirect url",
        "redirect type",
        "link count",
        "crawl timestamp",
    ])?;

    let mut state = AppState::default();
    let mut last_tick = Instant::now();
    let tick_rate = Duration::from_millis(120);

    loop {
        while let Ok(event) = rx.try_recv() {
            match event {
                CrawlEvent::Page {
                    row,
                    discovered_links,
                } => {
                    if state.push_row(row.clone(), discovered_links) {
                        writer.write_record([
                            row.url,
                            row.status.to_string(),
                            row.mime,
                            row.retrieval_status,
                            row.indexability,
                            row.title,
                            row.title_length.to_string(),
                            row.meta,
                            row.meta_length.to_string(),
                            row.h1,
                            row.canonical,
                            row.word_count.to_string(),
                            row.size.to_string(),
                            row.response_time.to_string(),
                            row.last_modified,
                            row.redirect_url,
                            row.redirect_type,
                            row.link_count.to_string(),
                            row.crawl_timestamp,
                        ])?;
                    }
                }
                CrawlEvent::Unretrieved { url, reason } => {
                    let row = unretrieved_row(url, reason);
                    if state.push_row(row.clone(), Vec::new()) {
                        writer.write_record([
                            row.url,
                            row.status.to_string(),
                            row.mime,
                            row.retrieval_status,
                            row.indexability,
                            row.title,
                            row.title_length.to_string(),
                            row.meta,
                            row.meta_length.to_string(),
                            row.h1,
                            row.canonical,
                            row.word_count.to_string(),
                            row.size.to_string(),
                            row.response_time.to_string(),
                            row.last_modified,
                            row.redirect_url,
                            row.redirect_type,
                            row.link_count.to_string(),
                            row.crawl_timestamp,
                        ])?;
                    }
                }
                CrawlEvent::Stats { discovered } => {
                    state.discovered_targets = state.discovered_targets.max(discovered);
                }
                CrawlEvent::Finished => state.done = true,
                CrawlEvent::Error(err) => state.push_error(err),
            }
        }

        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(6),
                    Constraint::Length(3),
                    Constraint::Min(10),
                    Constraint::Length(4),
                ])
                .split(f.area());

            let crawl_title = if state.done {
                if auto_close {
                    "Spider Crawl - Finished (auto-closing)"
                } else {
                    "Spider Crawl - Finished (press q to quit)"
                }
            } else {
                "Spider Crawl - Running (press q to quit)"
            };

            let discovered_total = state
                .discovered_targets
                .max(state.discovered_seen.len())
                .max(state.parsed);
            let remaining = discovered_total.saturating_sub(state.parsed);
            let buckets = status_buckets(&state.status_counts);
            let top_codes = top_status_codes(&state.status_counts, 10);
            let metric_label = Style::default().fg(Color::Gray);
            let sep_style = Style::default().fg(Color::DarkGray);
            let header_lines = vec![
                Line::from(vec![
                    Span::styled("Parsed ", metric_label),
                    Span::styled(
                        state.parsed.to_string(),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("  |  ", sep_style),
                    Span::styled("Discovered ", metric_label),
                    Span::styled(
                        discovered_total.to_string(),
                        Style::default()
                            .fg(Color::LightCyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("  |  ", sep_style),
                    Span::styled("Linked ", metric_label),
                    Span::styled(
                        state.discovered_seen.len().to_string(),
                        Style::default().fg(Color::Blue),
                    ),
                    Span::styled("  |  ", sep_style),
                    Span::styled("Remaining ", metric_label),
                    Span::styled(
                        remaining.to_string(),
                        Style::default()
                            .fg(if remaining == 0 {
                                Color::Green
                            } else {
                                Color::Yellow
                            })
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("  |  ", sep_style),
                    Span::styled("CSV ", metric_label),
                    Span::styled(output_path.to_string(), Style::default().fg(Color::White)),
                ]),
                Line::from(vec![
                    Span::styled("Buckets  ", metric_label),
                    Span::styled("2xx ", Style::default().fg(Color::Green)),
                    Span::styled(buckets.c2.to_string(), Style::default().fg(Color::Green)),
                    Span::styled("   3xx ", Style::default().fg(Color::Yellow)),
                    Span::styled(buckets.c3.to_string(), Style::default().fg(Color::Yellow)),
                    Span::styled("   4xx ", Style::default().fg(Color::Red)),
                    Span::styled(buckets.c4.to_string(), Style::default().fg(Color::Red)),
                    Span::styled("   5xx ", Style::default().fg(Color::Magenta)),
                    Span::styled(buckets.c5.to_string(), Style::default().fg(Color::Magenta)),
                    Span::styled("   not_retrieved ", Style::default().fg(Color::LightRed)),
                    Span::styled(
                        buckets.c0.to_string(),
                        Style::default()
                            .fg(Color::LightRed)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from({
                    let mut spans = vec![Span::styled("Top codes  ", metric_label)];
                    if top_codes.is_empty() {
                        spans.push(Span::styled("none", Style::default().fg(Color::DarkGray)));
                    } else {
                        for (idx, (code, count)) in top_codes.into_iter().enumerate() {
                            if idx > 0 {
                                spans.push(Span::raw("  "));
                            }
                            spans.push(Span::styled(
                                format!(
                                    "{}:",
                                    if code == 0 {
                                        "not_retrieved".to_string()
                                    } else {
                                        code.to_string()
                                    }
                                ),
                                status_code_style(code),
                            ));
                            spans.push(Span::styled(
                                count.to_string(),
                                Style::default().fg(Color::White),
                            ));
                        }
                    }
                    spans
                }),
            ];

            let header = Paragraph::new(header_lines)
                .block(
                    Block::default()
                        .title(crawl_title)
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(if state.done {
                            Color::Green
                        } else {
                            Color::Cyan
                        })),
                )
                .wrap(Wrap { trim: true });
            f.render_widget(header, chunks[0]);

            let ratio = if discovered_total == 0 {
                0.0
            } else {
                state.parsed as f64 / discovered_total as f64
            };
            let gauge = Gauge::default()
                .block(Block::default().title("Progress").borders(Borders::ALL))
                .gauge_style(
                    Style::default()
                        .fg(Color::Cyan)
                        .bg(Color::Black)
                        .add_modifier(Modifier::BOLD),
                )
                .ratio(ratio.clamp(0.0, 1.0))
                .label(format!("{:.1}%", ratio * 100.0));
            f.render_widget(gauge, chunks[1]);

            let rows = state.rows.iter().take(200).map(|r| {
                let status_style = if r.status >= 400 {
                    Style::default().fg(Color::Red)
                } else if r.status >= 300 {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default().fg(Color::Green)
                };

                Row::new(vec![
                    Cell::from(r.status.to_string()).style(status_style),
                    Cell::from(r.mime.clone()),
                    Cell::from(r.title.clone()),
                    Cell::from(r.url.clone()),
                ])
            });

            let table = Table::new(
                rows,
                [
                    Constraint::Length(8),
                    Constraint::Length(18),
                    Constraint::Length(36),
                    Constraint::Min(20),
                ],
            )
            .header(
                Row::new(vec!["Status", "Mime", "Title", "URL"])
                    .style(Style::default().add_modifier(Modifier::BOLD)),
            )
            .block(Block::default().title("Latest Pages").borders(Borders::ALL))
            .column_spacing(1);
            f.render_widget(table, chunks[2]);

            let errors = if state.errors.is_empty() {
                "No errors".to_string()
            } else {
                state
                    .errors
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" | ")
            };
            let footer = Paragraph::new(errors)
                .block(Block::default().title("Errors").borders(Borders::ALL))
                .wrap(Wrap { trim: true });
            f.render_widget(footer, chunks[3]);
        })?;

        writer.flush()?;

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') {
                    break;
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }

        if state.done && auto_close {
            break;
        }
    }

    writer.flush()?;
    Ok(())
}

async fn run_crawler(cli: Cli, tx: UnboundedSender<CrawlEvent>) {
    let mut website = Website::new(&cli.url);
    let root_host = Url::parse(&cli.url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()));

    website.configuration.subdomains = cli.subdomains;
    website.configuration.tld = cli.tld;
    website.configuration.return_page_links = true;
    website.configuration.respect_robots_txt = cli.respect_robots;
    website.configuration.full_resources = cli.full_resources;

    // 0 is "no limit" in spider and avoids missing deep paths by default.
    website.configuration.with_depth(cli.depth.unwrap_or(0));

    if let Some(delay) = cli.delay_ms {
        website.configuration.with_delay(delay);
    }
    if let Some(ref ua) = cli.user_agent {
        website.configuration.with_user_agent(Some(ua));
    }
    let redirect_client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(20))
        .build()
        .ok();

    let retry_missing = cli.retry_missing.max(1);
    let mut enable_webdriver = cli.webdriver || cli.webdriver_headless;
    let mut webdriver_url = cli.webdriver_url.clone();
    let mut driver_process: Option<Child> = None;
    let mut active_browser = cli.webdriver_browser;
    if enable_webdriver {
        match prepare_webdriver_backend(&cli, &cli.url, &webdriver_url, &tx).await {
            Ok((ready_endpoint, managed_child, browser)) => {
                webdriver_url = ready_endpoint;
                driver_process = managed_child;
                active_browser = browser;
                let _ = tx.send(CrawlEvent::Error(format!(
                    "WebDriver active browser: {:?}",
                    active_browser
                )));
            }
            Err(err) => {
                if cli.webdriver_required {
                    let _ = tx.send(CrawlEvent::Error(format!(
                        "WebDriver unavailable ({err}); aborting because --webdriver-required is set"
                    )));
                    let _ = tx.send(CrawlEvent::Unretrieved {
                        url: cli.url.clone(),
                        reason: format!("webdriver required but unavailable: {err}"),
                    });
                    let _ = tx.send(CrawlEvent::Finished);
                    return;
                }
                let _ = tx.send(CrawlEvent::Error(format!(
                    "WebDriver unavailable ({err}), falling back to HTTP crawl"
                )));
                enable_webdriver = false;
            }
        }
    }
    if enable_webdriver {
        let depth_limit = cli.depth.unwrap_or(0);
        match browser_discover_and_fetch(
            &webdriver_url,
            active_browser,
            cli.webdriver_headless,
            &cli.url,
            depth_limit,
            cli.seed_sitemap,
            retry_missing,
            cli.fetch_concurrency,
            root_host.as_deref(),
            &tx,
        )
        .await
        {
            Ok(discovered_count) => {
                let _ = tx.send(CrawlEvent::Stats {
                    discovered: discovered_count,
                });
                if discovered_count == 0 {
                    let _ = tx.send(CrawlEvent::Unretrieved {
                        url: cli.url.clone(),
                        reason: "browser backend discovered zero URLs".to_string(),
                    });
                }
                let _ = tx.send(CrawlEvent::Finished);
                stop_webdriver(driver_process);
                return;
            }
            Err(err) => {
                let _ = tx.send(CrawlEvent::Error(format!(
                    "Browser discovery backend failed: {err}"
                )));
                stop_webdriver(driver_process.take());
                if cli.webdriver_required {
                    let _ = tx.send(CrawlEvent::Unretrieved {
                        url: cli.url.clone(),
                        reason: format!("webdriver required but browser discovery failed: {err}"),
                    });
                    let _ = tx.send(CrawlEvent::Finished);
                    return;
                }
                enable_webdriver = false;
            }
        }
    }
    website.configuration.with_webdriver_config(None);

    let mut subscription = match website.subscribe(cli.channel_capacity.max(1)) {
        Some(s) => s,
        None => {
            let _ = tx.send(CrawlEvent::Error(
                "spider sync subscription unavailable (enable `sync` feature)".to_string(),
            ));
            website.crawl().await;
            let discovered = website.get_links().len();
            let _ = tx.send(CrawlEvent::Stats { discovered });
            let _ = tx.send(CrawlEvent::Finished);
            stop_webdriver(driver_process);
            return;
        }
    };

    let seed_sitemap = cli.seed_sitemap;
    let mut seen_urls = HashSet::<String>::new();
    let mut discovered_from_pages = HashSet::<String>::new();
    let crawl_task = tokio::spawn(async move {
        if seed_sitemap {
            let _ = tokio::time::timeout(Duration::from_secs(45), website.crawl_sitemap()).await;
        }
        website.crawl().await;

        website
            .get_links()
            .into_iter()
            .map(|u| u.to_string())
            .collect::<Vec<_>>()
    });

    loop {
        match subscription.recv().await {
            Ok(page) => {
                if let Some(client) = redirect_client.as_ref() {
                    let requested = page.get_url().to_string();
                    let final_url = page.get_url_final().to_string();
                    if requested != final_url {
                        if let Ok((redirect_rows, _)) =
                            raw_redirect_rows(client, &requested, 8).await
                        {
                            for (row, discovered_links) in redirect_rows {
                                let filtered_links =
                                    filter_crawlable_links(discovered_links, root_host.as_deref());
                                let _ = tx.send(CrawlEvent::Page {
                                    row,
                                    discovered_links: filtered_links,
                                });
                            }
                        }
                    }
                }
                let (mut row, discovered_links) = page_to_row(&page);
                let filtered_links = filter_crawlable_links(discovered_links, root_host.as_deref());
                row.link_count = filtered_links.len();
                seen_urls.insert(row.url.clone());
                for link in &filtered_links {
                    discovered_from_pages.insert(link.clone());
                }
                let _ = tx.send(CrawlEvent::Page {
                    row,
                    discovered_links: filtered_links,
                });
            }
            Err(RecvError::Lagged(skipped)) => {
                let _ = tx.send(CrawlEvent::Error(format!(
                    "subscription lagged, skipped {skipped} pages; increase --channel-capacity"
                )));
                continue;
            }
            Err(RecvError::Closed) => break,
        }
    }

    match crawl_task.await {
        Ok(discovered_urls) => {
            let mut candidate_urls = discovered_urls;
            candidate_urls.extend(discovered_from_pages);
            candidate_urls.push(cli.url.clone());

            let mut crawlable_candidates = candidate_urls
                .into_iter()
                .filter_map(|url| normalize_crawl_url(&url))
                .filter(|url| is_same_host(url, root_host.as_deref()))
                .collect::<Vec<_>>();
            crawlable_candidates.sort();
            crawlable_candidates.dedup();
            let discovered = crawlable_candidates.len().max(seen_urls.len());
            let _ = tx.send(CrawlEvent::Stats { discovered });
            let missing_urls = crawlable_candidates
                .into_iter()
                .filter(|url| !seen_urls.contains(url))
                .collect::<Vec<_>>();

            if enable_webdriver && seen_urls.is_empty() {
                let _ = tx.send(CrawlEvent::Error(
                    "WebDriver crawl returned zero pages; falling back to HTTP retrieval for discovered/seed URLs"
                        .to_string(),
                ));
            }

            if !missing_urls.is_empty() {
                let _ = tx.send(CrawlEvent::Error(format!(
                    "reconciling {} crawlable URLs missing page events",
                    missing_urls.len()
                )));
                fetch_missing_urls(
                    missing_urls,
                    retry_missing,
                    cli.fetch_concurrency,
                    root_host.as_deref(),
                    &tx,
                )
                .await;
            }

            let _ = tx.send(CrawlEvent::Finished);
        }
        Err(err) => {
            let _ = tx.send(CrawlEvent::Error(format!("crawl task failed: {err}")));
            let _ = tx.send(CrawlEvent::Finished);
        }
    }
    stop_webdriver(driver_process);
}

fn webdriver_reachable(endpoint: &str) -> bool {
    let parsed = match Url::parse(endpoint) {
        Ok(u) => u,
        Err(_) => return false,
    };
    let host = match parsed.host_str() {
        Some(h) => h,
        None => return false,
    };
    let port = parsed.port_or_known_default().unwrap_or(4444);
    let Ok(addrs) = (host, port).to_socket_addrs() else {
        return false;
    };
    addrs
        .into_iter()
        .any(|addr| TcpStream::connect_timeout(&addr, Duration::from_secs(2)).is_ok())
}

async fn start_webdriver(
    cli: &Cli,
    browser: BrowserArg,
    endpoint: &str,
    tx: &UnboundedSender<CrawlEvent>,
) -> Result<Child, String> {
    let parsed = Url::parse(endpoint).map_err(|e| format!("invalid webdriver url: {e}"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| "webdriver url has no host".to_string())?
        .to_ascii_lowercase();
    if host != "localhost" && host != "127.0.0.1" {
        return Err("autostart only supports localhost endpoints".to_string());
    }
    let port = parsed.port_or_known_default().unwrap_or(4444);

    let mut candidates: Vec<WebDriverLaunchCandidate> = if let Some(bin) = &cli.webdriver_binary {
        vec![WebDriverLaunchCandidate {
            driver_binary: bin.clone(),
            browser_binary: None,
        }]
    } else {
        match browser {
            BrowserArg::Chrome => {
                let mut chrome_candidates = Vec::new();
                if let Ok(bundle) = ensure_chromedriver_bundle(tx).await {
                    chrome_candidates.push(WebDriverLaunchCandidate {
                        driver_binary: bundle.driver_binary.to_string_lossy().to_string(),
                        browser_binary: bundle.browser_binary,
                    });
                }
                chrome_candidates.push(WebDriverLaunchCandidate {
                    driver_binary: "chromedriver".to_string(),
                    browser_binary: None,
                });
                chrome_candidates
            }
            BrowserArg::Firefox => {
                let mut firefox_candidates = Vec::new();
                if let Ok(bundle) = ensure_geckodriver_bundle(tx).await {
                    firefox_candidates.push(WebDriverLaunchCandidate {
                        driver_binary: bundle.driver_binary.to_string_lossy().to_string(),
                        browser_binary: bundle.browser_binary,
                    });
                }
                firefox_candidates.push(WebDriverLaunchCandidate {
                    driver_binary: "geckodriver".to_string(),
                    browser_binary: None,
                });
                firefox_candidates
            }
            BrowserArg::Edge => vec![WebDriverLaunchCandidate {
                driver_binary: "msedgedriver".to_string(),
                browser_binary: None,
            }],
            BrowserArg::Safari => vec![WebDriverLaunchCandidate {
                driver_binary: "safaridriver".to_string(),
                browser_binary: None,
            }],
        }
    };

    candidates.retain(|c| webdriver_binary_available(&c.driver_binary));

    let log_path = webdriver_log_path(port)?;
    let mut last_err = String::new();
    for candidate in candidates {
        let mut cmd = Command::new(&candidate.driver_binary);
        configure_webdriver_command(
            &mut cmd,
            &candidate.driver_binary,
            browser,
            port,
            candidate.browser_binary.as_deref(),
            cli.webdriver_allowed_ips.as_deref(),
        );
        let log_file = File::options()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&log_path)
            .map_err(|e| format!("failed to open webdriver log {}: {e}", log_path.display()))?;
        let log_file_err = log_file
            .try_clone()
            .map_err(|e| format!("failed to clone webdriver log handle: {e}"))?;
        cmd.stdout(Stdio::from(log_file))
            .stderr(Stdio::from(log_file_err))
            .stdin(Stdio::null());

        match cmd.spawn() {
            Ok(mut child) => {
                let steps = (cli.webdriver_start_timeout_ms / 200).max(1);
                for _ in 0..steps {
                    if webdriver_reachable(endpoint) {
                        return Ok(child);
                    }
                    if let Ok(Some(status)) = child.try_wait() {
                        last_err = format!(
                            "{} exited early with status {status} (log: {})",
                            candidate.driver_binary,
                            log_path.display()
                        );
                        if let Some(tail) = read_log_tail(&log_path, 30) {
                            last_err = format!("{last_err}; tail: {tail}");
                        }
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
                let _ = child.kill();
                let _ = child.wait();
                if last_err.is_empty() {
                    last_err = format!(
                        "{} did not become ready in time (log: {})",
                        candidate.driver_binary,
                        log_path.display()
                    );
                    if let Some(tail) = read_log_tail(&log_path, 30) {
                        last_err = format!("{last_err}; tail: {tail}");
                    }
                }
            }
            Err(err) => {
                last_err = format!("failed to spawn {}: {err}", candidate.driver_binary);
                let _ = tx.send(CrawlEvent::Error(last_err.clone()));
            }
        }
    }

    Err(if last_err.is_empty() {
        "no suitable webdriver binary found".to_string()
    } else {
        last_err
    })
}

fn configure_webdriver_command(
    cmd: &mut Command,
    bin: &str,
    browser: BrowserArg,
    port: u16,
    _browser_binary: Option<&Path>,
    allowed_ips: Option<&str>,
) {
    let lower = bin.to_ascii_lowercase();
    if matches!(browser, BrowserArg::Safari) || lower.contains("safaridriver") {
        cmd.arg("--port").arg(port.to_string());
        return;
    }
    if lower.contains("geckodriver") {
        cmd.arg("--port").arg(port.to_string());
        return;
    }
    cmd.arg(format!("--port={port}"));
    cmd.arg("--allowed-origins=*");
    if let Some(ips) = allowed_ips {
        cmd.arg(format!("--allowed-ips={ips}"));
    }
    if lower.contains("chromedriver") {
        cmd.arg("--log-level=SEVERE");
    }
}

async fn prepare_webdriver_backend(
    cli: &Cli,
    target_url: &str,
    requested_endpoint: &str,
    tx: &UnboundedSender<CrawlEvent>,
) -> Result<(String, Option<Child>, BrowserArg), String> {
    let mut errors = Vec::new();
    for browser in browser_candidates(cli) {
        if matches!(browser, BrowserArg::Firefox) && !firefox_binary_available() {
            match ensure_firefox_bundle(tx).await {
                Ok(path) => {
                    let _ = tx.send(CrawlEvent::Error(format!(
                        "bundled Firefox available at {}",
                        path.display()
                    )));
                }
                Err(err) => {
                    let _ = tx.send(CrawlEvent::Error(format!(
                        "Firefox bundle setup failed: {err}"
                    )));
                }
            }
        }
        if matches!(browser, BrowserArg::Firefox) && !firefox_binary_available() {
            let msg = "Firefox binary not found; skipping Firefox WebDriver".to_string();
            let _ = tx.send(CrawlEvent::Error(msg.clone()));
            errors.push(format!("firefox: {msg}"));
            continue;
        }
        match ensure_webdriver_ready(cli, browser, requested_endpoint, tx).await {
            Ok((endpoint, child, active_browser)) => {
                match webdriver_preflight(
                    &endpoint,
                    active_browser,
                    cli.webdriver_headless,
                    target_url,
                )
                .await
                {
                    Ok(()) => {
                        let _ = tx.send(CrawlEvent::Error(format!(
                            "WebDriver preflight succeeded for {:?} at {}",
                            active_browser, endpoint
                        )));
                        return Ok((endpoint, child, active_browser));
                    }
                    Err(err) => {
                        if child.is_some() {
                            stop_webdriver(child);
                        }
                        let msg = format!(
                            "preflight failed for {:?} at {}: {}",
                            active_browser, endpoint, err
                        );
                        let _ = tx.send(CrawlEvent::Error(msg.clone()));
                        errors.push(msg);
                    }
                }
            }
            Err(err) => {
                errors.push(format!("{browser:?}: {err}"));
            }
        }
    }

    if errors.is_empty() {
        Err("no webdriver backend candidates were available".to_string())
    } else {
        Err(errors.join(" || "))
    }
}

fn browser_candidates(cli: &Cli) -> Vec<BrowserArg> {
    let preferred = match cli.webdriver_browser {
        BrowserArg::Safari => BrowserArg::Chrome,
        other => other,
    };

    let mut out = vec![preferred];
    if !cli.webdriver_fallback {
        return out;
    }

    let ordered = match preferred {
        BrowserArg::Firefox => [BrowserArg::Chrome, BrowserArg::Edge],
        BrowserArg::Chrome => [BrowserArg::Firefox, BrowserArg::Edge],
        BrowserArg::Edge => [BrowserArg::Chrome, BrowserArg::Firefox],
        BrowserArg::Safari => [BrowserArg::Chrome, BrowserArg::Firefox],
    };
    for b in ordered {
        if !out.contains(&b) {
            out.push(b);
        }
    }
    out
}

async fn webdriver_preflight(
    endpoint: &str,
    browser: BrowserArg,
    headless: bool,
    target_url: &str,
) -> Result<(), String> {
    let target = Url::parse(target_url).map_err(|e| format!("invalid target url: {e}"))?;
    if !matches!(target.scheme(), "http" | "https") {
        return Err("preflight requires http/https URL".to_string());
    }

    let base = endpoint.trim_end_matches('/').to_string();
    let session_endpoint = format!("{base}/session");
    let caps = webdriver_capabilities(browser, headless);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(25))
        .build()
        .map_err(|e| format!("preflight http client build failed: {e}"))?;

    let create_res = client
        .post(&session_endpoint)
        .json(&caps)
        .send()
        .await
        .map_err(|e| format!("session create request failed: {e}"))?;
    let create_status = create_res.status();
    let create_body = create_res
        .text()
        .await
        .map_err(|e| format!("session create response read failed: {e}"))?;
    if !create_status.is_success() {
        return Err(format!(
            "session create HTTP {}: {}",
            create_status.as_u16(),
            truncate_for_log(&create_body, 260)
        ));
    }

    let create_json: Value = serde_json::from_str(&create_body).map_err(|e| {
        format!(
            "session create parse failed: {e}; body={}",
            truncate_for_log(&create_body, 220)
        )
    })?;
    if let Some(err_name) = create_json.pointer("/value/error").and_then(|v| v.as_str()) {
        let message = create_json
            .pointer("/value/message")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown webdriver error");
        return Err(format!(
            "session create webdriver error {err_name}: {message}"
        ));
    }

    let session_id = create_json
        .pointer("/value/sessionId")
        .and_then(|v| v.as_str())
        .or_else(|| create_json.pointer("/sessionId").and_then(|v| v.as_str()))
        .ok_or_else(|| {
            format!(
                "session create missing sessionId; body={}",
                truncate_for_log(&create_body, 220)
            )
        })?
        .to_string();

    let nav_endpoint = format!("{base}/session/{session_id}/url");
    let nav_res = client
        .post(&nav_endpoint)
        .json(&json!({ "url": target.as_str() }))
        .send()
        .await
        .map_err(|e| format!("navigation request failed: {e}"))?;
    let nav_status = nav_res.status();
    let nav_body = nav_res
        .text()
        .await
        .map_err(|e| format!("navigation response read failed: {e}"))?;

    let delete_endpoint = format!("{base}/session/{session_id}");
    let _ = client.delete(delete_endpoint).send().await;

    if !nav_status.is_success() {
        return Err(format!(
            "navigation HTTP {}: {}",
            nav_status.as_u16(),
            truncate_for_log(&nav_body, 260)
        ));
    }
    let nav_json: Value = serde_json::from_str(&nav_body).unwrap_or_default();
    if let Some(err_name) = nav_json.pointer("/value/error").and_then(|v| v.as_str()) {
        let message = nav_json
            .pointer("/value/message")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown webdriver navigation error");
        return Err(format!("navigation webdriver error {err_name}: {message}"));
    }

    Ok(())
}

fn webdriver_capabilities(browser: BrowserArg, headless: bool) -> Value {
    match browser {
        BrowserArg::Firefox => {
            let mut args = Vec::<String>::new();
            if headless {
                args.push("-headless".to_string());
            }
            let mut firefox_options = json!({ "args": args });
            if let Some(binary) = detect_browser_binary(BrowserArg::Firefox) {
                firefox_options["binary"] = json!(binary.to_string_lossy().to_string());
            }
            json!({
                "capabilities": {
                    "alwaysMatch": {
                        "browserName": "firefox",
                        "acceptInsecureCerts": true,
                        "moz:firefoxOptions": firefox_options
                    }
                }
            })
        }
        BrowserArg::Edge => {
            let mut args = Vec::<String>::new();
            if headless {
                args.push("--headless=new".to_string());
            }
            json!({
                "capabilities": {
                    "alwaysMatch": {
                        "browserName": "MicrosoftEdge",
                        "acceptInsecureCerts": true,
                        "ms:edgeOptions": { "args": args }
                    }
                }
            })
        }
        _ => {
            let mut args = Vec::<String>::new();
            let profile_dir = std::env::temp_dir().join(format!(
                "spider-tui-chrome-profile-{}-{}",
                std::process::id(),
                Utc::now().timestamp_millis()
            ));
            let _ = fs::create_dir_all(&profile_dir);
            args.push(format!("--user-data-dir={}", profile_dir.display()));
            if headless {
                args.push("--headless".to_string());
            }
            args.push("--window-size=1400,1200".to_string());
            args.push("--disable-gpu".to_string());
            args.push("--disable-dev-shm-usage".to_string());
            args.push("--remote-debugging-port=0".to_string());
            args.push("--no-first-run".to_string());
            args.push("--no-default-browser-check".to_string());
            args.push("--disable-crash-reporter".to_string());
            if !cfg!(target_os = "macos") {
                args.push("--no-sandbox".to_string());
            }
            let mut chrome_options = json!({ "args": args });
            if let Some(binary) = detect_browser_binary(BrowserArg::Chrome) {
                chrome_options["binary"] = json!(binary.to_string_lossy().to_string());
            }
            json!({
                "capabilities": {
                    "alwaysMatch": {
                        "browserName": "chrome",
                        "acceptInsecureCerts": true,
                        "goog:chromeOptions": chrome_options
                    }
                }
            })
        }
    }
}

fn detect_browser_binary(browser: BrowserArg) -> Option<PathBuf> {
    match browser {
        BrowserArg::Firefox => {
            if let Ok(v) = std::env::var("FIREFOX_BIN") {
                let p = PathBuf::from(v);
                if p.exists() {
                    return Some(p);
                }
            }
            #[cfg(target_os = "macos")]
            {
                let p = PathBuf::from("/Applications/Firefox.app/Contents/MacOS/firefox");
                if p.exists() {
                    return Some(p);
                }
            }
            if let Ok(cache_dir) = webdriver_cache_dir() {
                let bundled = firefox_bundle_binary_path(&cache_dir.join("firefox-dist"));
                if bundled.exists() {
                    return Some(bundled);
                }
            }
            which_binary_path("firefox")
        }
        BrowserArg::Edge => {
            #[cfg(target_os = "macos")]
            {
                let p =
                    PathBuf::from("/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge");
                if p.exists() {
                    return Some(p);
                }
            }
            which_binary_path("msedge")
        }
        _ => {
            if let Ok(v) = std::env::var("CHROME_BIN") {
                let p = PathBuf::from(v);
                if p.exists() {
                    return Some(p);
                }
            }
            #[cfg(target_os = "macos")]
            {
                let p =
                    PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome");
                if p.exists() {
                    return Some(p);
                }
            }
            if let Some(path) = which_binary_path("google-chrome") {
                return Some(path);
            }
            if let Some(path) = which_binary_path("chrome") {
                return Some(path);
            }
            if let Some(path) = webdriver_cache_dir().ok().and_then(|d| {
                chromedriver_platform()
                    .map(|platform| chrome_binary_path(&d.join("chrome-dist"), platform))
            }) {
                if path.exists() {
                    return Some(path);
                }
            }
            None
        }
    }
}

fn which_binary_path(name: &str) -> Option<PathBuf> {
    let output = Command::new("which").arg(name).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8(output.stdout).ok()?;
    let p = PathBuf::from(path.trim());
    if p.exists() { Some(p) } else { None }
}

fn truncate_for_log(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    input.chars().take(max_chars).collect::<String>() + "..."
}

async fn browser_discover_and_fetch(
    endpoint: &str,
    browser: BrowserArg,
    headless: bool,
    start_url: &str,
    depth_limit: usize,
    seed_sitemap: bool,
    retries: usize,
    fetch_concurrency: usize,
    root_host: Option<&str>,
    tx: &UnboundedSender<CrawlEvent>,
) -> Result<usize, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(40))
        .build()
        .map_err(|e| format!("webdriver client build failed: {e}"))?;
    let fetch_client = ClientBuilder::new()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("fetch client build failed: {e}"))?;
    let redirect_client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(25))
        .pool_max_idle_per_host(32)
        .build()
        .map_err(|e| format!("redirect client build failed: {e}"))?;

    let session_id = webdriver_create_session(&client, endpoint, browser, headless).await?;
    let mut discovered = HashSet::<String>::new();
    let mut visited = HashSet::<String>::new();
    let mut queue = VecDeque::<(String, usize)>::new();
    let root_host_owned = root_host.map(|s| s.to_string());
    let worker_limit = fetch_concurrency.max(1);
    let mut fetch_set: JoinSet<Vec<CrawlEvent>> = JoinSet::new();
    let start_url = normalize_crawl_url(start_url).unwrap_or_else(|| start_url.to_string());

    discovered.insert(start_url.clone());
    queue.push_back((start_url.clone(), 0));
    fetch_set.spawn(process_single_url(
        start_url.clone(),
        retries,
        root_host_owned.clone(),
        fetch_client.clone(),
        redirect_client.clone(),
    ));

    if seed_sitemap {
        for url in discover_sitemap_seed_urls(&client, &start_url, root_host).await {
            if discovered.insert(url.clone()) {
                queue.push_back((url.clone(), 0));
            }
        }
    }

    let unlimited_depth = depth_limit == 0;
    while let Some((url, depth)) = queue.pop_front() {
        if !visited.insert(url.clone()) {
            continue;
        }

        if let Ok((redirect_rows, _)) = raw_redirect_rows(&redirect_client, &url, 8).await {
            for (row, discovered_links) in redirect_rows {
                let filtered_links = filter_crawlable_links(discovered_links, root_host);
                let _ = tx.send(CrawlEvent::Page {
                    row,
                    discovered_links: filtered_links,
                });
            }
        }

        if let Err(err) = webdriver_navigate(&client, endpoint, &session_id, &url).await {
            let _ = tx.send(CrawlEvent::Error(format!(
                "browser navigate failed for {}: {}",
                url, err
            )));
            while fetch_set.len() >= worker_limit {
                if let Some(joined) = fetch_set.join_next().await {
                    emit_joined_fetch_events(joined, tx);
                }
            }
            fetch_set.spawn(process_single_url(
                url.clone(),
                retries,
                root_host_owned.clone(),
                fetch_client.clone(),
                redirect_client.clone(),
            ));
            continue;
        }

        let links = match webdriver_extract_links(&client, endpoint, &session_id).await {
            Ok(v) => v,
            Err(err) => {
                let _ = tx.send(CrawlEvent::Error(format!(
                    "browser link extraction failed for {}: {}",
                    url, err
                )));
                Vec::new()
            }
        };
        let filtered = filter_crawlable_links(links, root_host);

        match webdriver_rendered_snapshot(&client, endpoint, &session_id).await {
            Ok((rendered_url, rendered_html)) => {
                let rendered_url = normalize_crawl_url(&rendered_url).unwrap_or(rendered_url);
                let page = Page::new(&rendered_url, &fetch_client).await;
                let (mut row, _) = page_to_row(&page);
                row.url = rendered_url;
                apply_rendered_html_to_row(&mut row, &rendered_html);
                row.link_count = filtered.len();
                let _ = tx.send(CrawlEvent::Page {
                    row,
                    discovered_links: filtered.clone(),
                });
            }
            Err(err) => {
                let _ = tx.send(CrawlEvent::Error(format!(
                    "browser rendered snapshot failed for {}: {}",
                    url, err
                )));
                while fetch_set.len() >= worker_limit {
                    if let Some(joined) = fetch_set.join_next().await {
                        emit_joined_fetch_events(joined, tx);
                    }
                }
                fetch_set.spawn(process_single_url(
                    url.clone(),
                    retries,
                    root_host_owned.clone(),
                    fetch_client.clone(),
                    redirect_client.clone(),
                ));
            }
        }

        for link in filtered {
            if discovered.insert(link.clone()) {
                if unlimited_depth || depth < depth_limit {
                    queue.push_back((link, depth + 1));
                } else {
                    while fetch_set.len() >= worker_limit {
                        if let Some(joined) = fetch_set.join_next().await {
                            emit_joined_fetch_events(joined, tx);
                        }
                    }
                    fetch_set.spawn(process_single_url(
                        link,
                        retries,
                        root_host_owned.clone(),
                        fetch_client.clone(),
                        redirect_client.clone(),
                    ));
                }
            }
        }

        if visited.len() % 10 == 0 {
            let _ = tx.send(CrawlEvent::Stats {
                discovered: discovered.len(),
            });
        }

        while let Some(joined) = fetch_set.try_join_next() {
            emit_joined_fetch_events(joined, tx);
        }
    }

    while let Some(joined) = fetch_set.join_next().await {
        emit_joined_fetch_events(joined, tx);
    }
    let _ = webdriver_delete_session(&client, endpoint, &session_id).await;
    Ok(discovered.len())
}

fn emit_joined_fetch_events(
    joined: Result<Vec<CrawlEvent>, tokio::task::JoinError>,
    tx: &UnboundedSender<CrawlEvent>,
) {
    match joined {
        Ok(events) => {
            for event in events {
                let _ = tx.send(event);
            }
        }
        Err(err) => {
            let _ = tx.send(CrawlEvent::Error(format!("fetch worker failed: {err}")));
        }
    }
}

async fn webdriver_create_session(
    client: &reqwest::Client,
    endpoint: &str,
    browser: BrowserArg,
    headless: bool,
) -> Result<String, String> {
    let base = endpoint.trim_end_matches('/');
    let session_endpoint = format!("{base}/session");
    let caps = webdriver_capabilities(browser, headless);
    let res = client
        .post(&session_endpoint)
        .json(&caps)
        .send()
        .await
        .map_err(|e| format!("session create request failed: {e}"))?;
    let status = res.status();
    let body = res
        .text()
        .await
        .map_err(|e| format!("session create response read failed: {e}"))?;
    if !status.is_success() {
        return Err(format!(
            "session create HTTP {}: {}",
            status.as_u16(),
            truncate_for_log(&body, 260)
        ));
    }

    let value: Value =
        serde_json::from_str(&body).map_err(|e| format!("session create parse failed: {e}"))?;
    if let Some(err) = value.pointer("/value/error").and_then(|v| v.as_str()) {
        let message = value
            .pointer("/value/message")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown webdriver error");
        return Err(format!("{err}: {message}"));
    }
    value
        .pointer("/value/sessionId")
        .and_then(|v| v.as_str())
        .or_else(|| value.pointer("/sessionId").and_then(|v| v.as_str()))
        .map(|s| s.to_string())
        .ok_or_else(|| {
            format!(
                "session id missing in response: {}",
                truncate_for_log(&body, 220)
            )
        })
}

async fn webdriver_navigate(
    client: &reqwest::Client,
    endpoint: &str,
    session_id: &str,
    url: &str,
) -> Result<(), String> {
    let nav_endpoint = format!(
        "{}/session/{}/url",
        endpoint.trim_end_matches('/'),
        session_id
    );
    let res = client
        .post(nav_endpoint)
        .json(&json!({ "url": url }))
        .send()
        .await
        .map_err(|e| format!("navigate request failed: {e}"))?;
    let status = res.status();
    let body = res
        .text()
        .await
        .map_err(|e| format!("navigate response read failed: {e}"))?;
    if !status.is_success() {
        return Err(format!(
            "navigate HTTP {}: {}",
            status.as_u16(),
            truncate_for_log(&body, 240)
        ));
    }
    let value: Value = serde_json::from_str(&body).unwrap_or_default();
    if let Some(err) = value.pointer("/value/error").and_then(|v| v.as_str()) {
        let message = value
            .pointer("/value/message")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown navigate error");
        return Err(format!("{err}: {message}"));
    }
    Ok(())
}

async fn webdriver_extract_links(
    client: &reqwest::Client,
    endpoint: &str,
    session_id: &str,
) -> Result<Vec<String>, String> {
    let exec_endpoint = format!(
        "{}/session/{}/execute/sync",
        endpoint.trim_end_matches('/'),
        session_id
    );
    let script = r#"
        return Array.from(document.querySelectorAll('a[href],link[rel="alternate"][href],link[hreflang][href],link[rel="canonical"][href]'))
            .map(el => el.href)
            .filter(Boolean);
    "#;
    let res = client
        .post(exec_endpoint)
        .json(&json!({ "script": script, "args": [] }))
        .send()
        .await
        .map_err(|e| format!("execute script request failed: {e}"))?;
    let status = res.status();
    let body = res
        .text()
        .await
        .map_err(|e| format!("execute script response read failed: {e}"))?;
    if !status.is_success() {
        return Err(format!(
            "execute script HTTP {}: {}",
            status.as_u16(),
            truncate_for_log(&body, 240)
        ));
    }
    let value: Value =
        serde_json::from_str(&body).map_err(|e| format!("execute parse failed: {e}"))?;
    if let Some(err) = value.pointer("/value/error").and_then(|v| v.as_str()) {
        let message = value
            .pointer("/value/message")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown execute error");
        return Err(format!("{err}: {message}"));
    }

    Ok(value
        .pointer("/value")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default())
}

async fn webdriver_rendered_snapshot(
    client: &reqwest::Client,
    endpoint: &str,
    session_id: &str,
) -> Result<(String, String), String> {
    let exec_endpoint = format!(
        "{}/session/{}/execute/sync",
        endpoint.trim_end_matches('/'),
        session_id
    );
    let script = r#"
        return {
            url: window.location.href || "",
            html: document.documentElement ? document.documentElement.outerHTML : ""
        };
    "#;
    let res = client
        .post(exec_endpoint)
        .json(&json!({ "script": script, "args": [] }))
        .send()
        .await
        .map_err(|e| format!("execute snapshot request failed: {e}"))?;
    let status = res.status();
    let body = res
        .text()
        .await
        .map_err(|e| format!("execute snapshot response read failed: {e}"))?;
    if !status.is_success() {
        return Err(format!(
            "execute snapshot HTTP {}: {}",
            status.as_u16(),
            truncate_for_log(&body, 240)
        ));
    }
    let value: Value =
        serde_json::from_str(&body).map_err(|e| format!("execute snapshot parse failed: {e}"))?;
    if let Some(err) = value.pointer("/value/error").and_then(|v| v.as_str()) {
        let message = value
            .pointer("/value/message")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown execute snapshot error");
        return Err(format!("{err}: {message}"));
    }

    let url = value
        .pointer("/value/url")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let html = value
        .pointer("/value/html")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    if url.is_empty() || html.is_empty() {
        return Err("empty rendered snapshot".to_string());
    }

    Ok((url, html))
}

async fn webdriver_delete_session(
    client: &reqwest::Client,
    endpoint: &str,
    session_id: &str,
) -> Result<(), String> {
    let delete_endpoint = format!("{}/session/{}", endpoint.trim_end_matches('/'), session_id);
    let _ = client
        .delete(delete_endpoint)
        .send()
        .await
        .map_err(|e| format!("delete session failed: {e}"))?;
    Ok(())
}

async fn discover_sitemap_seed_urls(
    client: &reqwest::Client,
    start_url: &str,
    root_host: Option<&str>,
) -> Vec<String> {
    let mut sitemap_urls = Vec::<String>::new();
    let origin = Url::parse(start_url).ok().and_then(|u| {
        let scheme = u.scheme().to_string();
        let host = u.host_str()?.to_string();
        let port = u.port().map(|p| format!(":{p}")).unwrap_or_default();
        Some(format!("{scheme}://{host}{port}"))
    });
    let Some(origin) = origin else {
        return Vec::new();
    };

    let default_sitemap = format!("{origin}/sitemap.xml");
    let robots_url = format!("{origin}/robots.txt");
    let mut sitemap_sources = vec![default_sitemap];
    if let Ok(res) = client.get(&robots_url).send().await {
        if let Ok(text) = res.text().await {
            for line in text.lines() {
                let trimmed = line.trim();
                if trimmed.to_ascii_lowercase().starts_with("sitemap:") {
                    let url = trimmed
                        .split_once(':')
                        .map(|(_, rhs)| rhs.trim().to_string())
                        .unwrap_or_default();
                    if !url.is_empty() {
                        sitemap_sources.push(url);
                    }
                }
            }
        }
    }

    let mut discovered = HashSet::<String>::new();
    for sitemap in sitemap_sources.into_iter().take(8) {
        if let Ok(res) = client.get(&sitemap).send().await {
            if let Ok(text) = res.text().await {
                for loc in extract_xml_loc_values(&text).into_iter().take(5000) {
                    if (loc.starts_with("http://") || loc.starts_with("https://"))
                        && is_same_host(&loc, root_host)
                        && discovered.insert(loc.clone())
                    {
                        sitemap_urls.push(loc);
                    }
                }
            }
        }
    }

    sitemap_urls
}

fn extract_xml_loc_values(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = 0usize;
    while let Some(open_idx) = xml[start..].find("<loc>") {
        let open = start + open_idx + 5;
        let Some(close_rel) = xml[open..].find("</loc>") else {
            break;
        };
        let close = open + close_rel;
        let value = xml[open..close].trim();
        if !value.is_empty() {
            out.push(value.to_string());
        }
        start = close + 6;
    }
    out
}

async fn ensure_webdriver_ready(
    cli: &Cli,
    browser: BrowserArg,
    requested_endpoint: &str,
    tx: &UnboundedSender<CrawlEvent>,
) -> Result<(String, Option<Child>, BrowserArg), String> {
    let requested_endpoint = normalize_local_webdriver_endpoint(requested_endpoint);

    if !cli.no_webdriver_autostart {
        let mut endpoints = Vec::new();
        if let Ok(port) = find_free_local_port() {
            endpoints.push(format!("http://127.0.0.1:{port}"));
        }
        if !endpoints.contains(&requested_endpoint) {
            endpoints.push(requested_endpoint.clone());
        }

        let mut last_err = String::new();
        for endpoint in endpoints {
            match start_webdriver(cli, browser, &endpoint, tx).await {
                Ok(child) => {
                    let _ = tx.send(CrawlEvent::Error(format!(
                        "WebDriver autostarted at {}",
                        endpoint
                    )));
                    return Ok((endpoint, Some(child), browser));
                }
                Err(err) => {
                    last_err = format!("autostart failed at {}: {}", endpoint, err);
                    let _ = tx.send(CrawlEvent::Error(last_err.clone()));
                }
            }
        }
        if !last_err.is_empty() {
            return Err(last_err);
        }
    }

    if webdriver_reachable(&requested_endpoint) {
        let _ = tx.send(CrawlEvent::Error(format!(
            "WebDriver endpoint reachable at {}",
            requested_endpoint
        )));
        return Ok((requested_endpoint, None, browser));
    }

    Err(format!(
        "endpoint {} unreachable and --no-webdriver-autostart is set",
        requested_endpoint
    ))
}

fn normalize_local_webdriver_endpoint(endpoint: &str) -> String {
    let Ok(url) = Url::parse(endpoint) else {
        return endpoint.to_string();
    };
    let Some(host) = url.host_str() else {
        return endpoint.to_string();
    };
    if host != "localhost" && host != "127.0.0.1" {
        return endpoint.to_string();
    }
    let scheme = url.scheme();
    let port = url.port_or_known_default().unwrap_or(4444);
    format!("{scheme}://127.0.0.1:{port}")
}

fn webdriver_binary_available(bin: &str) -> bool {
    let p = Path::new(bin);
    if p.components().count() > 1 || p.is_absolute() {
        return p.exists();
    }
    Command::new(bin)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn firefox_binary_available() -> bool {
    detect_browser_binary(BrowserArg::Firefox).is_some()
}

fn webdriver_binary_available_path(path: &Path) -> bool {
    Command::new(path)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn chrome_binary_available_path(path: &Path) -> bool {
    Command::new(path)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[derive(Debug, Deserialize)]
struct CftLastKnownGood {
    channels: CftChannels,
}

#[derive(Debug, Deserialize)]
struct CftChannels {
    #[serde(rename = "Stable")]
    stable: Option<CftChannel>,
    #[serde(rename = "Beta")]
    beta: Option<CftChannel>,
    #[serde(rename = "Dev")]
    dev: Option<CftChannel>,
    #[serde(rename = "Canary")]
    canary: Option<CftChannel>,
}

#[derive(Debug, Deserialize)]
struct CftChannel {
    downloads: CftDownloads,
}

#[derive(Debug, Deserialize)]
struct CftDownloads {
    chromedriver: Option<Vec<CftAsset>>,
    chrome: Option<Vec<CftAsset>>,
}

#[derive(Debug, Deserialize)]
struct CftAsset {
    platform: String,
    url: String,
}

struct WebDriverBundle {
    driver_binary: PathBuf,
    browser_binary: Option<PathBuf>,
}

struct WebDriverLaunchCandidate {
    driver_binary: String,
    browser_binary: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

async fn ensure_chromedriver_bundle(
    tx: &UnboundedSender<CrawlEvent>,
) -> Result<WebDriverBundle, String> {
    let platform = chromedriver_platform()
        .ok_or_else(|| "unsupported OS/arch for bundled chromedriver".to_string())?;
    let cache_dir = webdriver_cache_dir()?;
    fs::create_dir_all(&cache_dir).map_err(|e| format!("cache dir create failed: {e}"))?;
    let driver_binary = if cfg!(windows) {
        cache_dir.join("chromedriver.exe")
    } else {
        cache_dir.join("chromedriver")
    };
    let chrome_dist_dir = cache_dir.join("chrome-dist");
    let chrome_binary = chrome_binary_path(&chrome_dist_dir, platform);
    if driver_binary.exists()
        && chrome_binary.exists()
        && webdriver_binary_available_path(&driver_binary)
        && chrome_binary_available_path(&chrome_binary)
    {
        clear_quarantine_if_macos(&driver_binary);
        clear_quarantine_if_macos(&chrome_dist_dir);
        return Ok(WebDriverBundle {
            driver_binary,
            browser_binary: Some(chrome_binary),
        });
    }

    let _ = tx.send(CrawlEvent::Error(
        "downloading bundled webdriver (chromedriver + chrome)".to_string(),
    ));

    let manifest_url = "https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions-with-downloads.json";
    let manifest_text = reqwest::get(manifest_url)
        .await
        .map_err(|e| format!("manifest request failed: {e}"))?
        .text()
        .await
        .map_err(|e| format!("manifest read failed: {e}"))?;
    let manifest: CftLastKnownGood =
        serde_json::from_str(&manifest_text).map_err(|e| format!("manifest parse failed: {e}"))?;

    let channels = [
        manifest.channels.stable,
        manifest.channels.beta,
        manifest.channels.dev,
        manifest.channels.canary,
    ];

    let mut driver_asset_url: Option<String> = None;
    let mut chrome_asset_url: Option<String> = None;
    for channel in channels.into_iter().flatten() {
        if let Some(drivers) = &channel.downloads.chromedriver {
            if let Some(asset) = drivers.iter().find(|a| a.platform == platform) {
                driver_asset_url = Some(asset.url.clone());
            }
        }
        if let Some(chromes) = &channel.downloads.chrome {
            if let Some(asset) = chromes.iter().find(|a| a.platform == platform) {
                chrome_asset_url = Some(asset.url.clone());
            }
        }
        if driver_asset_url.is_some() && chrome_asset_url.is_some() {
            break;
        }
    }
    let driver_asset_url =
        driver_asset_url.ok_or_else(|| format!("no chromedriver asset for {platform}"))?;
    let chrome_asset_url =
        chrome_asset_url.ok_or_else(|| format!("no chrome asset for {platform}"))?;

    let driver_zip_bytes = reqwest::get(&driver_asset_url)
        .await
        .map_err(|e| format!("chromedriver download failed: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("chromedriver download read failed: {e}"))?;
    extract_binary_from_zip(&driver_zip_bytes, &driver_binary, chromedriver_leaf_name())?;

    let chrome_zip_bytes = reqwest::get(&chrome_asset_url)
        .await
        .map_err(|e| format!("chrome download failed: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("chrome download read failed: {e}"))?;
    if chrome_dist_dir.exists() {
        let _ = fs::remove_dir_all(&chrome_dist_dir);
    }
    fs::create_dir_all(&chrome_dist_dir)
        .map_err(|e| format!("chrome dist dir create failed: {e}"))?;
    extract_zip_to_dir(&chrome_zip_bytes, &chrome_dist_dir)?;

    set_executable_if_needed(&driver_binary)?;
    set_executable_if_needed(&chrome_binary)?;
    clear_quarantine_if_macos(&driver_binary);
    clear_quarantine_if_macos(&chrome_dist_dir);

    Ok(WebDriverBundle {
        driver_binary,
        browser_binary: Some(chrome_binary),
    })
}

async fn ensure_geckodriver_bundle(
    tx: &UnboundedSender<CrawlEvent>,
) -> Result<WebDriverBundle, String> {
    let platform = geckodriver_platform()
        .ok_or_else(|| "unsupported OS/arch for bundled geckodriver".to_string())?;
    let cache_dir = webdriver_cache_dir()?;
    fs::create_dir_all(&cache_dir).map_err(|e| format!("cache dir create failed: {e}"))?;
    let driver_binary = if cfg!(windows) {
        cache_dir.join("geckodriver.exe")
    } else {
        cache_dir.join("geckodriver")
    };
    if driver_binary.exists() && webdriver_binary_available_path(&driver_binary) {
        clear_quarantine_if_macos(&driver_binary);
        return Ok(WebDriverBundle {
            driver_binary,
            browser_binary: None,
        });
    }

    let _ = tx.send(CrawlEvent::Error(
        "downloading bundled webdriver (geckodriver)".to_string(),
    ));

    let release_url = "https://api.github.com/repos/mozilla/geckodriver/releases/latest";
    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| format!("http client build failed: {e}"))?;
    let release_text = client
        .get(release_url)
        .header(reqwest::header::USER_AGENT, "spider-tui")
        .send()
        .await
        .map_err(|e| format!("geckodriver release request failed: {e}"))?
        .text()
        .await
        .map_err(|e| format!("geckodriver release read failed: {e}"))?;
    let release: GithubRelease = serde_json::from_str(&release_text)
        .map_err(|e| format!("geckodriver release parse failed: {e}"))?;

    let asset = release
        .assets
        .iter()
        .find(|a| a.name.contains(platform))
        .ok_or_else(|| {
            format!(
                "no geckodriver asset for {platform} in {}",
                release.tag_name
            )
        })?;

    let archive_bytes = client
        .get(&asset.browser_download_url)
        .header(reqwest::header::USER_AGENT, "spider-tui")
        .send()
        .await
        .map_err(|e| format!("geckodriver download failed: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("geckodriver download read failed: {e}"))?;

    if asset.name.ends_with(".zip") {
        extract_binary_from_zip(&archive_bytes, &driver_binary, geckodriver_leaf_name())?;
    } else if asset.name.ends_with(".tar.gz") {
        extract_binary_from_targz(&archive_bytes, &driver_binary, geckodriver_leaf_name())?;
    } else {
        return Err(format!(
            "unsupported geckodriver archive format: {}",
            asset.name
        ));
    }

    set_executable_if_needed(&driver_binary)?;
    clear_quarantine_if_macos(&driver_binary);
    Ok(WebDriverBundle {
        driver_binary,
        browser_binary: None,
    })
}

async fn ensure_firefox_bundle(tx: &UnboundedSender<CrawlEvent>) -> Result<PathBuf, String> {
    let cache_dir = webdriver_cache_dir()?;
    let dist_dir = cache_dir.join("firefox-dist");
    let binary = firefox_bundle_binary_path(&dist_dir);
    if binary.exists() {
        clear_quarantine_if_macos(&dist_dir);
        return Ok(binary);
    }

    #[cfg(target_os = "macos")]
    {
        let _ = tx.send(CrawlEvent::Error(
            "downloading bundled browser (Firefox)".to_string(),
        ));
        fs::create_dir_all(&cache_dir).map_err(|e| format!("cache dir create failed: {e}"))?;
        let dmg_url = "https://download.mozilla.org/?product=firefox-latest-ssl&os=osx&lang=en-US";
        let client = reqwest::Client::builder()
            .build()
            .map_err(|e| format!("http client build failed: {e}"))?;
        let dmg_bytes = client
            .get(dmg_url)
            .header(reqwest::header::USER_AGENT, "spider-tui")
            .send()
            .await
            .map_err(|e| format!("firefox dmg download failed: {e}"))?
            .bytes()
            .await
            .map_err(|e| format!("firefox dmg read failed: {e}"))?;

        let dmg_path = cache_dir.join("firefox-latest.dmg");
        fs::write(&dmg_path, &dmg_bytes).map_err(|e| format!("failed to write dmg: {e}"))?;
        let mount_dir = cache_dir.join("firefox-mount");
        if mount_dir.exists() {
            let _ = Command::new("hdiutil")
                .arg("detach")
                .arg(&mount_dir)
                .arg("-quiet")
                .status();
            let _ = fs::remove_dir_all(&mount_dir);
        }
        fs::create_dir_all(&mount_dir).map_err(|e| format!("mount dir create failed: {e}"))?;

        let attach = Command::new("hdiutil")
            .arg("attach")
            .arg(&dmg_path)
            .arg("-nobrowse")
            .arg("-mountpoint")
            .arg(&mount_dir)
            .arg("-quiet")
            .status()
            .map_err(|e| format!("hdiutil attach failed: {e}"))?;
        if !attach.success() {
            return Err("hdiutil attach returned non-zero status".to_string());
        }

        if dist_dir.exists() {
            let _ = fs::remove_dir_all(&dist_dir);
        }
        fs::create_dir_all(&dist_dir).map_err(|e| format!("dist dir create failed: {e}"))?;
        let source_app = mount_dir.join("Firefox.app");
        let target_app = dist_dir.join("Firefox.app");
        let copy_status = Command::new("cp")
            .arg("-R")
            .arg(&source_app)
            .arg(&target_app)
            .status()
            .map_err(|e| format!("copy Firefox.app failed: {e}"))?;
        let _ = Command::new("hdiutil")
            .arg("detach")
            .arg(&mount_dir)
            .arg("-quiet")
            .status();
        let _ = fs::remove_file(&dmg_path);
        let _ = fs::remove_dir_all(&mount_dir);

        if !copy_status.success() {
            return Err("copy Firefox.app returned non-zero status".to_string());
        }

        clear_quarantine_if_macos(&dist_dir);
        let binary = firefox_bundle_binary_path(&dist_dir);
        if binary.exists() {
            return Ok(binary);
        }
        return Err("bundled Firefox binary missing after install".to_string());
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = tx.send(CrawlEvent::Error(
            "automatic Firefox browser bundle is currently only implemented on macOS".to_string(),
        ));
        Err("Firefox browser not found".to_string())
    }
}

fn webdriver_cache_dir() -> Result<PathBuf, String> {
    if let Ok(home) = std::env::var("HOME") {
        return Ok(Path::new(&home).join(".cache/spider-tui/webdriver"));
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        return Ok(Path::new(&profile).join(".cache/spider-tui/webdriver"));
    }
    Err("cannot determine home directory for webdriver cache".to_string())
}

fn chromedriver_platform() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("mac-arm64"),
        ("macos", "x86_64") => Some("mac-x64"),
        ("linux", "x86_64") => Some("linux64"),
        ("windows", "x86_64") => Some("win64"),
        _ => None,
    }
}

fn geckodriver_platform() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("macos-aarch64"),
        ("macos", "x86_64") => Some("macos"),
        ("linux", "x86_64") => Some("linux64"),
        ("windows", "x86_64") => Some("win64"),
        _ => None,
    }
}

fn chromedriver_leaf_name() -> &'static str {
    if cfg!(windows) {
        "chromedriver.exe"
    } else {
        "chromedriver"
    }
}

fn geckodriver_leaf_name() -> &'static str {
    if cfg!(windows) {
        "geckodriver.exe"
    } else {
        "geckodriver"
    }
}

fn chrome_leaf_name() -> &'static str {
    if cfg!(windows) {
        "chrome.exe"
    } else {
        "chrome"
    }
}

fn chrome_binary_path(chrome_dist_dir: &Path, platform: &str) -> PathBuf {
    match platform {
        "mac-arm64" => chrome_dist_dir
            .join("chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"),
        "mac-x64" => chrome_dist_dir
            .join("chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"),
        "linux64" => chrome_dist_dir.join("chrome-linux64/chrome"),
        "win64" => chrome_dist_dir.join("chrome-win64/chrome.exe"),
        _ => chrome_dist_dir.join(chrome_leaf_name()),
    }
}

fn firefox_bundle_binary_path(dist_dir: &Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        return dist_dir.join("Firefox.app/Contents/MacOS/firefox");
    }
    #[cfg(target_os = "linux")]
    {
        return dist_dir.join("firefox/firefox");
    }
    #[cfg(target_os = "windows")]
    {
        return dist_dir.join("Firefox/firefox.exe");
    }
    #[allow(unreachable_code)]
    dist_dir.join("firefox")
}

fn extract_binary_from_zip(zip_bytes: &[u8], target: &Path, leaf_name: &str) -> Result<(), String> {
    let reader = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(reader).map_err(|e| format!("zip open failed: {e}"))?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("zip entry failed: {e}"))?;
        let name = file.name().to_string();
        let file_name_matches = Path::new(&name)
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n == leaf_name)
            .unwrap_or(false);
        if file_name_matches {
            let mut out = File::create(target).map_err(|e| format!("file create failed: {e}"))?;
            std::io::copy(&mut file, &mut out).map_err(|e| format!("file write failed: {e}"))?;
            return Ok(());
        }
    }

    Err(format!("{leaf_name} not found in archive"))
}

fn extract_binary_from_targz(
    archive_bytes: &[u8],
    target: &Path,
    leaf_name: &str,
) -> Result<(), String> {
    let cursor = std::io::Cursor::new(archive_bytes);
    let gz = flate2::read::GzDecoder::new(cursor);
    let mut archive = tar::Archive::new(gz);
    let entries = archive
        .entries()
        .map_err(|e| format!("tar entries failed: {e}"))?;

    for entry in entries {
        let mut file = entry.map_err(|e| format!("tar entry failed: {e}"))?;
        let path = file
            .path()
            .map_err(|e| format!("tar path failed: {e}"))?
            .to_path_buf();
        let file_name_matches = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n == leaf_name)
            .unwrap_or(false);
        if file_name_matches {
            let mut out = File::create(target).map_err(|e| format!("file create failed: {e}"))?;
            std::io::copy(&mut file, &mut out).map_err(|e| format!("file write failed: {e}"))?;
            return Ok(());
        }
    }

    Err(format!("{leaf_name} not found in archive"))
}

fn extract_zip_to_dir(zip_bytes: &[u8], destination: &Path) -> Result<(), String> {
    let reader = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(reader).map_err(|e| format!("zip open failed: {e}"))?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("zip entry failed: {e}"))?;
        let Some(safe_path) = file.enclosed_name() else {
            continue;
        };
        let out_path = destination.join(safe_path);
        if file.is_dir() {
            fs::create_dir_all(&out_path).map_err(|e| format!("dir create failed: {e}"))?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("dir create failed: {e}"))?;
        }
        let mut out = File::create(&out_path).map_err(|e| format!("file create failed: {e}"))?;
        std::io::copy(&mut file, &mut out).map_err(|e| format!("file write failed: {e}"))?;
    }
    Ok(())
}

fn set_executable_if_needed(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)
            .map_err(|e| format!("metadata failed for {}: {e}", path.display()))?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms)
            .map_err(|e| format!("chmod failed for {}: {e}", path.display()))?;
    }
    Ok(())
}

fn clear_quarantine_if_macos(path: &Path) {
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("xattr")
            .arg("-dr")
            .arg("com.apple.quarantine")
            .arg(path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

fn stop_webdriver(mut child: Option<Child>) {
    if let Some(ref mut c) = child {
        let _ = c.kill();
        let _ = c.wait();
    }
}

fn webdriver_log_path(port: u16) -> Result<PathBuf, String> {
    let cache_dir = webdriver_cache_dir()?;
    fs::create_dir_all(&cache_dir).map_err(|e| format!("cache dir create failed: {e}"))?;
    Ok(cache_dir.join(format!("webdriver-{port}.log")))
}

fn read_log_tail(path: &Path, lines: usize) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let tail = text
        .lines()
        .rev()
        .take(lines.max(1))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join(" | ");
    if tail.is_empty() { None } else { Some(tail) }
}

fn find_free_local_port() -> Result<u16, String> {
    let listener =
        TcpListener::bind("127.0.0.1:0").map_err(|e| format!("free port bind failed: {e}"))?;
    listener
        .local_addr()
        .map(|addr| addr.port())
        .map_err(|e| format!("local addr failed: {e}"))
}

fn is_same_host(candidate: &str, root_host: Option<&str>) -> bool {
    let Some(root) = root_host else {
        return true;
    };

    Url::parse(candidate)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.eq_ignore_ascii_case(root)))
        .unwrap_or(false)
}

fn normalize_crawl_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut url = Url::parse(trimmed).ok()?;
    let scheme = url.scheme().to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }

    let kept_params = url
        .query_pairs()
        .filter_map(|(k, v)| {
            if is_tracking_query_param(&k) {
                None
            } else {
                Some((k.into_owned(), v.into_owned()))
            }
        })
        .collect::<Vec<_>>();
    if kept_params.is_empty() {
        url.set_query(None);
    } else {
        let mut serializer = url::form_urlencoded::Serializer::new(String::new());
        for (k, v) in kept_params {
            serializer.append_pair(&k, &v);
        }
        url.set_query(Some(&serializer.finish()));
    }

    url.set_fragment(None);
    Some(url.to_string())
}

fn is_tracking_query_param(param: &str) -> bool {
    let name = param.to_ascii_lowercase();
    if name.starts_with("utm_") || name.starts_with("gad_") {
        return true;
    }
    matches!(
        name.as_str(),
        "gclid"
            | "fbclid"
            | "gbraid"
            | "wbraid"
            | "_gl"
            | "mc_cid"
            | "mc_eid"
            | "pk_campaign"
            | "pk_kwd"
            | "pk_source"
            | "pk_medium"
            | "pk_content"
    )
}

fn filter_crawlable_links(links: Vec<String>, root_host: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    for link in links {
        let Some(normalized) = normalize_crawl_url(&link) else {
            continue;
        };
        if !is_same_host(&normalized, root_host) {
            continue;
        }
        if seen.insert(normalized.clone()) {
            out.push(normalized);
        }
    }

    out
}

async fn fetch_missing_urls(
    urls: Vec<String>,
    retries: usize,
    concurrency: usize,
    root_host: Option<&str>,
    tx: &UnboundedSender<CrawlEvent>,
) {
    let concurrency = concurrency.max(1);
    let root_host_owned = root_host.map(|s| s.to_string());
    let client = match ClientBuilder::new()
        .timeout(Duration::from_secs(30))
        .build()
    {
        Ok(c) => c,
        Err(err) => {
            let _ = tx.send(CrawlEvent::Error(format!(
                "failed to create fallback client: {err}"
            )));
            for url in urls {
                let _ = tx.send(CrawlEvent::Unretrieved {
                    url,
                    reason: "fallback fetch client setup failed".to_string(),
                });
            }
            return;
        }
    };
    let redirect_client = match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(25))
        .pool_max_idle_per_host(32)
        .build()
    {
        Ok(c) => c,
        Err(err) => {
            let _ = tx.send(CrawlEvent::Error(format!(
                "failed to create redirect client: {err}"
            )));
            for url in urls {
                let _ = tx.send(CrawlEvent::Unretrieved {
                    url,
                    reason: "redirect probe client setup failed".to_string(),
                });
            }
            return;
        }
    };

    if concurrency == 1 || urls.len() <= 1 {
        for url in urls {
            let events = process_single_url(
                url,
                retries,
                root_host_owned.clone(),
                client.clone(),
                redirect_client.clone(),
            )
            .await;
            for event in events {
                let _ = tx.send(event);
            }
        }
        return;
    }

    let mut iter = urls.into_iter();
    let mut set = JoinSet::new();

    for _ in 0..concurrency {
        if let Some(url) = iter.next() {
            let root = root_host_owned.clone();
            let http_client = client.clone();
            let redir_client = redirect_client.clone();
            set.spawn(async move {
                process_single_url(url, retries, root, http_client, redir_client).await
            });
        }
    }

    while let Some(joined) = set.join_next().await {
        match joined {
            Ok(events) => {
                for event in events {
                    let _ = tx.send(event);
                }
            }
            Err(err) => {
                let _ = tx.send(CrawlEvent::Error(format!("fetch worker failed: {err}")));
            }
        }

        if let Some(url) = iter.next() {
            let root = root_host_owned.clone();
            let http_client = client.clone();
            let redir_client = redirect_client.clone();
            set.spawn(async move {
                process_single_url(url, retries, root, http_client, redir_client).await
            });
        }
    }
}

async fn process_single_url(
    url: String,
    retries: usize,
    root_host: Option<String>,
    client: spider::Client,
    redirect_client: reqwest::Client,
) -> Vec<CrawlEvent> {
    let mut out = Vec::new();
    let root_host_ref = root_host.as_deref();

    let (redirect_rows, fetch_url) = match raw_redirect_rows(&redirect_client, &url, 8).await {
        Ok(v) => v,
        Err(_) => (Vec::new(), url.clone()),
    };

    for (row, discovered_links) in redirect_rows {
        out.push(CrawlEvent::Page {
            row,
            discovered_links,
        });
    }

    let mut last_page: Option<Page> = None;
    for _ in 0..retries {
        let page = Page::new(&fetch_url, &client).await;
        let status = page.status_code.as_u16();
        let has_body = !page.get_html_bytes_u8().is_empty();
        last_page = Some(page);

        // Treat non-5xx responses as resolved to avoid endlessly dropping 2xx/3xx/4xx assets.
        if has_body || status < 500 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }

    if let Some(page) = last_page {
        let (mut row, discovered_links) = page_to_row(&page);
        row.url = fetch_url.clone();
        let filtered_links = filter_crawlable_links(discovered_links, root_host_ref);
        row.link_count = filtered_links.len();
        if row.size == 0 && row.status >= 500 {
            out.push(CrawlEvent::Unretrieved {
                url: row.url.clone(),
                reason: format!("http {} after retries", row.status),
            });
        } else {
            out.push(CrawlEvent::Page {
                row,
                discovered_links: filtered_links,
            });
        }
    } else {
        out.push(CrawlEvent::Unretrieved {
            url: url.clone(),
            reason: "fallback fetch could not start".to_string(),
        });
    }

    out
}

async fn raw_redirect_rows(
    client: &reqwest::Client,
    start_url: &str,
    max_hops: usize,
) -> Result<(Vec<(CrawlRow, Vec<String>)>, String), String> {
    let mut rows = Vec::<(CrawlRow, Vec<String>)>::new();
    let mut current = normalize_crawl_url(start_url).unwrap_or_else(|| start_url.to_string());
    let mut seen = HashSet::<String>::new();

    for _ in 0..max_hops.max(1) {
        if !seen.insert(current.clone()) {
            break;
        }

        let started = Instant::now();
        let response = match send_redirect_probe_request(client, &current, 3).await {
            Ok(response) => response,
            Err(_) => break,
        };
        let elapsed = started.elapsed().as_millis();
        let status = response.status().as_u16();
        let headers = response.headers().clone();

        if !(300..=399).contains(&status) {
            break;
        }

        let location_raw = headers
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.trim().to_string())
            .unwrap_or_default();
        if location_raw.is_empty() {
            break;
        }

        let resolved_target = Url::parse(&current)
            .ok()
            .and_then(|base| base.join(&location_raw).ok())
            .map(|u| u.to_string())
            .unwrap_or(location_raw.clone());
        let resolved_target = normalize_crawl_url(&resolved_target).unwrap_or(resolved_target);
        let mime = headers
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.split(';').next().unwrap_or("").trim().to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "unknown".to_string());
        let last_modified = headers
            .get(reqwest::header::LAST_MODIFIED)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.to_string())
            .unwrap_or_default();

        let row = CrawlRow {
            url: current.clone(),
            status,
            mime,
            retrieval_status: "retrieved".to_string(),
            indexability: "Non-Indexable".to_string(),
            title: String::new(),
            title_length: 0,
            meta: String::new(),
            meta_length: 0,
            h1: String::new(),
            canonical: String::new(),
            word_count: 0,
            size: 0,
            response_time: elapsed,
            last_modified,
            redirect_url: resolved_target.clone(),
            redirect_type: redirect_class(status).to_string(),
            link_count: 1,
            crawl_timestamp: Utc::now().to_rfc3339(),
        };
        rows.push((row, vec![resolved_target.clone()]));
        current = resolved_target;
    }

    Ok((rows, current))
}

async fn send_redirect_probe_request(
    client: &reqwest::Client,
    url: &str,
    attempts: usize,
) -> Result<reqwest::Response, String> {
    let mut last_error = String::new();
    let max_attempts = attempts.max(1);

    for attempt in 1..=max_attempts {
        match client.get(url).send().await {
            Ok(response) => return Ok(response),
            Err(err) => {
                last_error = err.to_string();
                let retryable = err.is_timeout() || err.is_connect() || err.is_request();
                if !retryable || attempt == max_attempts {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(120 * attempt as u64)).await;
            }
        }
    }

    Err(last_error)
}

fn page_to_row(page: &spider::page::Page) -> (CrawlRow, Vec<String>) {
    let html = page.get_html();
    let doc = Html::parse_document(&html);

    let mut title = extract_title(&doc);
    let meta = extract_meta_description(&doc);
    let h1 = extract_real_h1(&doc);
    if title.is_empty() {
        title = h1.clone();
    }
    let canonical = extract_canonical(&doc);

    let mime = header_value(page, "content-type")
        .map(|v| v.split(';').next().unwrap_or("").trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| infer_mime_from_page(page));

    let status = page.status_code.as_u16();
    let noindex = has_noindex(&doc, page);
    let indexability = if (200..=299).contains(&status) && !noindex {
        "Indexable".to_string()
    } else {
        "Non-Indexable".to_string()
    };

    let requested_url_raw = page.get_url().to_string();
    let final_url_raw = page.get_url_final().to_string();
    let requested_url =
        normalize_crawl_url(&requested_url_raw).unwrap_or_else(|| requested_url_raw.clone());
    let final_url = normalize_crawl_url(&final_url_raw).unwrap_or_else(|| final_url_raw.clone());
    let is_followed_redirect = requested_url != final_url;
    let row_url = if (300..=399).contains(&status) {
        requested_url.clone()
    } else if is_followed_redirect {
        final_url.clone()
    } else {
        requested_url.clone()
    };
    let redirect_url = if (300..=399).contains(&status) {
        if is_followed_redirect {
            final_url.clone()
        } else {
            String::new()
        }
    } else {
        String::new()
    };
    let redirect_type = if (300..=399).contains(&status) {
        redirect_class(status).to_string()
    } else {
        String::new()
    };

    let word_count = count_words(&doc);
    let size = page.get_html_bytes_u8().len();
    let response_time = page.get_duration_elapsed().as_millis();
    let last_modified = header_value(page, "last-modified").unwrap_or_default();

    let mut discovered_links = page
        .page_links
        .as_ref()
        .map(|links| links.iter().map(ToString::to_string).collect::<Vec<_>>())
        .unwrap_or_default();
    discovered_links.extend(extract_alternate_links(&doc));

    (
        CrawlRow {
            url: row_url,
            status,
            mime,
            retrieval_status: "retrieved".to_string(),
            indexability,
            title_length: title.chars().count(),
            title,
            meta_length: meta.chars().count(),
            meta,
            h1,
            canonical,
            word_count,
            size,
            response_time,
            last_modified,
            redirect_url,
            redirect_type,
            link_count: discovered_links.len(),
            crawl_timestamp: Utc::now().to_rfc3339(),
        },
        discovered_links,
    )
}

fn apply_rendered_html_to_row(row: &mut CrawlRow, html: &str) {
    let doc = Html::parse_document(html);
    let title = extract_title(&doc);
    let meta = extract_meta_description(&doc);
    let h1 = extract_real_h1(&doc);
    let canonical = extract_canonical(&doc);
    let noindex = has_noindex_meta(&doc);

    if !title.is_empty() {
        row.title = title;
        row.title_length = row.title.chars().count();
    }
    row.meta = meta;
    row.meta_length = row.meta.chars().count();
    row.h1 = h1;
    row.canonical = canonical;
    row.word_count = count_words(&doc);
    row.size = html.as_bytes().len();
    row.mime = "text/html".to_string();
    row.indexability = if (200..=299).contains(&row.status) && !noindex {
        "Indexable".to_string()
    } else {
        "Non-Indexable".to_string()
    };
}

fn unretrieved_row(url: String, reason: String) -> CrawlRow {
    let reason_len = reason.chars().count();
    CrawlRow {
        url,
        status: 0,
        mime: "unknown".to_string(),
        retrieval_status: "not_retrieved".to_string(),
        indexability: "Not Retrieved".to_string(),
        title: String::new(),
        title_length: 0,
        meta: reason,
        meta_length: reason_len,
        h1: String::new(),
        canonical: String::new(),
        word_count: 0,
        size: 0,
        response_time: 0,
        last_modified: String::new(),
        redirect_url: String::new(),
        redirect_type: String::new(),
        link_count: 0,
        crawl_timestamp: Utc::now().to_rfc3339(),
    }
}

fn header_value(page: &spider::page::Page, name: &'static str) -> Option<String> {
    page.headers.as_ref().and_then(|headers| {
        headers.iter().find_map(|(header_name, header_value)| {
            if header_name.as_str().eq_ignore_ascii_case(name) {
                header_value.to_str().ok().map(|v| v.to_string())
            } else {
                None
            }
        })
    })
}

fn infer_mime_from_page(page: &spider::page::Page) -> String {
    let url = page.get_url().to_ascii_lowercase();
    if url.ends_with(".xml") {
        return "application/xml".to_string();
    }
    if url.ends_with(".json") {
        return "application/json".to_string();
    }
    if url.ends_with(".pdf") {
        return "application/pdf".to_string();
    }
    if url.ends_with(".css") {
        return "text/css".to_string();
    }
    if url.ends_with(".js") {
        return "application/javascript".to_string();
    }
    if url.ends_with(".svg") {
        return "image/svg+xml".to_string();
    }
    if url.ends_with(".png") {
        return "image/png".to_string();
    }
    if url.ends_with(".jpg") || url.ends_with(".jpeg") {
        return "image/jpeg".to_string();
    }
    if url.ends_with(".webp") {
        return "image/webp".to_string();
    }

    let html = page.get_html();
    let html_lc = html.to_ascii_lowercase();
    if html_lc.contains("<html") || html_lc.contains("<!doctype html") || html_lc.contains("<body")
    {
        "text/html".to_string()
    } else {
        "unknown".to_string()
    }
}

fn extract_first_text(doc: &Html, selector: &str) -> String {
    let selector = match Selector::parse(selector) {
        Ok(s) => s,
        Err(_) => return String::new(),
    };

    for el in doc.select(&selector) {
        let text = normalize_text(&el.text().collect::<Vec<_>>().join(" "));
        if !text.is_empty() {
            return text;
        }
    }

    String::new()
}

fn normalize_text(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn extract_meta_content(doc: &Html, selector: &str) -> String {
    let selector = match Selector::parse(selector) {
        Ok(s) => s,
        Err(_) => return String::new(),
    };

    doc.select(&selector)
        .find_map(|el| el.value().attr("content"))
        .map(normalize_text)
        .unwrap_or_default()
}

fn extract_title(doc: &Html) -> String {
    let title = extract_first_text(doc, "title");
    if !title.is_empty() {
        return title;
    }
    let og_title = extract_meta_content(doc, "meta[property=\"og:title\"]");
    if !og_title.is_empty() {
        return og_title;
    }
    extract_meta_content(doc, "meta[name=\"twitter:title\"]")
}

fn extract_real_h1(doc: &Html) -> String {
    extract_first_text(doc, "h1")
}

fn extract_meta_description(doc: &Html) -> String {
    let description = extract_meta_content(doc, "meta[name=\"description\"]");
    if !description.is_empty() {
        return description;
    }
    let og_description = extract_meta_content(doc, "meta[property=\"og:description\"]");
    if !og_description.is_empty() {
        return og_description;
    }
    extract_meta_content(doc, "meta[name=\"twitter:description\"]")
}

fn extract_canonical(doc: &Html) -> String {
    let selector = match Selector::parse("link[rel=\"canonical\"]") {
        Ok(s) => s,
        Err(_) => return String::new(),
    };

    doc.select(&selector)
        .next()
        .and_then(|el| el.value().attr("href"))
        .map(normalize_text)
        .unwrap_or_default()
}

fn extract_alternate_links(doc: &Html) -> Vec<String> {
    let selector =
        match Selector::parse("link[rel=\"alternate\"][href], link[hreflang][href], a[href]") {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };

    let mut out = Vec::new();
    for el in doc.select(&selector) {
        if let Some(href) = el.value().attr("href") {
            out.push(href.trim().to_string());
        }
    }
    out
}

fn has_noindex(doc: &Html, page: &spider::page::Page) -> bool {
    if has_noindex_meta(doc) {
        return true;
    }

    header_value(page, "x-robots-tag")
        .map(|v| v.to_ascii_lowercase().contains("noindex"))
        .unwrap_or(false)
}

fn has_noindex_meta(doc: &Html) -> bool {
    let robots_sel = match Selector::parse("meta[name=\"robots\"], meta[name=\"googlebot\"]") {
        Ok(s) => s,
        Err(_) => return false,
    };

    doc.select(&robots_sel).any(|el| {
        el.value()
            .attr("content")
            .map(|c| c.to_ascii_lowercase().contains("noindex"))
            .unwrap_or(false)
    })
}

fn count_words(doc: &Html) -> usize {
    doc.root_element()
        .text()
        .flat_map(|t| t.split_whitespace())
        .count()
}

fn redirect_class(status: u16) -> &'static str {
    match status {
        301 | 308 => "Permanent",
        302 | 303 | 307 => "Temporary",
        300..=399 => "Redirect",
        _ => "",
    }
}

#[derive(Default, Copy, Clone)]
struct StatusBuckets {
    c2: usize,
    c3: usize,
    c4: usize,
    c5: usize,
    c0: usize,
}

fn status_buckets(counts: &HashMap<u16, usize>) -> StatusBuckets {
    let mut buckets = StatusBuckets::default();
    for (code, count) in counts {
        match *code {
            0 => buckets.c0 += *count,
            200..=299 => buckets.c2 += *count,
            300..=399 => buckets.c3 += *count,
            400..=499 => buckets.c4 += *count,
            500..=599 => buckets.c5 += *count,
            _ => {}
        }
    }
    buckets
}

fn top_status_codes(counts: &HashMap<u16, usize>, limit: usize) -> Vec<(u16, usize)> {
    let mut entries = counts
        .iter()
        .map(|(code, count)| (*code, *count))
        .collect::<Vec<_>>();
    entries.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    entries.into_iter().take(limit.max(1)).collect()
}

fn status_code_style(code: u16) -> Style {
    match code {
        0 => Style::default()
            .fg(Color::LightRed)
            .add_modifier(Modifier::BOLD),
        200..=299 => Style::default().fg(Color::Green),
        300..=399 => Style::default().fg(Color::Yellow),
        400..=499 => Style::default().fg(Color::Red),
        500..=599 => Style::default().fg(Color::Magenta),
        _ => Style::default().fg(Color::Gray),
    }
}
