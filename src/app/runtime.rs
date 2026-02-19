fn current_fetch_concurrency(concurrency: &Arc<AtomicUsize>) -> usize {
    sanitize_fetch_concurrency(concurrency.load(Ordering::Relaxed))
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
        CrawlEvent::Status(message) => state.push_status(message),
        CrawlEvent::Error(err) => state.push_error(err),
    }

    Ok(())
}

pub async fn run() -> io::Result<()> {
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
        return run_tui(&review_file, None, 1, None, auto_close, &mut rx);
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
    let initial_fetch_concurrency = sanitize_fetch_concurrency(cli.fetch_concurrency);
    let (control_tx, control_rx) = mpsc::unbounded_channel::<CrawlControl>();
    let crawl_handle = tokio::spawn(run_crawler(cli, tx, control_rx));
    let tui_result = if no_tui {
        drop(control_tx);
        run_headless(&output_path, output_format, &mut rx)
    } else {
        run_tui(
            &output_path,
            Some(control_tx),
            initial_fetch_concurrency,
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
            match &event {
                CrawlEvent::Status(message) => eprintln!("{message}"),
                CrawlEvent::Error(err) => eprintln!("{err}"),
                _ => {}
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
            match &event {
                CrawlEvent::Status(message) => eprintln!("{message}"),
                CrawlEvent::Error(err) => eprintln!("{err}"),
                _ => {}
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
    control_tx: Option<UnboundedSender<CrawlControl>>,
    initial_fetch_concurrency: usize,
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

    let tui_result = draw_loop(
        &mut terminal,
        session_label,
        control_tx,
        initial_fetch_concurrency,
        output_target,
        auto_close,
        rx,
    );

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    tui_result
}
