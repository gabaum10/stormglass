use std::io::{self, Write};
use crate::metrics::{SessionSummary, Turn};

// Insert comma thousands separators. No external dep.
pub fn commafy(n: u64) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
}

// Format duration in seconds as "Xh Ym" or "Ym Zs" (< 1h).
pub fn format_duration(secs: f64) -> String {
    let total_secs = secs.max(0.0) as u64;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs_rem = total_secs % 60;
    if hours > 0 {
        format!("{}h {}m", hours, mins)
    } else {
        format!("{}m {}s", mins, secs_rem)
    }
}

pub fn print_human(s: &SessionSummary, turns: &[Turn]) {
    // Header: session id (first 8 Unicode chars), model, optional mixed-session note.
    // Use chars().take(8) — byte slicing panics on multibyte characters.
    let sid: String = s.session_id.chars().take(8).collect();
    println!("stormglass / Session: {} ({})", &sid, s.model);
    if s.models_seen.len() > 1 {
        // Recompute per-model counts from the turns slice (not stored in SessionSummary)
        let mut model_counts: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
        for t in turns {
            *model_counts.entry(t.model.as_str()).or_default() += 1;
        }
        let mut note: Vec<String> = model_counts.iter()
            .map(|(m, c)| format!("{} ({})", m, c))
            .collect();
        note.sort();
        println!("mixed session: {}", note.join(", "));
    }

    println!(
        "\nDuration: {}  |  {} turns  |  {} user prompts",
        format_duration(s.duration_sec), s.total_turns, s.user_turns
    );

    println!("\nTokens");
    println!(
        "  Input:    {:>12}  (cache read: {} / cache write: {})",
        commafy(s.total_input_tokens), commafy(s.total_cache_read), commafy(s.total_cache_write)
    );
    println!("  Output:   {:>12}", commafy(s.total_output_tokens));
    if s.subagent_count > 0 {
        println!("  Subagent: {:>12}  ({} tasks)", commafy(s.total_subagent_tokens), s.subagent_count);
    }

    println!("\nBurn");
    println!("  Avg per turn:   {} input tokens", commafy(s.avg_burn_rate.max(0.0) as u64));

    // Peak turn tools: find the turn in the slice, format as "Name xN"
    let peak_tools_str = turns.iter()
        .find(|t| t.turn == s.peak_burn_turn)
        .map(|t| {
            let mut counts: Vec<(&str, u32)> = Vec::new();
            for tool in &t.tools_called {
                if let Some(entry) = counts.iter_mut().find(|(n, _)| *n == tool.as_str()) {
                    entry.1 += 1;
                } else {
                    counts.push((tool.as_str(), 1));
                }
            }
            if counts.is_empty() {
                String::new()
            } else {
                let parts: Vec<String> = counts.iter().map(|(n, c)| format!("{} x{}", n, c)).collect();
                format!("  ({})", parts.join(", "))
            }
        })
        .unwrap_or_default();
    println!(
        "  Peak:           turn {} — {} tokens{}",
        s.peak_burn_turn, commafy(s.peak_burn_value.max(0) as u64), peak_tools_str
    );
    println!("  Final context:  {} tokens", commafy(s.final_context_size));

    println!("\nThinking");
    println!(
        "  Turns with thinking: {}/{}  ({:.1}%)",
        s.turns_with_thinking, s.total_turns, s.thinking_ratio * 100.0
    );

    if !s.top_tools.is_empty() {
        println!("\nTools (top 5)");
        let max_name = s.top_tools.iter().map(|(n, _)| n.len()).max().unwrap_or(4);
        for (name, count) in &s.top_tools {
            println!("  {:<width$}  {}", name, commafy(*count as u64), width = max_name + 1);
        }
    }
}

pub fn print_json(s: &SessionSummary) {
    println!(
        "{}",
        serde_json::to_string_pretty(s).unwrap_or_else(|e| format!("{{\"error\":\"{}\"}}", e))
    );
}

pub fn write_csv_header(w: &mut impl Write) -> io::Result<()> {
    writeln!(w, "turn,timestamp,model,input_tokens,output_tokens,cache_read,cache_write,has_thinking,thinking_block_count,content_blocks,tool_count,tools_called,stop_reason,cumulative_input,burn_delta,skill,elapsed_sec,tokens_per_sec")
}

pub fn write_csv_row(t: &Turn, w: &mut impl Write) -> io::Result<()> {
    // Quote a CSV field if it contains a comma.
    let q = |s: &str| -> String {
        if s.contains(',') {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_string()
        }
    };
    writeln!(
        w,
        "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{:.6},{:.6}",
        t.turn,
        q(&t.timestamp),
        q(&t.model),
        t.input_tokens,
        t.output_tokens,
        t.cache_read_tokens,
        t.cache_write_tokens,
        t.has_thinking,
        t.thinking_block_count,
        t.content_blocks,
        t.tool_count,
        t.tools_called.join(";"),
        q(&t.stop_reason),
        t.cumulative_input,
        t.burn_delta,
        q(&t.skill),
        t.elapsed_sec,
        t.tokens_per_sec,
    )
}
