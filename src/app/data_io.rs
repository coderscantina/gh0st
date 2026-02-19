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

const MAX_FETCH_CONCURRENCY: usize = 256;

fn sanitize_fetch_concurrency(value: usize) -> usize {
    value.max(1).min(MAX_FETCH_CONCURRENCY)
}

