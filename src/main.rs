use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{self, File};
use std::io::{self, Stdout, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use chrono::Utc;
use clap::{ArgAction, Parser, ValueEnum};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers, MouseButton,
    MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Cell, Gauge, Paragraph, Row, Table, TableState, Tabs, Wrap,
};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
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
    about = "TUI crawler powered by spider with live CSV/JSON output"
)]
struct Cli {
    #[arg(value_name = "URL", required_unless_present = "review_file")]
    url: Option<String>,

    #[arg(
        long = "review",
        alias = "review-csv",
        alias = "review-json",
        value_name = "FILE"
    )]
    review_file: Option<String>,

    #[arg(short, long, value_name = "FILE")]
    output: Option<String>,

    #[arg(long, value_enum, default_value_t = FileFormatArg::Csv)]
    format: FileFormatArg,

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

#[derive(Debug, Copy, Clone, ValueEnum, PartialEq, Eq)]
enum FileFormatArg {
    Csv,
    Json,
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
    internal_link_count: usize,
    external_link_count: usize,
    h1_count: usize,
    h2_count: usize,
    image_count: usize,
    image_missing_alt_count: usize,
    structured_data_count: usize,
    seo_score: u8,
    issues: Vec<SeoIssue>,
    crawl_timestamp: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SeoIssue {
    NotRetrieved,
    Http4xx,
    Http5xx,
    Noindex,
    MissingTitle,
    TitleTooShort,
    TitleTooLong,
    MissingMetaDescription,
    MetaDescriptionTooShort,
    MetaDescriptionTooLong,
    MissingH1,
    MultipleH1,
    MissingCanonical,
    LowWordCount,
    ImagesMissingAlt,
    TooManyExternalLinks,
}

impl SeoIssue {
    fn label(self) -> &'static str {
        match self {
            SeoIssue::NotRetrieved => "not_retrieved",
            SeoIssue::Http4xx => "status_4xx",
            SeoIssue::Http5xx => "status_5xx",
            SeoIssue::Noindex => "noindex",
            SeoIssue::MissingTitle => "missing_title",
            SeoIssue::TitleTooShort => "title_too_short",
            SeoIssue::TitleTooLong => "title_too_long",
            SeoIssue::MissingMetaDescription => "missing_meta_description",
            SeoIssue::MetaDescriptionTooShort => "meta_description_too_short",
            SeoIssue::MetaDescriptionTooLong => "meta_description_too_long",
            SeoIssue::MissingH1 => "missing_h1",
            SeoIssue::MultipleH1 => "multiple_h1",
            SeoIssue::MissingCanonical => "missing_canonical",
            SeoIssue::LowWordCount => "low_word_count",
            SeoIssue::ImagesMissingAlt => "images_missing_alt",
            SeoIssue::TooManyExternalLinks => "too_many_external_links",
        }
    }

    fn penalty(self) -> u8 {
        match self {
            SeoIssue::NotRetrieved => 70,
            SeoIssue::Http5xx => 65,
            SeoIssue::Http4xx => 40,
            SeoIssue::Noindex => 20,
            SeoIssue::MissingTitle => 25,
            SeoIssue::TitleTooShort => 10,
            SeoIssue::TitleTooLong => 8,
            SeoIssue::MissingMetaDescription => 20,
            SeoIssue::MetaDescriptionTooShort => 8,
            SeoIssue::MetaDescriptionTooLong => 8,
            SeoIssue::MissingH1 => 14,
            SeoIssue::MultipleH1 => 8,
            SeoIssue::MissingCanonical => 10,
            SeoIssue::LowWordCount => 10,
            SeoIssue::ImagesMissingAlt => 8,
            SeoIssue::TooManyExternalLinks => 6,
        }
    }

