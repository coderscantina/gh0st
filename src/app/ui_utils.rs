fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
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
