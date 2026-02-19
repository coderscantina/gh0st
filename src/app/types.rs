#[derive(Debug, Parser, Clone)]
#[command(
    name = "gh0st",
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

    #[arg(long, value_name = "N", default_value_t = 2)]
    retry_5xx: usize,

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
}

impl ActivePanel {
    fn as_index(self) -> usize {
        match self {
            ActivePanel::Pages => 0,
            ActivePanel::Issues => 1,
        }
    }

    fn title(self) -> &'static str {
        match self {
            ActivePanel::Pages => "Pages",
            ActivePanel::Issues => "Issues",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageSortMode {
    Latest,
    Status,
    LowestSeoScore,
    HighestResponseTime,
}

impl PageSortMode {
    fn cycle(self) -> Self {
        match self {
            PageSortMode::Latest => PageSortMode::Status,
            PageSortMode::Status => PageSortMode::LowestSeoScore,
            PageSortMode::LowestSeoScore => PageSortMode::HighestResponseTime,
            PageSortMode::HighestResponseTime => PageSortMode::Latest,
        }
    }

    fn title(self) -> &'static str {
        match self {
            PageSortMode::Latest => "latest",
            PageSortMode::Status => "status",
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
    Status(String),
    Error(String),
}

#[derive(Debug)]
enum CrawlControl {
    SetFetchConcurrency(usize),
    RetryUrls {
        scope: RetryScope,
        urls: Vec<String>,
    },
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetryScope {
    SingleEntry,
    FailedOnly,
    Complete,
}

impl RetryScope {
    fn label(self) -> &'static str {
        match self {
            RetryScope::SingleEntry => "single_entry",
            RetryScope::FailedOnly => "failed_only",
            RetryScope::Complete => "complete",
        }
    }
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
    status_messages: VecDeque<String>,
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

    fn push_status(&mut self, message: String) {
        self.status_messages.push_front(message);
        while self.status_messages.len() > 20 {
            self.status_messages.pop_back();
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
    ) -> Vec<&'a CrawlRow> {
        let filter = filter.trim().to_ascii_lowercase();
        let mut rows = self
            .all_rows
            .iter()
            .rev()
            .filter(|row| filter.is_empty() || row_matches_filter_query(row, &filter))
            .collect::<Vec<_>>();

        match sort {
            PageSortMode::Latest => {
                if direction == SortDirection::Asc {
                    rows.reverse();
                }
            }
            PageSortMode::Status => {
                rows.sort_by(|a, b| a.status.cmp(&b.status).then(a.url.cmp(&b.url)));
                if direction == SortDirection::Desc {
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

        rows
    }

    fn retry_failed_urls(&self) -> Vec<String> {
        let mut urls = self
            .all_rows
            .iter()
            .filter(|row| row.retrieval_status != "retrieved" || (500..=599).contains(&row.status))
            .map(|row| row.url.clone())
            .collect::<Vec<_>>();
        urls.sort();
        urls.dedup();
        urls
    }

    fn retry_all_urls(&self) -> Vec<String> {
        let mut urls = self
            .all_rows
            .iter()
            .map(|row| row.url.clone())
            .collect::<Vec<_>>();
        urls.extend(self.discovered_seen.iter().cloned());
        urls.sort();
        urls.dedup();
        urls
    }
}

fn matches_status_filter(row: &CrawlRow, filter: &str) -> bool {
    let mut q = filter.trim().to_ascii_lowercase();
    if q.is_empty() {
        return false;
    }

    if let Some(rest) = q.strip_prefix("status:") {
        q = rest.trim().to_string();
    } else if let Some(rest) = q.strip_prefix("status=") {
        q = rest.trim().to_string();
    }

    match q.as_str() {
        "0" | "0xx" => row.status == 0,
        "2xx" | "2" => (200..=299).contains(&row.status),
        "3xx" | "3" => (300..=399).contains(&row.status),
        "4xx" | "4" => (400..=499).contains(&row.status),
        "5xx" | "5" => (500..=599).contains(&row.status),
        "retrieved" => row.retrieval_status == "retrieved",
        "not_retrieved" | "unretrieved" | "failed" => {
            row.retrieval_status != "retrieved" || row.status == 0
        }
        _ => q
            .parse::<u16>()
            .map(|code| row.status == code)
            .unwrap_or(false),
    }
}

fn row_matches_filter_query(row: &CrawlRow, filter: &str) -> bool {
    let url = row.url.to_ascii_lowercase();
    let title = row.title.to_ascii_lowercase();
    let meta = row.meta.to_ascii_lowercase();

    // Backward-compatible whole-query matching.
    if row_matches_token(row, filter, &url, &title, &meta) {
        return true;
    }

    // Advanced mode: split query into terms and require all positive terms to match.
    let mut saw_term = false;
    for raw in filter.split_whitespace() {
        if raw.is_empty() {
            continue;
        }
        saw_term = true;
        let (negated, token) = if let Some(rest) = raw.strip_prefix('!') {
            (true, rest)
        } else if let Some(rest) = raw.strip_prefix('-') {
            (true, rest)
        } else {
            (false, raw)
        };
        if token.is_empty() {
            continue;
        }
        let matched = row_matches_token(row, token, &url, &title, &meta);
        if negated {
            if matched {
                return false;
            }
        } else if !matched {
            return false;
        }
    }

    saw_term
}

fn row_matches_token(row: &CrawlRow, token: &str, url: &str, title: &str, meta: &str) -> bool {
    if token.is_empty() {
        return true;
    }

    if let Some((key, value)) = token.split_once(':') {
        let value = value.trim();
        if value.is_empty() {
            return false;
        }
        return match key {
            "status" => matches_status_filter(row, value),
            "issue" | "issues" => {
                if value == "none" {
                    row.issues.is_empty()
                } else {
                    row.issues.iter().any(|issue| issue.label().contains(value))
                }
            }
            "url" => url.contains(value),
            "title" => title.contains(value),
            "meta" => meta.contains(value),
            "host" => row_host_contains(&row.url, value),
            "retrieval" => row.retrieval_status.to_ascii_lowercase().contains(value),
            _ => false,
        };
    }

    url.contains(token)
        || title.contains(token)
        || meta.contains(token)
        || matches_status_filter(row, token)
        || row.issues.iter().any(|issue| issue.label().contains(token))
}

fn row_host_contains(url: &str, host_filter: &str) -> bool {
    Url::parse(url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(|host| host.to_ascii_lowercase()))
        .map(|host| host.contains(host_filter))
        .unwrap_or(false)
}