    fn from_label(label: &str) -> Option<Self> {
        match label.trim() {
            "not_retrieved" => Some(SeoIssue::NotRetrieved),
            "status_4xx" => Some(SeoIssue::Http4xx),
            "status_5xx" => Some(SeoIssue::Http5xx),
            "noindex" => Some(SeoIssue::Noindex),
            "missing_title" => Some(SeoIssue::MissingTitle),
            "title_too_short" => Some(SeoIssue::TitleTooShort),
            "title_too_long" => Some(SeoIssue::TitleTooLong),
            "missing_meta_description" => Some(SeoIssue::MissingMetaDescription),
            "meta_description_too_short" => Some(SeoIssue::MetaDescriptionTooShort),
            "meta_description_too_long" => Some(SeoIssue::MetaDescriptionTooLong),
            "missing_h1" => Some(SeoIssue::MissingH1),
            "multiple_h1" => Some(SeoIssue::MultipleH1),
            "missing_canonical" => Some(SeoIssue::MissingCanonical),
            "low_word_count" => Some(SeoIssue::LowWordCount),
            "images_missing_alt" => Some(SeoIssue::ImagesMissingAlt),
            "too_many_external_links" => Some(SeoIssue::TooManyExternalLinks),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActivePanel {
    Pages,
    Issues,
    Summary,
}

impl ActivePanel {
    fn as_index(self) -> usize {
        match self {
            ActivePanel::Pages => 0,
            ActivePanel::Issues => 1,
            ActivePanel::Summary => 2,
        }
    }

    fn title(self) -> &'static str {
        match self {
            ActivePanel::Pages => "Pages",
            ActivePanel::Issues => "Issues",
            ActivePanel::Summary => "Summary",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageSortMode {
    Latest,
    LowestSeoScore,
    HighestResponseTime,
}

impl PageSortMode {
    fn cycle(self) -> Self {
        match self {
            PageSortMode::Latest => PageSortMode::LowestSeoScore,
            PageSortMode::LowestSeoScore => PageSortMode::HighestResponseTime,
            PageSortMode::HighestResponseTime => PageSortMode::Latest,
        }
    }

    fn title(self) -> &'static str {
        match self {
            PageSortMode::Latest => "latest",
            PageSortMode::LowestSeoScore => "seo_score",
            PageSortMode::HighestResponseTime => "response_time",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortDirection {
    Asc,
    Desc,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum DataFormat {
    Csv,
    Json,
}

impl From<FileFormatArg> for DataFormat {
    fn from(value: FileFormatArg) -> Self {
        match value {
            FileFormatArg::Csv => DataFormat::Csv,
            FileFormatArg::Json => DataFormat::Json,
        }
    }
}

impl SortDirection {
    fn toggle(self) -> Self {
        match self {
            SortDirection::Asc => SortDirection::Desc,
            SortDirection::Desc => SortDirection::Asc,
        }
    }

    fn label(self) -> &'static str {
        match self {
            SortDirection::Asc => "asc",
            SortDirection::Desc => "desc",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PagesPane {
    Table,
    Details,
}

impl PagesPane {
    fn cycle(self) -> Self {
        match self {
            PagesPane::Table => PagesPane::Details,
            PagesPane::Details => PagesPane::Table,
        }
    }

    fn reverse_cycle(self) -> Self {
        self.cycle()
    }

    fn label(self) -> &'static str {
        match self {
            PagesPane::Table => "table",
            PagesPane::Details => "details",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IssuesPane {
    Distribution,
    Urls,
}

impl IssuesPane {
    fn cycle(self) -> Self {
        match self {
            IssuesPane::Distribution => IssuesPane::Urls,
            IssuesPane::Urls => IssuesPane::Distribution,
        }
    }

    fn reverse_cycle(self) -> Self {
        self.cycle()
    }

    fn label(self) -> &'static str {
        match self {
            IssuesPane::Distribution => "distribution",
            IssuesPane::Urls => "urls",
        }
    }
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
    all_rows: Vec<CrawlRow>,
    rows: VecDeque<CrawlRow>,
    seen: HashSet<String>,
    discovered_seen: HashSet<String>,
    incoming_links: HashMap<String, HashSet<String>>,
    outgoing_links: HashMap<String, Vec<String>>,
    done: bool,
    errors: VecDeque<String>,
    status_counts: HashMap<u16, usize>,
    issue_counts: HashMap<SeoIssue, usize>,
    title_counts: HashMap<String, usize>,
    meta_counts: HashMap<String, usize>,
}

impl AppState {
    fn push_row(&mut self, row: CrawlRow, discovered_links: Vec<String>) -> bool {
        let mut dedup_outgoing_seen = HashSet::new();
        let dedup_outgoing = discovered_links
            .iter()
            .filter_map(|link| {
                if dedup_outgoing_seen.insert(link.clone()) {
                    Some(link.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        for link in &discovered_links {
            if link != &row.url {
                self.incoming_links
                    .entry(link.clone())
                    .or_default()
                    .insert(row.url.clone());
            }
        }
        for link in discovered_links {
            self.discovered_seen.insert(link);
        }

        let inserted = self.seen.insert(row.url.clone());
        if inserted {
            self.outgoing_links.insert(row.url.clone(), dedup_outgoing);
            *self.status_counts.entry(row.status).or_insert(0) += 1;
            for issue in &row.issues {
                *self.issue_counts.entry(*issue).or_insert(0) += 1;
            }
            if !row.title.is_empty() {
                *self
                    .title_counts
                    .entry(row.title.trim().to_ascii_lowercase())
                    .or_insert(0) += 1;
            }
            if !row.meta.is_empty() && row.retrieval_status == "retrieved" {
                *self
                    .meta_counts
                    .entry(row.meta.trim().to_ascii_lowercase())
                    .or_insert(0) += 1;
            }
            self.parsed += 1;
            self.all_rows.push(row.clone());
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

    fn discovered_total(&self) -> usize {
        self.discovered_targets
            .max(self.discovered_seen.len())
            .max(self.parsed)
    }

    fn average_seo_score(&self) -> u8 {
        let mut sum = 0u64;
        let mut count = 0u64;
        for row in &self.all_rows {
            if row.retrieval_status == "retrieved" {
                sum += row.seo_score as u64;
                count += 1;
            }
        }
        if count == 0 { 0 } else { (sum / count) as u8 }
    }

    fn duplicate_title_pages(&self) -> usize {
        self.title_counts
            .values()
            .filter(|count| **count > 1)
            .map(|count| *count)
            .sum()
    }

    fn duplicate_meta_pages(&self) -> usize {
        self.meta_counts
            .values()
            .filter(|count| **count > 1)
            .map(|count| *count)
            .sum()
    }

    fn top_issues(&self, limit: usize) -> Vec<(SeoIssue, usize)> {
        let mut entries = self
            .issue_counts
            .iter()
            .map(|(issue, count)| (*issue, *count))
            .collect::<Vec<_>>();
        entries.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.label().cmp(b.0.label())));
        entries.into_iter().take(limit.max(1)).collect()
    }

    fn incoming_sources(&self, url: &str, limit: usize) -> Vec<String> {
        let mut sources = self
            .incoming_links
            .get(url)
            .map(|set| set.iter().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        sources.sort();
        sources.into_iter().take(limit.max(1)).collect()
    }

    fn incoming_count(&self, url: &str) -> usize {
        self.incoming_links
            .get(url)
            .map(|set| set.len())
            .unwrap_or(0)
    }

    fn filtered_rows_sorted<'a>(
        &'a self,
        filter: &str,
        sort: PageSortMode,
        direction: SortDirection,
        limit: usize,
    ) -> Vec<&'a CrawlRow> {
        let filter = filter.trim().to_ascii_lowercase();
        let mut rows = self
            .all_rows
            .iter()
            .rev()
            .filter(|row| {
                if filter.is_empty() {
                    true
                } else {
                    row.url.to_ascii_lowercase().contains(&filter)
                        || row.title.to_ascii_lowercase().contains(&filter)
                        || row.meta.to_ascii_lowercase().contains(&filter)
                        || row
                            .issues
                            .iter()
                            .any(|issue| issue.label().contains(&filter))
                }
            })
            .collect::<Vec<_>>();

        match sort {
            PageSortMode::Latest => {
                if direction == SortDirection::Asc {
                    rows.reverse();
                }
            }
            PageSortMode::LowestSeoScore => {
                rows.sort_by(|a, b| a.seo_score.cmp(&b.seo_score).then(a.url.cmp(&b.url)));
                if direction == SortDirection::Desc {
                    rows.reverse();
                }
            }
            PageSortMode::HighestResponseTime => {
                rows.sort_by(|a, b| {
                    a.response_time
                        .cmp(&b.response_time)
                        .then_with(|| a.url.cmp(&b.url))
                });
                if direction == SortDirection::Desc {
                    rows.reverse();
                }
            }
        }

        rows.into_iter().take(limit.max(1)).collect()
    }
}

const CSV_HEADERS: [&str; 31] = [
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
    "word_count",
    "size",
    "response_time_ms",
    "last_modified",
    "redirect_url",
    "redirect_type",
    "link_count",
    "internal_link_count",
    "external_link_count",
    "h1_count",
    "h2_count",
    "image_count",
    "image_missing_alt_count",
    "structured_data_count",
    "seo_score",
    "issue_count",
    "issues",
    "outgoing_links",
    "crawl_timestamp",
    "crawl_quality_bucket",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExportRecord {
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
    response_time_ms: u128,
    last_modified: String,
    redirect_url: String,
    redirect_type: String,
    link_count: usize,
    internal_link_count: usize,
    external_link_count: usize,
    h1_count: usize,
    h2_count: usize,
    image_count: usize,
    image_missing_alt_count: usize,
    structured_data_count: usize,
    seo_score: u8,
    issue_count: usize,
    issues: String,
    outgoing_links: Vec<String>,
    crawl_timestamp: String,
    crawl_quality_bucket: String,
}

fn crawl_quality_bucket(seo_score: u8) -> &'static str {
    match seo_score {
        85..=100 => "excellent",
        70..=84 => "good",
        50..=69 => "warning",
        _ => "critical",
    }
}

fn row_to_export_record(row: &CrawlRow, outgoing_links: &[String]) -> ExportRecord {
    ExportRecord {
        url: row.url.clone(),
        status: row.status,
        mime: row.mime.clone(),
        retrieval_status: row.retrieval_status.clone(),
        indexability: row.indexability.clone(),
        title: row.title.clone(),
        title_length: row.title_length,
        meta: row.meta.clone(),
        meta_length: row.meta_length,
        h1: row.h1.clone(),
        canonical: row.canonical.clone(),
        word_count: row.word_count,
        size: row.size,
        response_time_ms: row.response_time,
        last_modified: row.last_modified.clone(),
        redirect_url: row.redirect_url.clone(),
        redirect_type: row.redirect_type.clone(),
        link_count: row.link_count,
        internal_link_count: row.internal_link_count,
        external_link_count: row.external_link_count,
        h1_count: row.h1_count,
        h2_count: row.h2_count,
        image_count: row.image_count,
        image_missing_alt_count: row.image_missing_alt_count,
        structured_data_count: row.structured_data_count,
        seo_score: row.seo_score,
        issue_count: row.issues.len(),
        issues: issues_to_csv(&row.issues),
        outgoing_links: outgoing_links.to_vec(),
        crawl_timestamp: row.crawl_timestamp.clone(),
        crawl_quality_bucket: crawl_quality_bucket(row.seo_score).to_string(),
    }
}

fn export_record_to_row(record: ExportRecord) -> (CrawlRow, Vec<String>) {
    let mut issues = record
        .issues
        .split('|')
        .filter_map(SeoIssue::from_label)
        .collect::<Vec<_>>();
    if issues.is_empty() && record.retrieval_status == "not_retrieved" {
        issues.push(SeoIssue::NotRetrieved);
    }

    (
        CrawlRow {
            url: record.url,
            status: record.status,
            mime: record.mime,
            retrieval_status: record.retrieval_status,
            indexability: record.indexability,
            title: record.title,
            title_length: record.title_length,
            meta: record.meta,
            meta_length: record.meta_length,
            h1: record.h1,
            canonical: record.canonical,
            word_count: record.word_count,
            size: record.size,
            response_time: record.response_time_ms,
            last_modified: record.last_modified,
            redirect_url: record.redirect_url,
            redirect_type: record.redirect_type,
            link_count: record.link_count,
            internal_link_count: record.internal_link_count,
            external_link_count: record.external_link_count,
            h1_count: record.h1_count,
            h2_count: record.h2_count,
            image_count: record.image_count,
            image_missing_alt_count: record.image_missing_alt_count,
            structured_data_count: record.structured_data_count,
            seo_score: if record.seo_score == 0 && !issues.is_empty() {
                compute_seo_score(&issues)
            } else {
                record.seo_score
            },
            issues,
            crawl_timestamp: record.crawl_timestamp,
        },
        record.outgoing_links,
    )
}

struct CsvSink {
    writer: csv::Writer<File>,
}

impl CsvSink {
    fn new(output_path: &str) -> io::Result<Self> {
        let file = File::create(output_path)?;
        let mut writer = csv::Writer::from_writer(file);
        writer.write_record(CSV_HEADERS)?;
        Ok(Self { writer })
    }

    fn write_row(&mut self, row: &CrawlRow, outgoing_links: &[String]) -> io::Result<()> {
        let rec = row_to_export_record(row, outgoing_links);
        self.writer.write_record([
            rec.url,
            rec.status.to_string(),
            rec.mime,
            rec.retrieval_status,
            rec.indexability,
            rec.title,
            rec.title_length.to_string(),
            rec.meta,
            rec.meta_length.to_string(),
            rec.h1,
            rec.canonical,
            rec.word_count.to_string(),
            rec.size.to_string(),
            rec.response_time_ms.to_string(),
            rec.last_modified,
            rec.redirect_url,
            rec.redirect_type,
            rec.link_count.to_string(),
            rec.internal_link_count.to_string(),
            rec.external_link_count.to_string(),
            rec.h1_count.to_string(),
            rec.h2_count.to_string(),
            rec.image_count.to_string(),
            rec.image_missing_alt_count.to_string(),
            rec.structured_data_count.to_string(),
            rec.seo_score.to_string(),
            rec.issue_count.to_string(),
            rec.issues,
            rec.outgoing_links.join("|"),
            rec.crawl_timestamp,
            rec.crawl_quality_bucket,
        ])?;
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

struct JsonSink {
    file: File,
    first: bool,
    closed: bool,
}

impl JsonSink {
    fn new(output_path: &str) -> io::Result<Self> {
        let mut file = File::create(output_path)?;
        file.write_all(b"[\n")?;
        Ok(Self {
            file,
            first: true,
            closed: false,
        })
    }

    fn write_row(&mut self, row: &CrawlRow, outgoing_links: &[String]) -> io::Result<()> {
        let rec = row_to_export_record(row, outgoing_links);
        if !self.first {
            self.file.write_all(b",\n")?;
        }
        self.first = false;
        serde_json::to_writer(&mut self.file, &rec).map_err(io::Error::other)?;
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }

    fn finalize(&mut self) -> io::Result<()> {
        if !self.closed {
            if self.first {
                self.file.write_all(b"]\n")?;
            } else {
                self.file.write_all(b"\n]\n")?;
            }
            self.closed = true;
        }
        self.file.flush()
    }
}

impl Drop for JsonSink {
    fn drop(&mut self) {
        let _ = self.finalize();
    }
}

enum OutputSink {
    Csv(CsvSink),
    Json(JsonSink),
}

impl OutputSink {
    fn new(output_path: &str, format: DataFormat) -> io::Result<Self> {
        match format {
            DataFormat::Csv => Ok(OutputSink::Csv(CsvSink::new(output_path)?)),
            DataFormat::Json => Ok(OutputSink::Json(JsonSink::new(output_path)?)),
        }
    }

    fn write_row(&mut self, row: &CrawlRow, outgoing_links: &[String]) -> io::Result<()> {
        match self {
            OutputSink::Csv(sink) => sink.write_row(row, outgoing_links),
            OutputSink::Json(sink) => sink.write_row(row, outgoing_links),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            OutputSink::Csv(sink) => sink.flush(),
            OutputSink::Json(sink) => sink.flush(),
        }
    }

    fn finalize(&mut self) -> io::Result<()> {
        match self {
            OutputSink::Csv(sink) => sink.flush(),
            OutputSink::Json(sink) => sink.finalize(),
        }
    }
}

fn issues_to_csv(issues: &[SeoIssue]) -> String {
    issues
        .iter()
        .map(|issue| issue.label())
        .collect::<Vec<_>>()
        .join("|")
}

fn load_rows_from_csv(path: &str) -> io::Result<Vec<(CrawlRow, Vec<String>)>> {
    let mut reader = csv::Reader::from_path(path)?;
    let headers = reader.headers()?.clone();
    let mut index = HashMap::<String, usize>::new();
    for (idx, header) in headers.iter().enumerate() {
        index.insert(header.trim().to_ascii_lowercase(), idx);
    }

    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record?;
        let get = |names: &[&str]| -> String {
            for name in names {
                if let Some(idx) = index.get(&name.to_ascii_lowercase())
                    && let Some(value) = record.get(*idx)
                {
                    return value.to_string();
                }
            }
            String::new()
        };

        let url = get(&["url"]);
        if url.trim().is_empty() {
            continue;
        }
        let status = get(&["status"]).parse::<u16>().unwrap_or(0);
        let retrieval_status = get(&["retrieval_status"]);
        let title = get(&["title"]);
        let meta = get(&["meta"]);
        let h1 = get(&["h1"]);
        let canonical = get(&["canonical"]);
        let issues_raw = get(&["issues"]);
        let rec = ExportRecord {
            url,
            status,
            mime: get(&["mime"]),
            retrieval_status: if retrieval_status.is_empty() {
                if status == 0 {
                    "not_retrieved".to_string()
                } else {
                    "retrieved".to_string()
                }
            } else {
                retrieval_status
            },
            indexability: get(&["indexability"]),
            title_length: get(&["title_length"])
                .parse::<usize>()
                .unwrap_or(title.chars().count()),
            title,
            meta_length: get(&["meta_length"])
                .parse::<usize>()
                .unwrap_or(meta.chars().count()),
            meta,
            h1,
            canonical,
            word_count: get(&["word_count", "word count"])
                .parse::<usize>()
                .unwrap_or(0),
            size: get(&["size"]).parse::<usize>().unwrap_or(0),
            response_time_ms: get(&["response_time_ms", "response_time"])
                .parse::<u128>()
                .unwrap_or(0),
            last_modified: get(&["last_modified"]),
            redirect_url: get(&["redirect_url", "redirect url"]),
            redirect_type: get(&["redirect_type", "redirect type"]),
            link_count: get(&["link_count", "link count"])
                .parse::<usize>()
                .unwrap_or(0),
            internal_link_count: get(&["internal_link_count"]).parse::<usize>().unwrap_or(0),
            external_link_count: get(&["external_link_count"]).parse::<usize>().unwrap_or(0),
            h1_count: get(&["h1_count"]).parse::<usize>().unwrap_or(0),
            h2_count: get(&["h2_count"]).parse::<usize>().unwrap_or(0),
            image_count: get(&["image_count"]).parse::<usize>().unwrap_or(0),
            image_missing_alt_count: get(&["image_missing_alt_count"])
                .parse::<usize>()
                .unwrap_or(0),
            structured_data_count: get(&["structured_data_count"])
                .parse::<usize>()
                .unwrap_or(0),
            seo_score: get(&["seo_score"]).parse::<u8>().unwrap_or(0),
            issue_count: get(&["issue_count"]).parse::<usize>().unwrap_or(0),
            issues: issues_raw,
            outgoing_links: get(&["outgoing_links"])
                .split('|')
                .map(str::trim)
                .filter(|link| !link.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            crawl_timestamp: get(&["crawl_timestamp", "crawl timestamp"]),
            crawl_quality_bucket: get(&["crawl_quality_bucket"]),
        };
        rows.push(export_record_to_row(rec));
    }

    Ok(rows)
}

fn load_rows_from_json(path: &str) -> io::Result<Vec<(CrawlRow, Vec<String>)>> {
    let content = fs::read_to_string(path)?;
    if content.trim().is_empty() {
        return Ok(Vec::new());
    }

    if let Ok(records) = serde_json::from_str::<Vec<ExportRecord>>(&content) {
        return Ok(records.into_iter().map(export_record_to_row).collect());
    }

    let mut out = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let record = serde_json::from_str::<ExportRecord>(line)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))?;
        out.push(export_record_to_row(record));
    }
    Ok(out)
}

fn detect_data_format(path: &str, fallback: DataFormat) -> DataFormat {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".json") {
        DataFormat::Json
    } else if lower.ends_with(".csv") {
        DataFormat::Csv
    } else {
        fallback
    }
}

fn load_rows_from_file(path: &str) -> io::Result<Vec<(CrawlRow, Vec<String>)>> {
    match detect_data_format(path, DataFormat::Csv) {
        DataFormat::Csv => load_rows_from_csv(path),
        DataFormat::Json => load_rows_from_json(path),
    }
}

fn default_output_path(url: &str, format: DataFormat) -> String {
    let host = Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
        .unwrap_or_else(|| "crawl".to_string());
    let host = host
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    let ts = Utc::now().format("%Y%m%d_%H%M%S");
    match format {
        DataFormat::Csv => format!("{host}_{ts}.csv"),
        DataFormat::Json => format!("{host}_{ts}.json"),
    }
}

fn handle_crawl_event(
    state: &mut AppState,
    sink: Option<&mut OutputSink>,
    event: CrawlEvent,
) -> io::Result<()> {
    match event {
        CrawlEvent::Page {
            row,
            discovered_links,
        } => {
            if state.push_row(row.clone(), discovered_links.clone()) {
                if let Some(sink) = sink {
                    sink.write_row(&row, &discovered_links)?;
                }
            }
        }
        CrawlEvent::Unretrieved { url, reason } => {
            let row = unretrieved_row(url, reason);
            if state.push_row(row.clone(), Vec::new()) {
                if let Some(sink) = sink {
                    sink.write_row(&row, &[])?;
                }
            }
        }
        CrawlEvent::Stats { discovered } => {
            state.discovered_targets = state.discovered_targets.max(discovered);
        }
        CrawlEvent::Finished => state.done = true,
        CrawlEvent::Error(err) => state.push_error(err),
    }

    Ok(())
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let cli = Cli::parse();
    let auto_close = cli.auto_close;
    let no_tui = cli.no_tui;
    if let Some(review_file) = cli.review_file.clone() {
        let review_rows = load_rows_from_file(&review_file)?;
        let (tx, mut rx) = mpsc::unbounded_channel::<CrawlEvent>();
        for (row, outgoing_links) in review_rows {
            let _ = tx.send(CrawlEvent::Page {
                row,
                discovered_links: outgoing_links,
            });
        }
        let _ = tx.send(CrawlEvent::Finished);
        drop(tx);

        if no_tui {
            return run_review_headless(&review_file, &mut rx);
        }
        return run_tui(&review_file, None, auto_close, &mut rx);
    }

    let start_url = cli
        .url
        .as_deref()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing URL"))?;
    let configured_format: DataFormat = cli.format.into();
    let output_format = cli
        .output
        .as_deref()
        .map(|path| detect_data_format(path, configured_format))
        .unwrap_or(configured_format);
    let output_path = cli
        .output
        .clone()
        .unwrap_or_else(|| default_output_path(start_url, output_format));

    let (tx, mut rx) = mpsc::unbounded_channel::<CrawlEvent>();
    let crawl_handle = tokio::spawn(run_crawler(cli, tx));
    let tui_result = if no_tui {
        run_headless(&output_path, output_format, &mut rx)
    } else {
        run_tui(
            &output_path,
            Some((output_path.as_str(), output_format)),
            auto_close,
            &mut rx,
        )
    };

    if let Err(e) = crawl_handle.await {
        eprintln!("crawler task join error: {e}");
    }

    tui_result
}

fn run_headless(
    output_path: &str,
    output_format: DataFormat,
    rx: &mut UnboundedReceiver<CrawlEvent>,
) -> io::Result<()> {
    let mut sink = OutputSink::new(output_path, output_format)?;
    let mut state = AppState::default();
    loop {
        while let Ok(event) = rx.try_recv() {
            if let CrawlEvent::Error(err) = &event {
                eprintln!("{err}");
            }
            handle_crawl_event(&mut state, Some(&mut sink), event)?;
        }

        sink.flush()?;
        if state.done {
            break;
        }
        std::thread::sleep(Duration::from_millis(120));
    }

    sink.finalize()?;
    eprintln!(
        "finished crawl: parsed={} discovered={} avg_score={} output={}",
        state.parsed,
        state.discovered_total(),
        state.average_seo_score(),
        output_path
    );
    Ok(())
}

fn run_review_headless(
    review_path: &str,
    rx: &mut UnboundedReceiver<CrawlEvent>,
) -> io::Result<()> {
    let mut state = AppState::default();
    loop {
        while let Ok(event) = rx.try_recv() {
            if let CrawlEvent::Error(err) = &event {
                eprintln!("{err}");
            }
            handle_crawl_event(&mut state, None, event)?;
        }

        if state.done {
            break;
        }
        std::thread::sleep(Duration::from_millis(120));
    }

    eprintln!(
        "loaded review dataset: parsed={} discovered={} avg_score={} input={}",
        state.parsed,
        state.discovered_total(),
        state.average_seo_score(),
        review_path
    );
    Ok(())
}

fn run_tui(
    session_label: &str,
    output_target: Option<(&str, DataFormat)>,
    auto_close: bool,
    rx: &mut UnboundedReceiver<CrawlEvent>,
) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let tui_result = draw_loop(&mut terminal, session_label, output_target, auto_close, rx);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    tui_result
}

fn draw_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    session_label_input: &str,
    output_target: Option<(&str, DataFormat)>,
    auto_close: bool,
    rx: &mut UnboundedReceiver<CrawlEvent>,
) -> io::Result<()> {
    let mut sink = if let Some((path, format)) = output_target {
        Some(OutputSink::new(path, format)?)
    } else {
        None
    };
    let session_label = session_label_input.to_string();
    let mut state = AppState::default();
    let mut last_tick = Instant::now();
    let tick_rate = Duration::from_millis(120);
    let mut active_panel = ActivePanel::Pages;
    let mut sort_mode = PageSortMode::Latest;
    let mut sort_direction = SortDirection::Desc;
    let mut filter = String::new();
    let mut filter_mode = false;
    let mut paused = false;
    let mut selected_page_idx = 0usize;
    let mut selected_issue_idx = 0usize;
    let mut selected_issue_page_idx = 0usize;
    let mut page_table_state = TableState::default();
    let mut issue_table_state = TableState::default();
    let mut issue_page_table_state = TableState::default();
    let mut paused_events = VecDeque::<CrawlEvent>::new();
    let mut page_table_area: Option<Rect> = None;
    let mut issue_distribution_area: Option<Rect> = None;
    let mut issue_urls_area: Option<Rect> = None;
    let mut page_view_urls: Vec<String> = Vec::new();
    let mut issue_view_urls: Vec<String> = Vec::new();
    let mut pages_pane = PagesPane::Table;
    let mut issues_pane = IssuesPane::Distribution;
    let mut hovered_page_url_idx: Option<usize> = None;
    let mut hovered_issue_url_idx: Option<usize> = None;
    let mut last_page_click: Option<(usize, Instant)> = None;
    let mut last_issue_url_click: Option<(usize, Instant)> = None;

    loop {
        if !paused {
            for _ in 0..300 {
                let Some(event) = paused_events.pop_front() else {
                    break;
                };
                handle_crawl_event(&mut state, sink.as_mut(), event)?;
            }
        }

        while let Ok(event) = rx.try_recv() {
            if paused
                && matches!(
                    event,
                    CrawlEvent::Page { .. } | CrawlEvent::Unretrieved { .. }
                )
            {
                paused_events.push_back(event);
                if paused_events.len() > 20_000 {
                    paused_events.pop_front();
                }
                continue;
            }
            handle_crawl_event(&mut state, sink.as_mut(), event)?;
        }

        terminal.draw(|f| {
            page_table_area = None;
            issue_distribution_area = None;
            issue_urls_area = None;
            page_view_urls.clear();
            issue_view_urls.clear();

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(5),
                    Constraint::Length(3),
                    Constraint::Min(12),
                    Constraint::Length(5),
                ])
                .split(f.area());

            let crawl_title = if state.done {
                if auto_close {
                    "gh0st Crawl- Finished (auto-closing)"
                } else {
                    "gh0st Crawl - Finished (press q to quit)"
                }
            } else {
                "gh0st Crawl - Running (press q to quit)"
            };

            let discovered_total = state.discovered_total();
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
                    Span::styled("Avg SEO ", metric_label),
                    Span::styled(
                        state.average_seo_score().to_string(),
                        seo_score_style(state.average_seo_score()),
                    ),
                    Span::styled("  |  ", sep_style),
                    Span::styled("Dup title ", metric_label),
                    Span::styled(
                        state.duplicate_title_pages().to_string(),
                        Style::default().fg(Color::Yellow),
                    ),
                    Span::styled("  |  ", sep_style),
                    Span::styled("Dup meta ", metric_label),
                    Span::styled(
                        state.duplicate_meta_pages().to_string(),
                        Style::default().fg(Color::Yellow),
                    ),
                    Span::styled("  |  ", sep_style),
                    Span::styled(
                        if output_target.is_some() {
                            "CSV "
                        } else {
                            "Dataset "
                        },
                        metric_label,
                    ),
                    Span::styled(session_label.clone(), Style::default().fg(Color::White)),
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
            let controls = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(24), Constraint::Min(20)])
                .split(chunks[1]);

            let hotkey_style = Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD);
            let tab_label_style = Style::default().fg(Color::Gray);
            let tabs = Tabs::new(vec![
                Line::from(vec![
                    Span::styled("P", hotkey_style),
                    Span::styled(" Pages", tab_label_style),
                ]),
                Line::from(vec![
                    Span::styled("I", hotkey_style),
                    Span::styled(" Issues", tab_label_style),
                ]),
                Line::from(vec![
                    Span::styled("S", hotkey_style),
                    Span::styled(" Summary", tab_label_style),
                ]),
            ])
            .select(active_panel.as_index())
            .block(Block::default().title("Panel").borders(Borders::ALL))
            .highlight_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            );
            f.render_widget(tabs, controls[0]);

            let gauge = Gauge::default()
                .block(Block::default().title("Progress").borders(Borders::ALL))
                .gauge_style(
                    Style::default()
                        .fg(Color::Cyan)
                        .bg(Color::Black)
                        .add_modifier(Modifier::BOLD),
                )
                .ratio(ratio.clamp(0.0, 1.0))
                .label(format!(
                    "{:.1}% | quality {} | {}{}",
                    ratio * 100.0,
                    state.average_seo_score(),
                    if paused { "paused" } else { "live" },
                    if paused_events.is_empty() {
                        String::new()
                    } else {
                        format!(" (buffered {})", paused_events.len())
                    }
                ));
            f.render_widget(gauge, controls[1]);

            match active_panel {
                ActivePanel::Pages => {
                    let panel_chunks = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Percentage(66), Constraint::Percentage(34)])
                        .split(chunks[2]);

                    let page_rows =
                        state.filtered_rows_sorted(&filter, sort_mode, sort_direction, 400);
                    page_view_urls = page_rows.iter().map(|row| row.url.clone()).collect();
                    if page_rows.is_empty() {
                        selected_page_idx = 0;
                        page_table_state.select(None);
                    } else {
                        selected_page_idx = selected_page_idx.min(page_rows.len() - 1);
                        page_table_state.select(Some(selected_page_idx));
                    }

                    let rows = page_rows.iter().enumerate().map(|(idx, r)| {
                        let url_style = if hovered_page_url_idx == Some(idx) {
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::UNDERLINED)
                        } else {
                            Style::default()
                        };
                        Row::new(vec![
                            Cell::from(r.status.to_string()).style(status_code_style(r.status)),
                            Cell::from(r.seo_score.to_string()).style(seo_score_style(r.seo_score)),
                            Cell::from(r.response_time.to_string()),
                            Cell::from(r.title.clone()),
                            Cell::from(r.url.clone()).style(url_style),
                        ])
                    });
                    let pages_table = Table::new(
                        rows,
                        [
                            Constraint::Length(8),
                            Constraint::Length(6),
                            Constraint::Length(9),
                            Constraint::Length(34),
                            Constraint::Min(20),
                        ],
                    )
                    .header(
                        Row::new(vec!["Status", "SEO", "RT(ms)", "Title", "URL"])
                            .style(Style::default().add_modifier(Modifier::BOLD)),
                    )
                    .row_highlight_style(
                        Style::default()
                            .bg(Color::DarkGray)
                            .add_modifier(Modifier::BOLD),
                    )
                    .block(
                        Block::default()
                            .title(format!("Pages ({})", page_rows.len()))
                            .borders(Borders::ALL)
                            .border_style(if pages_pane == PagesPane::Table {
                                Style::default().fg(Color::Cyan)
                            } else {
                                Style::default().fg(Color::DarkGray)
                            }),
                    )
                    .column_spacing(1);
                    page_table_area = Some(panel_chunks[0]);
                    f.render_stateful_widget(pages_table, panel_chunks[0], &mut page_table_state);

                    let detail = if let Some(row) = page_rows.get(selected_page_idx) {
                        let issues = if row.issues.is_empty() {
                            "none".to_string()
                        } else {
                            row.issues
                                .iter()
                                .map(|i| i.label())
                                .collect::<Vec<_>>()
                                .join(", ")
                        };
                        let incoming = state.incoming_sources(&row.url, 5);
                        let incoming_count = state.incoming_count(&row.url);
                        let incoming_preview = if incoming.is_empty() {
                            "none".to_string()
                        } else {
                            incoming.join(" | ")
                        };
                        vec![
                            Line::from(format!("URL: {}", row.url)),
                            Line::from(format!("Title: {}", row.title)),
                            Line::from(format!(
                                "SEO score: {} | Status: {} | Indexability: {}",
                                row.seo_score, row.status, row.indexability
                            )),
                            Line::from(format!(
                                "H1/H2: {}/{} | Words: {} | Images: {} (missing alt {})",
                                row.h1_count,
                                row.h2_count,
                                row.word_count,
                                row.image_count,
                                row.image_missing_alt_count
                            )),
                            Line::from(format!(
                                "Links: {} (internal {} / external {})",
                                row.link_count, row.internal_link_count, row.external_link_count
                            )),
                            Line::from(format!(
                                "Incoming refs: {}{}",
                                incoming_count,
                                if row.status == 404 {
                                    " (check these for 404 source)"
                                } else {
                                    ""
                                }
                            )),
                            Line::from(format!("Referrers: {}", incoming_preview)),
                            Line::from(format!(
                                "Structured data blocks: {}",
                                row.structured_data_count
                            )),
                            Line::from(format!("Issues: {}", issues)),
                        ]
                    } else {
                        vec![Line::from("No pages match the current filter")]
                    };
                    let details = Paragraph::new(detail)
                        .block(
                            Block::default()
                                .title("Selected Page")
                                .borders(Borders::ALL)
                                .border_style(if pages_pane == PagesPane::Details {
                                    Style::default().fg(Color::Cyan)
                                } else {
                                    Style::default().fg(Color::DarkGray)
                                }),
                        )
                        .wrap(Wrap { trim: true });
                    f.render_widget(details, panel_chunks[1]);
                }
                ActivePanel::Issues => {
                    let panel_chunks = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
                        .split(chunks[2]);

                    let mut issue_nav_entries: Vec<(String, usize, Option<SeoIssue>)> = vec![(
                        "Most Problematic Pages".to_string(),
                        state
                            .all_rows
                            .iter()
                            .filter(|row| !row.issues.is_empty())
                            .count(),
                        None,
                    )];
                    issue_nav_entries.extend(
                        state
                            .top_issues(30)
                            .into_iter()
                            .map(|(issue, count)| (issue.label().to_string(), count, Some(issue))),
                    );

                    selected_issue_idx = selected_issue_idx.min(issue_nav_entries.len() - 1);
                    issue_table_state.select(Some(selected_issue_idx));

                    let rows = issue_nav_entries.iter().map(|(label, count, issue)| {
                        let style = if issue.is_none() {
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default()
                        };
                        Row::new(vec![
                            Cell::from(label.clone()).style(style),
                            Cell::from(count.to_string()),
                        ])
                    });
                    let issues_table =
                        Table::new(rows, [Constraint::Min(22), Constraint::Length(10)])
                            .header(
                                Row::new(vec!["Issue", "Pages"])
                                    .style(Style::default().add_modifier(Modifier::BOLD)),
                            )
                            .row_highlight_style(
                                Style::default()
                                    .bg(Color::DarkGray)
                                    .add_modifier(Modifier::BOLD),
                            )
                            .block(
                                Block::default()
                                    .title("Issue Distribution")
                                    .borders(Borders::ALL)
                                    .border_style(if issues_pane == IssuesPane::Distribution {
                                        Style::default().fg(Color::Cyan)
                                    } else {
                                        Style::default().fg(Color::DarkGray)
                                    }),
                            )
                            .column_spacing(1);
                    issue_distribution_area = Some(panel_chunks[0]);
                    f.render_stateful_widget(issues_table, panel_chunks[0], &mut issue_table_state);

                    let selected_issue = issue_nav_entries
                        .get(selected_issue_idx)
                        .and_then(|(_, _, issue)| *issue);
                    let mut filtered_pages = state
                        .all_rows
                        .iter()
                        .filter(|row| match selected_issue {
                            Some(issue) => row.issues.contains(&issue),
                            None => true,
                        })
                        .collect::<Vec<_>>();
                    filtered_pages.sort_by(|a, b| {
                        b.issues
                            .len()
                            .cmp(&a.issues.len())
                            .then(a.seo_score.cmp(&b.seo_score))
                            .then_with(|| a.url.cmp(&b.url))
                    });
                    issue_view_urls = filtered_pages
                        .iter()
                        .take(100)
                        .map(|row| row.url.clone())
                        .collect::<Vec<_>>();
                    if issue_view_urls.is_empty() {
                        selected_issue_page_idx = 0;
                        issue_page_table_state.select(None);
                    } else {
                        selected_issue_page_idx =
                            selected_issue_page_idx.min(issue_view_urls.len() - 1);
                        issue_page_table_state.select(Some(selected_issue_page_idx));
                    }

                    let rows = filtered_pages
                        .iter()
                        .take(100)
                        .enumerate()
                        .map(|(idx, row)| {
                            let url_style = if hovered_issue_url_idx == Some(idx) {
                                Style::default()
                                    .fg(Color::Cyan)
                                    .add_modifier(Modifier::UNDERLINED)
                            } else {
                                Style::default()
                            };
                            Row::new(vec![
                                Cell::from(row.seo_score.to_string())
                                    .style(seo_score_style(row.seo_score)),
                                Cell::from(row.issues.len().to_string()),
                                Cell::from(row.status.to_string())
                                    .style(status_code_style(row.status)),
                                Cell::from(state.incoming_count(&row.url).to_string()),
                                Cell::from(row.url.clone()).style(url_style),
                            ])
                        });
                    let table = Table::new(
                        rows,
                        [
                            Constraint::Length(8),
                            Constraint::Length(8),
                            Constraint::Length(8),
                            Constraint::Length(8),
                            Constraint::Min(20),
                        ],
                    )
                    .header(
                        Row::new(vec!["SEO", "Issues", "Status", "In", "URL"])
                            .style(Style::default().add_modifier(Modifier::BOLD)),
                    )
                    .row_highlight_style(
                        Style::default()
                            .bg(Color::DarkGray)
                            .add_modifier(Modifier::BOLD),
                    )
                    .block(
                        Block::default()
                            .title(match selected_issue {
                                Some(issue) => format!(
                                    "Filtered Pages for '{}' ({})",
                                    issue.label(),
                                    filtered_pages.len()
                                ),
                                None => {
                                    format!("Most Problematic Pages ({})", filtered_pages.len())
                                }
                            })
                            .borders(Borders::ALL)
                            .border_style(if issues_pane == IssuesPane::Urls {
                                Style::default().fg(Color::Cyan)
                            } else {
                                Style::default().fg(Color::DarkGray)
                            }),
                    )
                    .column_spacing(1);
                    issue_urls_area = Some(panel_chunks[1]);
                    f.render_stateful_widget(table, panel_chunks[1], &mut issue_page_table_state);
                }
                ActivePanel::Summary => {
                    let top_issues = state
                        .top_issues(8)
                        .into_iter()
                        .map(|(issue, count)| format!("{}: {}", issue.label(), count))
                        .collect::<Vec<_>>()
                        .join(" | ");
                    let summary_lines = vec![
                        Line::from(format!("Pages parsed: {}", state.parsed)),
                        Line::from(format!("Discovered crawl targets: {}", discovered_total)),
                        Line::from(format!("Average SEO score: {}", state.average_seo_score())),
                        Line::from(format!(
                            "Duplicate title pages: {}",
                            state.duplicate_title_pages()
                        )),
                        Line::from(format!(
                            "Duplicate meta description pages: {}",
                            state.duplicate_meta_pages()
                        )),
                        Line::from(format!(
                            "Indexed candidates (2xx + indexable): {}",
                            state
                                .all_rows
                                .iter()
                                .filter(|row| {
                                    (200..=299).contains(&row.status)
                                        && row.indexability == "Indexable"
                                })
                                .count()
                        )),
                        Line::from(format!(
                            "Top issues: {}",
                            if top_issues.is_empty() {
                                "none".to_string()
                            } else {
                                top_issues
                            }
                        )),
                    ];
                    let summary = Paragraph::new(summary_lines)
                        .block(
                            Block::default()
                                .title("Site SEO Summary")
                                .borders(Borders::ALL),
                        )
                        .wrap(Wrap { trim: true });
                    f.render_widget(summary, chunks[2]);
                }
            }

            let error_count = state.errors.len();
            let last_error = state
                .errors
                .front()
                .map(|e| truncate_for_log(e, 170))
                .unwrap_or_else(|| "none".to_string());
            let footer_status_style = if error_count > 0 {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            } else if paused {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else if state.done {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            };
            let footer_border_style = if error_count > 0 {
                Style::default().fg(Color::Red)
            } else if filter_mode {
                Style::default().fg(Color::Yellow)
            } else if paused {
                Style::default().fg(Color::Magenta)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let state_label = if state.done {
                "DONE"
            } else if paused {
                "PAUSED"
            } else {
                "LIVE"
            };
            let mode_label = if filter_mode {
                "FILTER INPUT"
            } else {
                "NAVIGATION"
            };
            let pane_label = match active_panel {
                ActivePanel::Pages => pages_pane.label(),
                ActivePanel::Issues => issues_pane.label(),
                ActivePanel::Summary => "summary",
            };
            let footer_lines = vec![
                Line::from(vec![
                    Span::styled("STATE ", Style::default().fg(Color::DarkGray)),
                    Span::styled(state_label, footer_status_style),
                    Span::styled("   MODE ", Style::default().fg(Color::DarkGray)),
                    Span::styled(mode_label, Style::default().fg(Color::Yellow)),
                    Span::styled("   PANEL ", Style::default().fg(Color::DarkGray)),
                    Span::styled(active_panel.title(), Style::default().fg(Color::Cyan)),
                    Span::styled("   FOCUS ", Style::default().fg(Color::DarkGray)),
                    Span::styled(pane_label, Style::default().fg(Color::LightCyan)),
                    Span::styled("   SORT ", Style::default().fg(Color::DarkGray)),
                    Span::styled(sort_mode.title(), Style::default().fg(Color::LightCyan)),
                    Span::styled("   DIR ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        sort_direction.label(),
                        Style::default().fg(Color::LightCyan),
                    ),
                    Span::styled("   FILTER ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        if filter.is_empty() { "none" } else { &filter },
                        Style::default().fg(if filter.is_empty() {
                            Color::DarkGray
                        } else {
                            Color::White
                        }),
                    ),
                    Span::styled("   BUFFER ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        paused_events.len().to_string(),
                        Style::default().fg(if paused_events.is_empty() {
                            Color::Green
                        } else {
                            Color::Yellow
                        }),
                    ),
                    Span::styled("   ERRORS ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        error_count.to_string(),
                        Style::default().fg(if error_count > 0 {
                            Color::Red
                        } else {
                            Color::Green
                        }),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("LAST ERROR ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        last_error,
                        Style::default().fg(if error_count > 0 {
                            Color::LightRed
                        } else {
                            Color::DarkGray
                        }),
                    ),
                ]),
                Line::from(vec![
                    Span::styled(
                        "q",
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" quit  ", Style::default().fg(Color::Gray)),
                    Span::styled(
                        "tab",
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" next pane  ", Style::default().fg(Color::Gray)),
                    Span::styled(
                        "shift+tab",
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" prev pane  ", Style::default().fg(Color::Gray)),
                    Span::styled(
                        "P I S",
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" panels  ", Style::default().fg(Color::Gray)),
                    Span::styled(
                        "up/down",
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" select  ", Style::default().fg(Color::Gray)),
                    Span::styled(
                        "dbl-click",
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" open link  ", Style::default().fg(Color::Gray)),
                    Span::styled(
                        "mod+click",
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" force open  ", Style::default().fg(Color::Gray)),
                    Span::styled(
                        "enter",
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" open selected URL  ", Style::default().fg(Color::Gray)),
                    Span::styled(
                        "r",
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" sort  ", Style::default().fg(Color::Gray)),
                    Span::styled(
                        "d",
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" direction  ", Style::default().fg(Color::Gray)),
                    Span::styled(
                        "/",
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" filter  ", Style::default().fg(Color::Gray)),
                    Span::styled(
                        "space",
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" pause/resume  ", Style::default().fg(Color::Gray)),
                    Span::styled(
                        "esc/enter",
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" exit filter", Style::default().fg(Color::Gray)),
                ]),
            ];

            let footer = Paragraph::new(footer_lines)
                .block(
                    Block::default()
                        .title("Command & Health Bar")
                        .borders(Borders::ALL)
                        .border_style(footer_border_style),
                )
                .wrap(Wrap { trim: true });
            f.render_widget(footer, chunks[3]);
        })?;

        if let Some(sink) = sink.as_mut() {
            sink.flush()?;
        }

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) => {
                    if filter_mode {
                        match key.code {
                            KeyCode::Esc | KeyCode::Enter => filter_mode = false,
                            KeyCode::Backspace => {
                                filter.pop();
                            }
                            KeyCode::Char(ch) => {
                                filter.push(ch);
                            }
                            _ => {}
                        }
                    } else {
                        match key.code {
                            KeyCode::Char('q') => break,
                            KeyCode::Tab => match active_panel {
                                ActivePanel::Pages => pages_pane = pages_pane.cycle(),
                                ActivePanel::Issues => issues_pane = issues_pane.cycle(),
                                ActivePanel::Summary => {}
                            },
                            KeyCode::BackTab => match active_panel {
                                ActivePanel::Pages => pages_pane = pages_pane.reverse_cycle(),
                                ActivePanel::Issues => issues_pane = issues_pane.reverse_cycle(),
                                ActivePanel::Summary => {}
                            },
                            KeyCode::Char('p') | KeyCode::Char('P') => {
                                active_panel = ActivePanel::Pages
                            }
                            KeyCode::Char('i') | KeyCode::Char('I') => {
                                active_panel = ActivePanel::Issues
                            }
                            KeyCode::Char('s') | KeyCode::Char('S') => {
                                active_panel = ActivePanel::Summary
                            }
                            KeyCode::Char('r') | KeyCode::Char('R') => {
                                sort_mode = sort_mode.cycle()
                            }
                            KeyCode::Char('d') | KeyCode::Char('D') => {
                                sort_direction = sort_direction.toggle()
                            }
                            KeyCode::Char('/') => filter_mode = true,
                            KeyCode::Char(' ') => paused = !paused,
                            KeyCode::Enter => match active_panel {
                                ActivePanel::Pages => {
                                    let selected =
                                        page_table_state.selected().unwrap_or(selected_page_idx);
                                    if let Some(url) = page_view_urls.get(selected)
                                        && let Err(err) = open_url_in_browser(url)
                                    {
                                        state.push_error(format!(
                                            "failed to open link in browser: {err}"
                                        ));
                                    }
                                }
                                ActivePanel::Issues => {
                                    if issues_pane == IssuesPane::Urls {
                                        let selected = issue_page_table_state
                                            .selected()
                                            .unwrap_or(selected_issue_page_idx);
                                        if let Some(url) = issue_view_urls.get(selected)
                                            && let Err(err) = open_url_in_browser(url)
                                        {
                                            state.push_error(format!(
                                                "failed to open link in browser: {err}"
                                            ));
                                        }
                                    }
                                }
                                ActivePanel::Summary => {}
                            },
                            KeyCode::Up => match active_panel {
                                ActivePanel::Pages => {
                                    if pages_pane == PagesPane::Table {
                                        selected_page_idx = selected_page_idx.saturating_sub(1);
                                    }
                                }
                                ActivePanel::Issues => {
                                    if issues_pane == IssuesPane::Distribution {
                                        selected_issue_idx = selected_issue_idx.saturating_sub(1);
                                    } else {
                                        selected_issue_page_idx =
                                            selected_issue_page_idx.saturating_sub(1);
                                    }
                                }
                                ActivePanel::Summary => {}
                            },
                            KeyCode::Down => match active_panel {
                                ActivePanel::Pages => {
                                    if pages_pane == PagesPane::Table {
                                        selected_page_idx = selected_page_idx.saturating_add(1);
                                    }
                                }
                                ActivePanel::Issues => {
                                    if issues_pane == IssuesPane::Distribution {
                                        selected_issue_idx = selected_issue_idx.saturating_add(1);
                                    } else {
                                        selected_issue_page_idx =
                                            selected_issue_page_idx.saturating_add(1);
                                    }
                                }
                                ActivePanel::Summary => {}
                            },
                            _ => {}
                        }
                    }
                }
                Event::Mouse(mouse) => {
                    let modifier_held = mouse.modifiers.intersects(
                        KeyModifiers::SHIFT | KeyModifiers::CONTROL | KeyModifiers::ALT,
                    );
                    hovered_page_url_idx = None;
                    hovered_issue_url_idx = None;

                    if modifier_held {
                        if active_panel == ActivePanel::Pages
                            && let Some(area) = page_table_area
                            && let Some(row_idx) = table_row_index_at(area, mouse.row)
                            && row_idx < page_view_urls.len()
                            && point_in_rect(mouse.column, mouse.row, area)
                        {
                            hovered_page_url_idx = Some(row_idx);
                        }
                        if active_panel == ActivePanel::Issues
                            && let Some(area) = issue_urls_area
                            && let Some(row_idx) = table_row_index_at(area, mouse.row)
                            && row_idx < issue_view_urls.len()
                            && point_in_rect(mouse.column, mouse.row, area)
                        {
                            hovered_issue_url_idx = Some(row_idx);
                        }
                    }

                    if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
                        match active_panel {
                            ActivePanel::Pages => {
                                if let Some(area) = page_table_area
                                    && let Some(row_idx) = table_row_index_at(area, mouse.row)
                                    && row_idx < page_view_urls.len()
                                    && point_in_rect(mouse.column, mouse.row, area)
                                {
                                    selected_page_idx = row_idx;
                                    page_table_state.select(Some(selected_page_idx));
                                    let now = Instant::now();
                                    let double_click = last_page_click
                                        .map(|(prev_idx, prev_time)| {
                                            prev_idx == selected_page_idx
                                                && now.duration_since(prev_time)
                                                    <= Duration::from_millis(450)
                                        })
                                        .unwrap_or(false);
                                    if (double_click || modifier_held)
                                        && let Some(url) = page_view_urls.get(selected_page_idx)
                                        && let Err(err) = open_url_in_browser(url)
                                    {
                                        state.push_error(format!(
                                            "failed to open link in browser: {err}"
                                        ));
                                    }
                                    last_page_click = Some((selected_page_idx, now));
                                }
                            }
                            ActivePanel::Issues => {
                                if let Some(area) = issue_distribution_area
                                    && let Some(row_idx) = table_row_index_at(area, mouse.row)
                                    && point_in_rect(mouse.column, mouse.row, area)
                                {
                                    selected_issue_idx = row_idx;
                                    issue_table_state.select(Some(selected_issue_idx));
                                    issues_pane = IssuesPane::Distribution;
                                }
                                if let Some(area) = issue_urls_area
                                    && let Some(row_idx) = table_row_index_at(area, mouse.row)
                                    && row_idx < issue_view_urls.len()
                                    && point_in_rect(mouse.column, mouse.row, area)
                                {
                                    selected_issue_page_idx = row_idx;
                                    issue_page_table_state.select(Some(selected_issue_page_idx));
                                    issues_pane = IssuesPane::Urls;
                                    let now = Instant::now();
                                    let double_click = last_issue_url_click
                                        .map(|(prev_idx, prev_time)| {
                                            prev_idx == selected_issue_page_idx
                                                && now.duration_since(prev_time)
                                                    <= Duration::from_millis(450)
                                        })
                                        .unwrap_or(false);
                                    if (double_click || modifier_held)
                                        && let Some(url) =
                                            issue_view_urls.get(selected_issue_page_idx)
                                        && let Err(err) = open_url_in_browser(url)
                                    {
                                        state.push_error(format!(
                                            "failed to open link in browser: {err}"
                                        ));
                                    }
                                    last_issue_url_click = Some((selected_issue_page_idx, now));
                                }
                            }
                            ActivePanel::Summary => {}
                        }
                    }
                }
                _ => {}
            }
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }

        if state.done && auto_close {
            break;
        }
    }

