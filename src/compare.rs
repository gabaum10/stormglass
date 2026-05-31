use crate::metrics::SessionSummary;
use crate::output::commafy;
use crate::parse::parse_session;

/// Parse each path into a SessionSummary, then format side-by-side or JSON.
pub fn compare_sessions(paths: &[String], json: bool) {
    let mut summaries: Vec<SessionSummary> = Vec::new();
    for path in paths {
        match parse_session(path, None) {
            Ok(result) => summaries.push(result.summary),
            Err(e) => {
                eprintln!("{}: {}", path, e);
                std::process::exit(1);
            }
        }
    }

    if json {
        match serde_json::to_string_pretty(&summaries) {
            Ok(s) => println!("{}", s),
            Err(e) => eprintln!("JSON serialization error: {}", e),
        }
        return;
    }

    print_comparison_table(paths, &summaries);
}

fn print_comparison_table(paths: &[String], summaries: &[SessionSummary]) {
    // Column headers: basename + first 8 of session_id
    let headers: Vec<String> = paths
        .iter()
        .zip(summaries.iter())
        .map(|(path, s)| {
            let basename = std::path::Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(path.as_str());
            if s.session_id.is_empty() {
                basename.to_string()
            } else {
                let short = if s.session_id.len() > 8 {
                    &s.session_id[..8]
                } else {
                    &s.session_id
                };
                format!("{} ({})", basename, short)
            }
        })
        .collect();

    // Rows: label + values per session
    struct Row {
        label: &'static str,
        values: Vec<String>,
    }

    let rows: Vec<Row> = vec![
        Row {
            label: "Model",
            values: summaries.iter().map(|s| s.model.clone()).collect(),
        },
        Row {
            label: "Duration",
            values: summaries.iter().map(|s| format_duration(s.duration_sec)).collect(),
        },
        Row {
            label: "Turns",
            values: summaries.iter().map(|s| s.total_turns.to_string()).collect(),
        },
        Row {
            label: "Total input",
            values: summaries.iter().map(|s| commafy(s.total_input_tokens)).collect(),
        },
        Row {
            label: "Total output",
            values: summaries.iter().map(|s| commafy(s.total_output_tokens)).collect(),
        },
        Row {
            label: "Cache read",
            values: summaries.iter().map(|s| commafy(s.total_cache_read)).collect(),
        },
        Row {
            label: "Cache write",
            values: summaries.iter().map(|s| commafy(s.total_cache_write)).collect(),
        },
        Row {
            label: "Avg burn/turn",
            values: summaries.iter().map(|s| commafy(s.avg_burn_rate.max(0.0) as u64)).collect(),
        },
        Row {
            label: "Peak burn",
            values: summaries
                .iter()
                .map(|s| format!("{} (t{})", commafy(s.peak_burn_value.max(0) as u64), s.peak_burn_turn))
                .collect(),
        },
        Row {
            label: "Final context",
            values: summaries.iter().map(|s| commafy(s.final_context_size)).collect(),
        },
        Row {
            label: "Thinking ratio",
            values: summaries.iter().map(|s| format!("{:.1}%", s.thinking_ratio * 100.0)).collect(),
        },
        Row {
            label: "Subagent tokens",
            values: summaries.iter().map(|s| commafy(s.total_subagent_tokens)).collect(),
        },
        Row {
            label: "Subagent tasks",
            values: summaries.iter().map(|s| s.subagent_count.to_string()).collect(),
        },
    ];

    // Compute column widths
    let label_width = rows.iter().map(|r| r.label.len()).max().unwrap_or(0);
    let col_widths: Vec<usize> = (0..summaries.len())
        .map(|i| {
            let header_w = headers[i].len();
            let max_val_w = rows.iter().map(|r| r.values[i].len()).max().unwrap_or(0);
            header_w.max(max_val_w)
        })
        .collect();

    // Header row
    let mut header_line = format!("{:width$}", "", width = label_width + 2);
    for (i, h) in headers.iter().enumerate() {
        header_line.push_str(&format!("  {:width$}", h, width = col_widths[i]));
    }
    println!("{}", header_line);

    // Separator
    let sep_len = label_width + 2 + col_widths.iter().map(|w| w + 2).sum::<usize>();
    println!("{}", "-".repeat(sep_len));

    // Data rows
    for row in &rows {
        let mut line = format!("{:width$}  ", row.label, width = label_width);
        for (i, val) in row.values.iter().enumerate() {
            line.push_str(&format!("{:>width$}  ", val, width = col_widths[i]));
        }
        println!("{}", line.trim_end());
    }
}

fn format_duration(sec: f64) -> String {
    let total_sec = sec.max(0.0).round() as u64;
    if total_sec >= 3600 {
        let h = total_sec / 3600;
        let m = (total_sec % 3600) / 60;
        format!("{}h {}m", h, m)
    } else {
        let m = total_sec / 60;
        let s = total_sec % 60;
        format!("{}m {}s", m, s)
    }
}
