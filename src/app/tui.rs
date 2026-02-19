const PAGE_JUMP_STEP: usize = 10;

fn draw_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    session_label_input: &str,
    control_tx: Option<UnboundedSender<CrawlControl>>,
    initial_fetch_concurrency: usize,
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
    let mut help_mode = false;
    let mut retry_prompt_mode = false;
    let mut retry_scope_selection = RetryScope::FailedOnly;
    let mut fetch_concurrency = sanitize_fetch_concurrency(initial_fetch_concurrency);
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
                        .fg(if state.done {
                            Color::Green
                        } else {
                            Color::Cyan
                        })
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

                    let page_rows = state.filtered_rows_sorted(&filter, sort_mode, sort_direction);
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
            }

            let error_count = state.errors.len();
            let status_count = state.status_messages.len();
            let (last_event_label, last_event, last_event_style) = if let Some(err) =
                state.errors.front()
            {
                (
                    "LAST ERROR",
                    truncate_for_log(err, 170),
                    Style::default().fg(Color::LightRed),
                )
            } else if let Some(message) = state.status_messages.front() {
                (
                    "LAST STATUS",
                    truncate_for_log(message, 170),
                    Style::default().fg(Color::Cyan),
                )
            } else {
                (
                    "LAST STATUS",
                    "none".to_string(),
                    Style::default().fg(Color::DarkGray),
                )
            };
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
            } else if help_mode || filter_mode || retry_prompt_mode {
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
            } else if help_mode {
                "HELP"
            } else if retry_prompt_mode {
                "RETRY PROMPT"
            } else {
                "NAVIGATION"
            };
            let pane_label = match active_panel {
                ActivePanel::Pages => pages_pane.label(),
                ActivePanel::Issues => issues_pane.label(),
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
                    Span::styled("   FETCH ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        fetch_concurrency.to_string(),
                        Style::default().fg(if control_tx.is_some() {
                            Color::LightCyan
                        } else {
                            Color::DarkGray
                        }),
                    ),
                    Span::styled("   RETRY ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        if retry_prompt_mode {
                            retry_scope_selection.label()
                        } else {
                            "-"
                        },
                        Style::default().fg(if retry_prompt_mode {
                            Color::Yellow
                        } else {
                            Color::DarkGray
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
                    Span::styled("   STATUS ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        status_count.to_string(),
                        Style::default().fg(if status_count > 0 {
                            Color::Cyan
                        } else {
                            Color::Green
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
                    Span::styled(format!("{last_event_label} "), Style::default().fg(Color::DarkGray)),
                    Span::styled(last_event, last_event_style),
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
                        "P I",
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
                        "R",
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" retry prompt  ", Style::default().fg(Color::Gray)),
                    Span::styled(
                        "t",
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" retry selected  ", Style::default().fg(Color::Gray)),
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
                        "?",
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" help  ", Style::default().fg(Color::Gray)),
                    Span::styled(
                        "j/k",
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" move  ", Style::default().fg(Color::Gray)),
                    Span::styled(
                        "pgup/pgdn",
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" jump  ", Style::default().fg(Color::Gray)),
                    Span::styled(
                        "g/G",
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" top/bot  ", Style::default().fg(Color::Gray)),
                    Span::styled(
                        "+/-",
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" concurrency  ", Style::default().fg(Color::Gray)),
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
                    Span::styled(" close modal", Style::default().fg(Color::Gray)),
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

            if help_mode {
                let area = centered_rect(72, 54, f.area());
                f.render_widget(Clear, area);
                f.render_widget(
                    Block::default()
                        .title("Help")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Yellow)),
                    area,
                );
                let help_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(8),
                        Constraint::Length(7),
                        Constraint::Length(8),
                        Constraint::Min(3),
                    ])
                    .split(area);
                f.render_widget(
                    Paragraph::new(vec![
                        Line::from("Navigation"),
                        Line::from("  up/down or j/k: move selection"),
                        Line::from("  pgup/pgdn: jump by 10 rows"),
                        Line::from("  g/G or home/end: first/last row"),
                        Line::from("  tab / shift+tab: switch pane focus"),
                        Line::from("  p / i: switch panel"),
                        Line::from("  enter: open selected URL"),
                    ])
                    .block(Block::default().borders(Borders::ALL).title("Keys"))
                    .wrap(Wrap { trim: true }),
                    help_chunks[0],
                );
                f.render_widget(
                    Paragraph::new(vec![
                        Line::from("Operations"),
                        Line::from("  r: cycle sort mode, d: sort direction"),
                        Line::from("  /: edit filter, ctrl+u: clear filter input"),
                        Line::from("  +/-: adjust fetch concurrency"),
                        Line::from("  space: pause/resume live updates"),
                        Line::from("  t: retry selected URL, R: retry prompt"),
                    ])
                    .block(Block::default().borders(Borders::ALL).title("Actions"))
                    .wrap(Wrap { trim: true }),
                    help_chunks[1],
                );
                f.render_widget(
                    Paragraph::new(vec![
                        Line::from("Filter syntax"),
                        Line::from("  Free text: home pricing"),
                        Line::from("  status:4xx  status:retrieved  status:404"),
                        Line::from("  issue:missing_h1  issue:none"),
                        Line::from("  host:example.com  title:blog  url:/pricing"),
                        Line::from("  Negate terms with ! or - (example: status:4xx -issue:noindex)"),
                    ])
                    .block(Block::default().borders(Borders::ALL).title("Filter Query"))
                    .wrap(Wrap { trim: true }),
                    help_chunks[2],
                );
                f.render_widget(
                    Paragraph::new("Press ? or Esc to close.")
                        .block(Block::default().borders(Borders::ALL).title("Close"))
                        .wrap(Wrap { trim: true }),
                    help_chunks[3],
                );
            } else if retry_prompt_mode {
                let area = centered_rect(62, 36, f.area());
                let prompt_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(3),
                        Constraint::Length(6),
                        Constraint::Length(3),
                    ])
                    .split(area);
                let failed_count = state.retry_failed_urls().len();
                let complete_count = state.retry_all_urls().len();
                let failed_style = if retry_scope_selection == RetryScope::FailedOnly {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                let complete_style = if retry_scope_selection == RetryScope::Complete {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                f.render_widget(Clear, area);
                f.render_widget(
                    Block::default()
                        .title("Retry / Refresh Scope")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Yellow)),
                    area,
                );
                f.render_widget(
                    Paragraph::new("Choose what to refresh and press Enter.")
                        .block(Block::default().borders(Borders::NONE)),
                    prompt_chunks[0],
                );
                f.render_widget(
                    Paragraph::new(vec![
                        Line::from(vec![
                            Span::styled("Failed only", failed_style),
                            Span::raw(format!("  ({failed_count} URLs)")),
                        ]),
                        Line::from("status 5xx or not_retrieved"),
                        Line::from(""),
                        Line::from(vec![
                            Span::styled("Complete", complete_style),
                            Span::raw(format!("  ({complete_count} URLs)")),
                        ]),
                        Line::from("all crawled + discovered internal URLs"),
                    ])
                    .block(Block::default().borders(Borders::ALL).title("Options"))
                    .wrap(Wrap { trim: true }),
                    prompt_chunks[1],
                );
                f.render_widget(
                    Paragraph::new(
                        "Up/Down to change scope, Enter to queue refresh, Esc to cancel.",
                    )
                    .block(Block::default().borders(Borders::NONE))
                    .wrap(Wrap { trim: true }),
                    prompt_chunks[2],
                );
            } else if filter_mode {
                let area = centered_rect(72, 28, f.area());
                f.render_widget(Clear, area);
                f.render_widget(
                    Block::default()
                        .title("Filter")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Yellow)),
                    area,
                );
                let prompt_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(3),
                        Constraint::Length(4),
                        Constraint::Min(3),
                    ])
                    .split(area);
                f.render_widget(
                    Paragraph::new(format!(
                        "Query: {}",
                        if filter.is_empty() { "<empty>" } else { &filter }
                    ))
                    .block(Block::default().borders(Borders::ALL).title("Current"))
                    .wrap(Wrap { trim: true }),
                    prompt_chunks[0],
                );
                f.render_widget(
                    Paragraph::new("Enter/esc to apply. Backspace to edit. Ctrl+u to clear.")
                        .block(Block::default().borders(Borders::ALL).title("Input"))
                        .wrap(Wrap { trim: true }),
                    prompt_chunks[1],
                );
                f.render_widget(
                    Paragraph::new(
                        "Examples: status:4xx issue:missing_h1 host:example.com -issue:noindex",
                    )
                    .block(Block::default().borders(Borders::ALL).title("Examples"))
                    .wrap(Wrap { trim: true }),
                    prompt_chunks[2],
                );
            }
        })?;

        if let Some(sink) = sink.as_mut() {
            sink.flush()?;
        }

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) => {
                    if retry_prompt_mode {
                        match key.code {
                            KeyCode::Esc => retry_prompt_mode = false,
                            KeyCode::Up | KeyCode::Left => {
                                retry_scope_selection = RetryScope::FailedOnly;
                            }
                            KeyCode::Down | KeyCode::Right => {
                                retry_scope_selection = RetryScope::Complete;
                            }
                            KeyCode::Enter => {
                                if let Some(control_tx) = control_tx.as_ref() {
                                    let urls = match retry_scope_selection {
                                        RetryScope::SingleEntry => state.retry_failed_urls(),
                                        RetryScope::FailedOnly => state.retry_failed_urls(),
                                        RetryScope::Complete => state.retry_all_urls(),
                                    };
                                    if urls.is_empty() {
                                        state.push_error("no URLs available for retry".to_string());
                                    } else if control_tx
                                        .send(CrawlControl::RetryUrls {
                                            scope: retry_scope_selection,
                                            urls,
                                        })
                                        .is_err()
                                    {
                                        state.push_error(
                                            "crawler control channel is closed".to_string(),
                                        );
                                    } else {
                                        state.done = false;
                                        state.push_error(format!(
                                            "queued refresh: {}",
                                            retry_scope_selection.label()
                                        ));
                                        retry_prompt_mode = false;
                                    }
                                } else {
                                    retry_prompt_mode = false;
                                }
                            }
                            _ => {}
                        }
                    } else if help_mode {
                        match key.code {
                            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('?') => {
                                help_mode = false;
                            }
                            _ => {}
                        }
                    } else if filter_mode {
                        match key.code {
                            KeyCode::Esc | KeyCode::Enter => filter_mode = false,
                            KeyCode::Backspace => {
                                filter.pop();
                            }
                            KeyCode::Char('u')
                                if key.modifiers.contains(KeyModifiers::CONTROL) =>
                            {
                                filter.clear();
                            }
                            KeyCode::Char(ch) => {
                                if !key.modifiers.intersects(
                                    KeyModifiers::CONTROL | KeyModifiers::ALT,
                                ) {
                                    filter.push(ch);
                                }
                            }
                            _ => {}
                        }
                    } else {
                        match key.code {
                            KeyCode::Char('q') => {
                                if let Some(control_tx) = control_tx.as_ref() {
                                    let _ = control_tx.send(CrawlControl::Shutdown);
                                }
                                break;
                            }
                            KeyCode::Tab => match active_panel {
                                ActivePanel::Pages => pages_pane = pages_pane.cycle(),
                                ActivePanel::Issues => issues_pane = issues_pane.cycle(),
                            },
                            KeyCode::BackTab => match active_panel {
                                ActivePanel::Pages => pages_pane = pages_pane.reverse_cycle(),
                                ActivePanel::Issues => issues_pane = issues_pane.reverse_cycle(),
                            },
                            KeyCode::Char('p') | KeyCode::Char('P') => {
                                active_panel = ActivePanel::Pages
                            }
                            KeyCode::Char('i') | KeyCode::Char('I') => {
                                active_panel = ActivePanel::Issues
                            }
                            KeyCode::Char('r') => sort_mode = sort_mode.cycle(),
                            KeyCode::Char('R') => {
                                if control_tx.is_some() {
                                    retry_prompt_mode = true;
                                    filter_mode = false;
                                    retry_scope_selection = RetryScope::FailedOnly;
                                }
                            }
                            KeyCode::Char('t') | KeyCode::Char('T') => {
                                if let Some(control_tx) = control_tx.as_ref() {
                                    let selected_url = match active_panel {
                                        ActivePanel::Pages => {
                                            let selected = page_table_state
                                                .selected()
                                                .unwrap_or(selected_page_idx);
                                            page_view_urls.get(selected).cloned()
                                        }
                                        ActivePanel::Issues => {
                                            if issues_pane == IssuesPane::Urls {
                                                let selected = issue_page_table_state
                                                    .selected()
                                                    .unwrap_or(selected_issue_page_idx);
                                                issue_view_urls.get(selected).cloned()
                                            } else {
                                                None
                                            }
                                        }
                                    };

                                    if let Some(url) = selected_url {
                                        if control_tx
                                            .send(CrawlControl::RetryUrls {
                                                scope: RetryScope::SingleEntry,
                                                urls: vec![url.clone()],
                                            })
                                            .is_err()
                                        {
                                            state.push_error(
                                                "crawler control channel is closed".to_string(),
                                            );
                                        } else {
                                            state.done = false;
                                            state.push_error(format!(
                                                "queued refresh for selected URL: {}",
                                                url
                                            ));
                                        }
                                    } else {
                                        state.push_error(
                                            "no selected URL in current panel".to_string(),
                                        );
                                    }
                                }
                            }
                            KeyCode::Char('d') | KeyCode::Char('D') => {
                                sort_direction = sort_direction.toggle()
                            }
                            KeyCode::Char('/') => {
                                filter_mode = true;
                                help_mode = false;
                                retry_prompt_mode = false;
                            }
                            KeyCode::Char('?') => {
                                help_mode = true;
                                filter_mode = false;
                                retry_prompt_mode = false;
                            }
                            KeyCode::Char('+') | KeyCode::Char('=') => {
                                let next =
                                    sanitize_fetch_concurrency(fetch_concurrency.saturating_add(1));
                                if next != fetch_concurrency {
                                    fetch_concurrency = next;
                                    if let Some(control_tx) = control_tx.as_ref() {
                                        if control_tx
                                            .send(CrawlControl::SetFetchConcurrency(
                                                fetch_concurrency,
                                            ))
                                            .is_err()
                                        {
                                            state.push_error(
                                                "crawler control channel is closed".to_string(),
                                            );
                                        }
                                    }
                                }
                            }
                            KeyCode::Char('-') | KeyCode::Char('_') => {
                                let next =
                                    sanitize_fetch_concurrency(fetch_concurrency.saturating_sub(1));
                                if next != fetch_concurrency {
                                    fetch_concurrency = next;
                                    if let Some(control_tx) = control_tx.as_ref() {
                                        if control_tx
                                            .send(CrawlControl::SetFetchConcurrency(
                                                fetch_concurrency,
                                            ))
                                            .is_err()
                                        {
                                            state.push_error(
                                                "crawler control channel is closed".to_string(),
                                            );
                                        }
                                    }
                                }
                            }
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
                            },
                            KeyCode::Up | KeyCode::Char('k')
                                if key.modifiers == KeyModifiers::NONE =>
                            {
                                match active_panel {
                                    ActivePanel::Pages => {
                                        if pages_pane == PagesPane::Table {
                                            selected_page_idx = selected_page_idx.saturating_sub(1);
                                        }
                                    }
                                    ActivePanel::Issues => {
                                        if issues_pane == IssuesPane::Distribution {
                                            selected_issue_idx =
                                                selected_issue_idx.saturating_sub(1);
                                        } else {
                                            selected_issue_page_idx =
                                                selected_issue_page_idx.saturating_sub(1);
                                        }
                                    }
                                }
                            }
                            KeyCode::Down | KeyCode::Char('j')
                                if key.modifiers == KeyModifiers::NONE =>
                            {
                                match active_panel {
                                    ActivePanel::Pages => {
                                        if pages_pane == PagesPane::Table {
                                            selected_page_idx = selected_page_idx.saturating_add(1);
                                        }
                                    }
                                    ActivePanel::Issues => {
                                        if issues_pane == IssuesPane::Distribution {
                                            selected_issue_idx =
                                                selected_issue_idx.saturating_add(1);
                                        } else {
                                            selected_issue_page_idx =
                                                selected_issue_page_idx.saturating_add(1);
                                        }
                                    }
                                }
                            }
                            KeyCode::PageUp => match active_panel {
                                ActivePanel::Pages => {
                                    if pages_pane == PagesPane::Table {
                                        selected_page_idx =
                                            selected_page_idx.saturating_sub(PAGE_JUMP_STEP);
                                    }
                                }
                                ActivePanel::Issues => {
                                    if issues_pane == IssuesPane::Distribution {
                                        selected_issue_idx =
                                            selected_issue_idx.saturating_sub(PAGE_JUMP_STEP);
                                    } else {
                                        selected_issue_page_idx =
                                            selected_issue_page_idx
                                                .saturating_sub(PAGE_JUMP_STEP);
                                    }
                                }
                            },
                            KeyCode::PageDown => match active_panel {
                                ActivePanel::Pages => {
                                    if pages_pane == PagesPane::Table {
                                        selected_page_idx =
                                            selected_page_idx.saturating_add(PAGE_JUMP_STEP);
                                    }
                                }
                                ActivePanel::Issues => {
                                    if issues_pane == IssuesPane::Distribution {
                                        selected_issue_idx =
                                            selected_issue_idx.saturating_add(PAGE_JUMP_STEP);
                                    } else {
                                        selected_issue_page_idx =
                                            selected_issue_page_idx
                                                .saturating_add(PAGE_JUMP_STEP);
                                    }
                                }
                            },
                            KeyCode::Home | KeyCode::Char('g')
                                if key.modifiers == KeyModifiers::NONE =>
                            {
                                match active_panel {
                                    ActivePanel::Pages => {
                                        if pages_pane == PagesPane::Table {
                                            selected_page_idx = 0;
                                        }
                                    }
                                    ActivePanel::Issues => {
                                        if issues_pane == IssuesPane::Distribution {
                                            selected_issue_idx = 0;
                                        } else {
                                            selected_issue_page_idx = 0;
                                        }
                                    }
                                }
                            }
                            KeyCode::End | KeyCode::Char('G') => match active_panel {
                                ActivePanel::Pages => {
                                    if pages_pane == PagesPane::Table {
                                        selected_page_idx = page_view_urls.len().saturating_sub(1);
                                    }
                                }
                                ActivePanel::Issues => {
                                    if issues_pane == IssuesPane::Distribution {
                                        let issue_count = state.top_issues(30).len() + 1;
                                        selected_issue_idx = issue_count.saturating_sub(1);
                                    } else {
                                        selected_issue_page_idx =
                                            issue_view_urls.len().saturating_sub(1);
                                    }
                                }
                            },
                            _ => {}
                        }
                    }
                }
                Event::Mouse(mouse) => {
                    if retry_prompt_mode || help_mode || filter_mode {
                        continue;
                    }
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