    if let Some(sink) = sink.as_mut() {
        sink.finalize()?;
    }
    Ok(())
}

async fn run_crawler(cli: Cli, tx: UnboundedSender<CrawlEvent>) {
    let Some(start_url) = cli.url.clone() else {
        let _ = tx.send(CrawlEvent::Error("missing URL".to_string()));
        let _ = tx.send(CrawlEvent::Finished);
        return;
    };

    let mut website = Website::new(&start_url);
    let root_host = Url::parse(&start_url)
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
        match prepare_webdriver_backend(&cli, &start_url, &webdriver_url, &tx).await {
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
                        url: start_url.clone(),
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
            &start_url,
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
                        url: start_url.clone(),
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
                        url: start_url.clone(),
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
                let (mut row, discovered_links) = page_to_row(&page, root_host.as_deref());
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
            candidate_urls.push(start_url.clone());

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
                let (mut row, _) = page_to_row(&page, root_host);
                row.url = rendered_url;
                apply_rendered_html_to_row(&mut row, &rendered_html, root_host);
                row.internal_link_count = filtered.len();
                row.link_count = row.internal_link_count + row.external_link_count;
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
        let (mut row, discovered_links) = page_to_row(&page, root_host_ref);
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
            internal_link_count: 1,
            external_link_count: 0,
            h1_count: 0,
            h2_count: 0,
            image_count: 0,
            image_missing_alt_count: 0,
            structured_data_count: 0,
            seo_score: 100,
            issues: Vec::new(),
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

fn page_to_row(page: &spider::page::Page, root_host: Option<&str>) -> (CrawlRow, Vec<String>) {
    let html = page.get_html();
    let doc = Html::parse_document(&html);

    let mut title = extract_title(&doc);
    let meta = extract_meta_description(&doc);
    let h1 = extract_real_h1(&doc);
    let mime = header_value(page, "content-type")
        .map(|v| v.split(';').next().unwrap_or("").trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| infer_mime_from_page(page));
    let status = page.status_code.as_u16();

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

    if title.is_empty() {
        title = h1.clone();
    }
    let canonical = extract_canonical(&doc, &row_url);
    let noindex = has_noindex(&doc, page);
    let is_html = mime.to_ascii_lowercase().contains("html");
    let h1_count = if is_html {
        count_elements(&doc, "h1")
    } else {
        0
    };
    let h2_count = if is_html {
        count_elements(&doc, "h2")
    } else {
        0
    };
    let (image_count, image_missing_alt_count) = if is_html {
        image_alt_stats(&doc)
    } else {
        (0, 0)
    };
    let structured_data_count = if is_html {
        count_structured_data_blocks(&doc)
    } else {
        0
    };
    let word_count = if is_html { count_words(&doc) } else { 0 };
    let (doc_links, internal_link_count, external_link_count) = if is_html {
        extract_crawl_links_with_breakdown(&doc, &row_url, root_host)
    } else {
        (Vec::new(), 0, 0)
    };

    let size = page.get_html_bytes_u8().len();
    let response_time = page.get_duration_elapsed().as_millis();
    let last_modified = header_value(page, "last-modified").unwrap_or_default();

    let mut discovered_links = page
        .page_links
        .as_ref()
        .map(|links| links.iter().map(ToString::to_string).collect::<Vec<_>>())
        .unwrap_or_default();
    discovered_links.extend(doc_links);
    let mut discovered_dedupe = HashSet::new();
    discovered_links.retain(|link| discovered_dedupe.insert(link.clone()));

    let issues = collect_row_issues(
        status,
        "retrieved",
        is_html,
        noindex,
        title.chars().count(),
        meta.chars().count(),
        h1_count,
        &canonical,
        word_count,
        image_missing_alt_count,
        external_link_count,
    );
    let seo_score = compute_seo_score(&issues);
    let indexability = if (200..=299).contains(&status) && !noindex {
        "Indexable".to_string()
    } else {
        "Non-Indexable".to_string()
    };

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
            internal_link_count,
            external_link_count,
            h1_count,
            h2_count,
            image_count,
            image_missing_alt_count,
            structured_data_count,
            seo_score,
            issues,
            crawl_timestamp: Utc::now().to_rfc3339(),
        },
        discovered_links,
    )
}

fn apply_rendered_html_to_row(row: &mut CrawlRow, html: &str, root_host: Option<&str>) {
    let doc = Html::parse_document(html);
    let title = extract_title(&doc);
    let meta = extract_meta_description(&doc);
    let h1 = extract_real_h1(&doc);
    let canonical = extract_canonical(&doc, &row.url);
    let noindex = has_noindex_meta(&doc);
    let h1_count = count_elements(&doc, "h1");
    let h2_count = count_elements(&doc, "h2");
    let (image_count, image_missing_alt_count) = image_alt_stats(&doc);
    let structured_data_count = count_structured_data_blocks(&doc);
    let (_, internal_link_count, external_link_count) =
        extract_crawl_links_with_breakdown(&doc, &row.url, root_host);
    let word_count = count_words(&doc);

    if !title.is_empty() {
        row.title = title;
        row.title_length = row.title.chars().count();
    }
    row.meta = meta;
    row.meta_length = row.meta.chars().count();
    row.h1 = h1;
    row.h1_count = h1_count;
    row.h2_count = h2_count;
    row.canonical = canonical;
    row.word_count = word_count;
    row.size = html.as_bytes().len();
    row.image_count = image_count;
    row.image_missing_alt_count = image_missing_alt_count;
    row.structured_data_count = structured_data_count;
    row.internal_link_count = internal_link_count;
    row.external_link_count = external_link_count;
    row.link_count = internal_link_count + external_link_count;
    row.mime = "text/html".to_string();
    row.indexability = if (200..=299).contains(&row.status) && !noindex {
        "Indexable".to_string()
    } else {
        "Non-Indexable".to_string()
    };
    row.issues = collect_row_issues(
        row.status,
        &row.retrieval_status,
        true,
        noindex,
        row.title_length,
        row.meta_length,
        row.h1_count,
        &row.canonical,
        row.word_count,
        row.image_missing_alt_count,
        row.external_link_count,
    );
    row.seo_score = compute_seo_score(&row.issues);
}

fn unretrieved_row(url: String, reason: String) -> CrawlRow {
    let reason_len = reason.chars().count();
    let issues = vec![SeoIssue::NotRetrieved];
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
        internal_link_count: 0,
        external_link_count: 0,
        h1_count: 0,
        h2_count: 0,
        image_count: 0,
        image_missing_alt_count: 0,
        structured_data_count: 0,
        seo_score: compute_seo_score(&issues),
        issues,
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

fn extract_canonical(doc: &Html, page_url: &str) -> String {
    let selector = match Selector::parse("link[rel=\"canonical\"]") {
        Ok(s) => s,
        Err(_) => return String::new(),
    };

    doc.select(&selector)
        .next()
        .and_then(|el| el.value().attr("href"))
        .map(normalize_text)
        .and_then(|href| resolve_href(page_url, &href).or(Some(href)))
        .unwrap_or_default()
}

fn extract_crawl_links_with_breakdown(
    doc: &Html,
    page_url: &str,
    root_host: Option<&str>,
) -> (Vec<String>, usize, usize) {
    let selector =
        match Selector::parse("link[rel=\"alternate\"][href], link[hreflang][href], a[href]") {
            Ok(s) => s,
            Err(_) => return (Vec::new(), 0, 0),
        };

    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut internal_count = 0usize;
    let mut external_count = 0usize;
    for el in doc.select(&selector) {
        if let Some(href) = el.value().attr("href") {
            let href = href.trim();
            let Some(resolved) = resolve_href(page_url, href) else {
                continue;
            };
            if is_same_host(&resolved, root_host) {
                internal_count += 1;
            } else {
                external_count += 1;
            }
            if seen.insert(resolved.clone()) {
                out.push(resolved);
            }
        }
    }
    (out, internal_count, external_count)
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

fn resolve_href(page_url: &str, href: &str) -> Option<String> {
    if href.is_empty()
        || href.starts_with('#')
        || href.starts_with("mailto:")
        || href.starts_with("javascript:")
        || href.starts_with("tel:")
    {
        return None;
    }

    let resolved = if href.starts_with("http://") || href.starts_with("https://") {
        href.to_string()
    } else {
        let base = Url::parse(page_url).ok()?;
        base.join(href).ok()?.to_string()
    };
    normalize_crawl_url(&resolved)
}

fn count_elements(doc: &Html, selector: &str) -> usize {
    Selector::parse(selector)
        .ok()
        .map(|sel| doc.select(&sel).count())
        .unwrap_or(0)
}

fn image_alt_stats(doc: &Html) -> (usize, usize) {
    let selector = match Selector::parse("img") {
        Ok(sel) => sel,
        Err(_) => return (0, 0),
    };
    let mut total = 0usize;
    let mut missing_alt = 0usize;
    for el in doc.select(&selector) {
        total += 1;
        let alt = el.value().attr("alt").unwrap_or_default().trim();
        if alt.is_empty() {
            missing_alt += 1;
        }
    }
    (total, missing_alt)
}

fn count_structured_data_blocks(doc: &Html) -> usize {
    let selector = match Selector::parse("script[type=\"application/ld+json\"]") {
        Ok(sel) => sel,
        Err(_) => return 0,
    };
    doc.select(&selector)
        .filter(|el| !normalize_text(&el.text().collect::<Vec<_>>().join(" ")).is_empty())
        .count()
}

fn collect_row_issues(
    status: u16,
    retrieval_status: &str,
    is_html: bool,
    noindex: bool,
    title_length: usize,
    meta_length: usize,
    h1_count: usize,
    canonical: &str,
    word_count: usize,
    image_missing_alt_count: usize,
    external_link_count: usize,
) -> Vec<SeoIssue> {
    let mut issues = Vec::new();
    if retrieval_status != "retrieved" {
        issues.push(SeoIssue::NotRetrieved);
        return issues;
    }
    if (400..=499).contains(&status) {
        issues.push(SeoIssue::Http4xx);
    }
    if (500..=599).contains(&status) {
        issues.push(SeoIssue::Http5xx);
    }

    if !is_html || !(200..=299).contains(&status) {
        return issues;
    }

    if noindex {
        issues.push(SeoIssue::Noindex);
    }
    if title_length == 0 {
        issues.push(SeoIssue::MissingTitle);
    } else if title_length < 15 {
        issues.push(SeoIssue::TitleTooShort);
    } else if title_length > 60 {
        issues.push(SeoIssue::TitleTooLong);
    }

    if meta_length == 0 {
        issues.push(SeoIssue::MissingMetaDescription);
    } else if meta_length < 70 {
        issues.push(SeoIssue::MetaDescriptionTooShort);
    } else if meta_length > 160 {
        issues.push(SeoIssue::MetaDescriptionTooLong);
    }

    if h1_count == 0 {
        issues.push(SeoIssue::MissingH1);
    } else if h1_count > 1 {
        issues.push(SeoIssue::MultipleH1);
    }

    if canonical.trim().is_empty() {
        issues.push(SeoIssue::MissingCanonical);
    }

    if word_count < 120 {
        issues.push(SeoIssue::LowWordCount);
    }

    if image_missing_alt_count > 0 {
        issues.push(SeoIssue::ImagesMissingAlt);
    }

    if external_link_count > 60 {
        issues.push(SeoIssue::TooManyExternalLinks);
    }

    issues
}

fn compute_seo_score(issues: &[SeoIssue]) -> u8 {
    let penalty = issues
        .iter()
        .map(|issue| issue.penalty() as u16)
        .sum::<u16>();
    (100u16.saturating_sub(penalty)) as u8
}

fn point_in_rect(x: u16, y: u16, rect: Rect) -> bool {
    let right = rect.x.saturating_add(rect.width);
    let bottom = rect.y.saturating_add(rect.height);
    x >= rect.x && x < right && y >= rect.y && y < bottom
}

fn table_row_index_at(area: Rect, mouse_row: u16) -> Option<usize> {
    if area.height <= 3 {
        return None;
    }
    let first_data_row = area.y.saturating_add(2);
    let last_data_row = area.y + area.height - 1;
    if mouse_row >= first_data_row && mouse_row < last_data_row {
        Some((mouse_row - first_data_row) as usize)
    } else {
        None
    }
}

fn open_url_in_browser(url: &str) -> Result<(), String> {
    if url.trim().is_empty() {
        return Err("empty URL".to_string());
    }

    #[cfg(target_os = "macos")]
    let mut command = {
        let mut cmd = Command::new("open");
        cmd.arg(url);
        cmd
    };

    #[cfg(target_os = "windows")]
    let mut command = {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "start", "", url]);
        cmd
    };

    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let mut command = {
        let mut cmd = Command::new("xdg-open");
        cmd.arg(url);
        cmd
    };

    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| err.to_string())?;

    Ok(())
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

fn seo_score_style(score: u8) -> Style {
    match score {
        85..=100 => Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
        70..=84 => Style::default().fg(Color::LightGreen),
        50..=69 => Style::default().fg(Color::Yellow),
        _ => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
    }
}
