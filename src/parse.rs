use std::collections::HashMap;
use std::io::{self, BufRead};

use crate::metrics::{
    flush_turn, parse_ts_ms, SessionMeta, SubagentRecord, SubagentSplit, SummaryAccumulator, Turn,
    Usage,
};

// ── Entry types ──────────────────────────────────────────────────────────────

pub enum Entry {
    Assistant(AssistantEntry),
    UserHuman,        // non-meta user with no tool_result block (see D2)
    Subagent(String), // raw queue-operation content string for XML extraction
    Skip,             // sidechain, meta-user, tool_result-user, unknown, parse error
}

pub struct AssistantEntry {
    pub message_id: String,
    pub timestamp: String,
    pub model: String,
    pub stop_reason: Option<String>,
    pub usage: Option<Usage>, // None -> warn on flush, default 0
    pub content_type: ContentType,
    pub skill: Option<String>,
    pub session_id: String, // envelope sessionId (capture once)
}

pub enum ContentType {
    Thinking,
    Text,
    ToolUse { tool_name: String },
}

// ── Turn accumulator ─────────────────────────────────────────────────────────

pub struct TurnAccumulator {
    pub message_id: String,
    pub timestamp: String, // FIRST entry in group
    pub model: String,
    pub stop_reason: Option<String>,
    pub skill: Option<String>,
    pub usage: Option<Usage>, // FIRST entry; subsequent ignored
    pub thinking_count: u32,
    pub text_count: u32,
    pub tool_names: Vec<String>,
}

// ── ParseResult ──────────────────────────────────────────────────────────────

pub struct ParseResult {
    pub turns: Vec<Turn>,
    pub summary: crate::metrics::SessionSummary,
    #[allow(dead_code)]
    pub subagents: Vec<SubagentRecord>,
}

// ── dispatch_line: the ONLY place that touches serde_json::Value ─────────────

/// Classify a single JSONL line into an Entry variant.
/// Parses the JSON, then delegates to `dispatch_value`.
/// Returns Skip on JSON parse failure (silently — callers that want a diagnostic
/// should call `serde_json::from_str` themselves and call `dispatch_value` directly).
/// Kept as a public, unit-testable entry point; the hot parse loop calls dispatch_value.
#[allow(dead_code)]
pub fn dispatch_line(line: &str) -> Entry {
    let v: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return Entry::Skip,
    };
    dispatch_value(&v)
}

/// Classify an already-parsed JSON value into an Entry variant.
/// Check order: isSidechain skip -> type -> match type.
/// All serde_json::Value navigation is concentrated here.
pub fn dispatch_value(v: &serde_json::Value) -> Entry {
    // Skip sidechains before checking type
    if v.get("isSidechain")
        .and_then(|x| x.as_bool())
        .unwrap_or(false)
    {
        return Entry::Skip;
    }

    let entry_type = match v.get("type").and_then(|x| x.as_str()) {
        Some(t) => t,
        None => return Entry::Skip,
    };

    match entry_type {
        "assistant" => dispatch_assistant(v),
        "user" => dispatch_user(v),
        "queue-operation" => dispatch_queue_operation(v),
        _ => Entry::Skip,
    }
}

fn dispatch_assistant(v: &serde_json::Value) -> Entry {
    // message.id is required
    let message_id = match v
        .get("message")
        .and_then(|m| m.get("id"))
        .and_then(|id| id.as_str())
    {
        Some(s) => s.to_owned(),
        None => return Entry::Skip,
    };

    let timestamp = v
        .get("timestamp")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_owned();

    let model = v
        .get("message")
        .and_then(|m| m.get("model"))
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_owned();

    let stop_reason = v
        .get("message")
        .and_then(|m| m.get("stop_reason"))
        .and_then(|x| x.as_str())
        .map(|s| s.to_owned());

    let session_id = v
        .get("sessionId")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_owned();

    let skill = v
        .get("attributionSkill")
        .and_then(|x| x.as_str())
        .map(|s| s.to_owned());

    // usage — only if the usage object exists
    let usage = v
        .get("message")
        .and_then(|m| m.get("usage"))
        .map(Usage::from_value);

    // content_type from message.content[0] (each JSONL line is one content block)
    let content_type = v
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|block| block.get("type"))
        .and_then(|t| t.as_str())
        .map(|t| match t {
            "thinking" => ContentType::Thinking,
            "tool_use" => {
                let tool_name = v
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|block| block.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("unknown")
                    .to_owned();
                ContentType::ToolUse { tool_name }
            }
            _ => ContentType::Text,
        })
        .unwrap_or(ContentType::Text);

    Entry::Assistant(AssistantEntry {
        message_id,
        timestamp,
        model,
        stop_reason,
        usage,
        content_type,
        skill,
        session_id,
    })
}

fn dispatch_user(v: &serde_json::Value) -> Entry {
    let is_meta = v.get("isMeta").and_then(|x| x.as_bool()).unwrap_or(false);
    let content = v.get("message").and_then(|m| m.get("content"));

    if is_meta {
        // Hook-delivered prompts arrive as isMeta with string content. Count them.
        // Skill loads arrive as isMeta with list content. Skip them.
        return match content {
            Some(c) if c.is_string() => Entry::UserHuman,
            _ => Entry::Skip,
        };
    }

    // D2: inspect content to determine if this is a real human turn
    match content {
        // string content -> human turn
        Some(c) if c.is_string() => Entry::UserHuman,
        // list content -> human only if no tool_result element
        Some(c) if c.is_array() => {
            let has_tool_result = c
                .as_array()
                .map(|arr| {
                    arr.iter().any(|block| {
                        block.get("type").and_then(|t| t.as_str()) == Some("tool_result")
                    })
                })
                .unwrap_or(false);
            if has_tool_result {
                Entry::Skip
            } else {
                Entry::UserHuman
            }
        }
        // no content or unknown shape -> skip
        _ => Entry::Skip,
    }
}

fn dispatch_queue_operation(v: &serde_json::Value) -> Entry {
    // Extract the content string for XML parsing downstream
    let content = v
        .get("content")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_owned();
    Entry::Subagent(content)
}

// ── XML tag extraction (no regex) ───────────────────────────────────────────

fn extract_u64(content: &str, tag: &str) -> Option<u64> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = content.find(&open)? + open.len();
    let end = content[start..].find(&close)?;
    content[start..start + end].trim().parse().ok()
}

// ── Streaming parse ──────────────────────────────────────────────────────────

/// Parse a session JSONL file, optionally streaming CSV rows.
/// `from_turn`: turns numbered below this value are counted but excluded from
/// metrics, CSV output, and the returned `turns` vec. Use 1 (the default) for
/// a full-session analysis.
pub fn parse_session(
    path: &str,
    csv_path: Option<&str>,
    from_turn: u32,
) -> io::Result<ParseResult> {
    // Reject non-regular-file paths up front (e.g. directories) — File::open
    // succeeds on a directory but BufReader::lines() errors on every poll,
    // causing an infinite loop without this guard.
    // Normalize from_turn so callers passing 0 get full-session behavior.
    let from_turn = from_turn.max(1);

    let meta = std::fs::metadata(path)?;
    if !meta.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{}: not a regular file", path),
        ));
    }

    let file = std::fs::File::open(path)?;
    let reader = io::BufReader::new(file);

    let mut csv_writer: Option<Box<dyn std::io::Write>> = if let Some(p) = csv_path {
        let f = std::fs::File::create(p)?;
        let mut w: Box<dyn std::io::Write> = Box::new(io::BufWriter::new(f));
        crate::output::write_csv_header(&mut w)?;
        Some(w)
    } else {
        None
    };

    let mut turns: Vec<Turn> = Vec::new();
    let mut subagents: Vec<SubagentRecord> = Vec::new();
    let mut summary_acc = SummaryAccumulator::default();

    // Session-level state
    let mut session_id = String::new();
    let mut start_time = String::new();
    let mut end_time = String::new();
    let mut user_turns: u32 = 0;

    // Per-turn streaming state
    let mut current_group: Option<TurnAccumulator> = None;
    let mut prev_context_tokens: Option<u64> = None;
    let mut prev_timestamp_ms: Option<i64> = None;
    let mut cum_context: u64 = 0;
    let mut turn_num: u32 = 0;

    // Last-seen timestamp for subagent records (queue-operation entries)
    let mut last_queue_timestamp = String::new();

    let lines = reader.lines();
    for (line_idx, line_result) in lines.enumerate() {
        let line = match line_result {
            Ok(l) => l,
            Err(e) => {
                // I/O error on read — break, do not continue looping (would spin forever
                // on persistent errors like reading from a directory fd).
                eprintln!("Line {}: read error: {}", line_idx + 1, e);
                break;
            }
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Parse JSON exactly once per line. Emit a diagnostic and skip on failure
        // (F4: spec requires "Line N: parse error: ..."). Intentional skips (sidechain /
        // meta / unknown type) are SILENT — only JSON parse errors get the diagnostic.
        // The parsed Value is reused for timestamp tracking AND entry dispatch so
        // the string is not parsed a second time.
        let raw_val = match serde_json::from_str::<serde_json::Value>(line) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("Line {}: parse error: {}", line_idx + 1, e);
                continue;
            }
        };

        // Track last-seen timestamp for queue-operation subagent records and
        // session time boundaries.
        if let Some(ts) = raw_val.get("timestamp").and_then(|x| x.as_str()) {
            if !ts.is_empty() {
                last_queue_timestamp = ts.to_owned();
                if start_time.is_empty() {
                    start_time = ts.to_owned();
                }
                end_time = ts.to_owned();
            }
        }

        match dispatch_value(&raw_val) {
            Entry::Skip => {}

            Entry::UserHuman => {
                user_turns += 1;
            }

            Entry::Subagent(content) => {
                // D1: only create a record if <subagent_tokens> is present
                if let Some(tokens) = extract_u64(&content, "subagent_tokens") {
                    let tool_uses = extract_u64(&content, "tool_uses").unwrap_or(0) as u32;
                    let duration_ms = extract_u64(&content, "duration_ms").unwrap_or(0);
                    subagents.push(SubagentRecord {
                        timestamp: last_queue_timestamp.clone(),
                        tokens,
                        tool_uses,
                        duration_ms,
                    });
                }
                // no record for dequeue/remove/popAll or token-less enqueues
            }

            Entry::Assistant(entry) => {
                // Capture session_id from first assistant entry that has it
                if session_id.is_empty() && !entry.session_id.is_empty() {
                    session_id = entry.session_id.clone();
                }

                let is_new_group = current_group
                    .as_ref()
                    .map(|g| g.message_id != entry.message_id)
                    .unwrap_or(true);

                if is_new_group {
                    // Flush previous group if any
                    if let Some(acc) = current_group.take() {
                        let turn = flush_and_record(
                            acc,
                            &mut turn_num,
                            &mut prev_context_tokens,
                            &mut prev_timestamp_ms,
                            &mut cum_context,
                            &mut summary_acc,
                            from_turn,
                        );
                        if turn.turn >= from_turn {
                            if let Some(ref mut w) = csv_writer {
                                crate::output::write_csv_row(&turn, w)?;
                            }
                            turns.push(turn);
                        }
                    }

                    // Start new group — consume entry into accumulator and record
                    // the first content block immediately.
                    let mut new_acc = TurnAccumulator {
                        message_id: entry.message_id,
                        timestamp: entry.timestamp,
                        model: entry.model,
                        stop_reason: entry.stop_reason,
                        skill: entry.skill,
                        usage: entry.usage,
                        thinking_count: 0,
                        text_count: 0,
                        tool_names: Vec::new(),
                    };
                    match entry.content_type {
                        ContentType::Thinking => new_acc.thinking_count += 1,
                        ContentType::Text => new_acc.text_count += 1,
                        ContentType::ToolUse { tool_name } => new_acc.tool_names.push(tool_name),
                    }
                    current_group = Some(new_acc);
                } else {
                    // Accumulate additional content block into existing group
                    if let Some(ref mut acc) = current_group {
                        // stop_reason: last non-None wins for the group
                        if entry.stop_reason.is_some() {
                            acc.stop_reason = entry.stop_reason;
                        }
                        // skill: first non-None wins; usage: FIRST entry only (already set)
                        if acc.skill.is_none() && entry.skill.is_some() {
                            acc.skill = entry.skill;
                        }
                        match entry.content_type {
                            ContentType::Thinking => acc.thinking_count += 1,
                            ContentType::Text => acc.text_count += 1,
                            ContentType::ToolUse { tool_name } => acc.tool_names.push(tool_name),
                        }
                    }
                }
            }
        }
    }

    // EOF flush: flush the last group if any
    if let Some(acc) = current_group.take() {
        let turn = flush_and_record(
            acc,
            &mut turn_num,
            &mut prev_context_tokens,
            &mut prev_timestamp_ms,
            &mut cum_context,
            &mut summary_acc,
            from_turn,
        );
        if turn.turn >= from_turn {
            if let Some(ref mut w) = csv_writer {
                crate::output::write_csv_row(&turn, w)?;
            }
            turns.push(turn);
        }
    }

    // Second, independent measurement: aggregate the sibling agent-*.jsonl
    // transcripts (W448). Whole-session regardless of from_turn — a sliced
    // analysis still reports the full subagent split.
    let subagent_split = aggregate_subagent_files(path);

    let summary = crate::metrics::build_summary(
        summary_acc,
        &subagents,
        SessionMeta {
            session_id: &session_id,
            start_time: &start_time,
            end_time: &end_time,
            user_turns,
            from_turn,
        },
        subagent_split,
    );

    // Relocated-transcript / older-session signal: the queue-op lump saw
    // subagent activity but the agent-file split found nothing on disk next
    // to the transcript. Expected (not an error) for sample fixtures and
    // transcripts analyzed away from their original directory.
    if summary.subagent_count > 0 && summary.subagent_agent_file_count == 0 {
        eprintln!("note: subagent files not found next to transcript; split unavailable");
    }

    Ok(ParseResult {
        turns,
        summary,
        subagents,
    })
}

// ── Subagent-file aggregation (W448) ────────────────────────────────────────
//
// Agent transcripts are NOT siblings of the main transcript file — they live
// in a derived subdirectory:
//   <dir>/<session-id>.jsonl -> <dir>/<session-id>/subagents/agent-*.jsonl
//
// Every assistant line in these files carries isSidechain:true, which is
// exactly the flag dispatch_value/dispatch_assistant skip. This aggregator
// therefore reads and classifies agent-file lines directly — it deliberately
// does NOT call dispatch_value/dispatch_assistant, or every field here would
// silently compute to zero against real production data.

/// Derive the subagents directory from a transcript path, glob agent-*.jsonl
/// files (via std::fs::read_dir — no glob crate), and aggregate their token
/// usage. Missing `.jsonl` suffix, missing/unreadable directory, or a dir with
/// no matching files all yield an all-zero split — never an error, never a
/// panic. Callers (older sessions, relocated transcripts, fixtures without a
/// subagents dir) rely on this.
pub fn aggregate_subagent_files(transcript_path: &str) -> SubagentSplit {
    let dir = match transcript_path.strip_suffix(".jsonl") {
        Some(p) => format!("{p}/subagents"),
        None => return SubagentSplit::default(),
    };

    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return SubagentSplit::default(),
    };

    // Filter to agent-*.jsonl (naturally excludes the agent-*.meta.json
    // companion files, which end in .json not .jsonl). Sort for deterministic
    // aggregation order.
    let mut files: Vec<std::path::PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("agent-") && n.ends_with(".jsonl"))
                .unwrap_or(false)
        })
        .collect();
    files.sort();

    let mut total = SubagentSplit::default();
    for file in &files {
        if let Some(file_usage) = aggregate_one_agent_file(file) {
            total.input = total.input.saturating_add(file_usage.input_tokens);
            total.output = total.output.saturating_add(file_usage.output_tokens);
            total.cache_read = total
                .cache_read
                .saturating_add(file_usage.cache_read_input_tokens);
            total.cache_write = total
                .cache_write
                .saturating_add(file_usage.cache_creation_input_tokens);
            total.agent_file_count += 1;
        }
    }
    total
}

/// Stream one agent-*.jsonl file and return its summed usage, or None if it
/// yielded no usable (usage-bearing) turn. Streams line-by-line — files run
/// up to ~700KB and there can be 8-24 per session, so this never slurps a
/// whole file (let alone all files) into memory at once.
///
/// The aggregation trap (measured, load-bearing): usage is repeated on every
/// content-block line sharing a message.id. Naive summation across lines
/// inflates every field (measured ~4.6x on input). Group by message.id first;
/// within a group, input/cache are constant across blocks but output_tokens
/// grows (e.g. 5,5,5,412) — take the max per group rather than strictly the
/// last line, which gives the same result on well-formed monotonic data and
/// is strictly safer if a group is ever malformed.
fn aggregate_one_agent_file(path: &std::path::Path) -> Option<Usage> {
    let file = std::fs::File::open(path).ok()?;
    let reader = io::BufReader::new(file);

    let mut groups: HashMap<String, Usage> = HashMap::new();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break, // I/O error mid-file — stop, don't spin
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue, // malformed line — skip, not fatal
        };

        // Deliberately no isSidechain check: every real line here is a
        // sidechain by definition (that's why it lives in this file).
        if v.get("type").and_then(|t| t.as_str()) != Some("assistant") {
            continue;
        }

        let message_id = match v
            .get("message")
            .and_then(|m| m.get("id"))
            .and_then(|id| id.as_str())
        {
            Some(s) => s.to_owned(),
            None => continue,
        };

        let usage = match v.get("message").and_then(|m| m.get("usage")) {
            Some(u) => Usage::from_value(u),
            None => continue,
        };

        groups
            .entry(message_id)
            .and_modify(|acc| {
                acc.output_tokens = acc.output_tokens.max(usage.output_tokens);
                // input/cache are constant within a group in well-formed data;
                // re-assigning from the latest line is a no-op there and self-
                // heals if a line is ever missing them (usage defaults to 0
                // only when the field itself is absent from that line).
                acc.input_tokens = usage.input_tokens;
                acc.cache_read_input_tokens = usage.cache_read_input_tokens;
                acc.cache_creation_input_tokens = usage.cache_creation_input_tokens;
            })
            .or_insert(usage);
    }

    if groups.is_empty() {
        return None;
    }

    let mut total = Usage::default();
    for usage in groups.values() {
        total.input_tokens = total.input_tokens.saturating_add(usage.input_tokens);
        total.output_tokens = total.output_tokens.saturating_add(usage.output_tokens);
        total.cache_read_input_tokens = total
            .cache_read_input_tokens
            .saturating_add(usage.cache_read_input_tokens);
        total.cache_creation_input_tokens = total
            .cache_creation_input_tokens
            .saturating_add(usage.cache_creation_input_tokens);
    }
    Some(total)
}

/// Flush a TurnAccumulator and update SummaryAccumulator.
/// Turns with `turn_num < from_turn` still update prev-state (so burn deltas
/// stay correct for the slice) but are excluded from all summary counters.
fn flush_and_record(
    acc: TurnAccumulator,
    turn_num: &mut u32,
    prev_context_tokens: &mut Option<u64>,
    prev_timestamp_ms: &mut Option<i64>,
    cum_context: &mut u64,
    summary_acc: &mut SummaryAccumulator,
    from_turn: u32,
) -> Turn {
    *turn_num += 1;
    let n = *turn_num;

    let prev_ts_ms = *prev_timestamp_ms;
    let this_ts_ms = parse_ts_ms(&acc.timestamp);

    let turn = flush_turn(acc, n, *prev_context_tokens, prev_ts_ms, cum_context);

    // Only accumulate metrics for turns within the requested slice.
    if n >= from_turn {
        // Update summary accumulator (saturating to avoid overflow on malformed large values)
        summary_acc.total_input = summary_acc.total_input.saturating_add(turn.input_tokens);
        summary_acc.total_output = summary_acc.total_output.saturating_add(turn.output_tokens);
        summary_acc.total_cache_read = summary_acc
            .total_cache_read
            .saturating_add(turn.cache_read_tokens);
        summary_acc.total_cache_write = summary_acc
            .total_cache_write
            .saturating_add(turn.cache_write_tokens);
        summary_acc.total_turns += 1;
        summary_acc.final_context_size = turn.context_tokens;

        if turn.has_thinking {
            summary_acc.turns_with_thinking += 1;
            summary_acc.total_thinking_output = summary_acc
                .total_thinking_output
                .saturating_add(turn.output_tokens);
        }

        // Accumulate tools
        for tool in &turn.tools_called {
            *summary_acc.tool_counts.entry(tool.clone()).or_insert(0) += 1;
        }

        // Accumulate model counts
        if !turn.model.is_empty() {
            *summary_acc
                .models_seen
                .entry(turn.model.clone())
                .or_insert(0) += 1;
        }

        // Burn tracking: skip only the absolute first turn (n == 1), which always
        // has burn_delta == 0 from flush_turn (prev_context_tokens is None).
        // When from_turn > 1, the first included turn has a real delta because
        // prev-state was maintained through all skipped turns — include it.
        if n >= 2 {
            summary_acc.burn_sum_excl_first += turn.burn_delta;
            if turn.burn_delta > summary_acc.peak_burn_value {
                summary_acc.peak_burn_value = turn.burn_delta;
                summary_acc.peak_burn_turn = n;
                summary_acc.peak_burn_tools = turn.tools_called.clone();
            }
        }
    }

    // Always advance prev state so burn deltas stay correct for subsequent turns.
    *prev_context_tokens = Some(turn.context_tokens);
    *prev_timestamp_ms = this_ts_ms;

    turn
}

// ── Unit tests for dispatch_line ─────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_assistant(message_id: &str, content_type: &str, tool_name: Option<&str>) -> String {
        let content_block = match content_type {
            "thinking" => r#"{"type":"thinking","thinking":"..."}"#.to_owned(),
            "tool_use" => {
                let name = tool_name.unwrap_or("Bash");
                format!(
                    r#"{{"type":"tool_use","name":"{}","id":"t1","input":{{}}}}"#,
                    name
                )
            }
            _ => r#"{"type":"text","text":"hello"}"#.to_owned(),
        };
        format!(
            r#"{{"type":"assistant","timestamp":"2026-05-29T10:00:00.000Z","sessionId":"sess1","isSidechain":false,"message":{{"id":"{}","model":"claude-opus-4-8","stop_reason":"end_turn","usage":{{"input_tokens":1000,"output_tokens":200,"cache_read_input_tokens":800,"cache_creation_input_tokens":100}},"content":[{}]}}}}"#,
            message_id, content_block
        )
    }

    #[test]
    fn test_dispatch_assistant_text() {
        let line = make_assistant("msg_001", "text", None);
        match dispatch_line(&line) {
            Entry::Assistant(e) => {
                assert_eq!(e.message_id, "msg_001");
                assert_eq!(e.model, "claude-opus-4-8");
                assert!(matches!(e.content_type, ContentType::Text));
                let u = e.usage.unwrap();
                assert_eq!(u.input_tokens, 1000);
                assert_eq!(u.output_tokens, 200);
            }
            _ => panic!("expected Assistant entry"),
        }
    }

    #[test]
    fn test_dispatch_assistant_thinking() {
        let line = make_assistant("msg_002", "thinking", None);
        match dispatch_line(&line) {
            Entry::Assistant(e) => {
                assert!(matches!(e.content_type, ContentType::Thinking));
            }
            _ => panic!("expected Assistant entry"),
        }
    }

    #[test]
    fn test_dispatch_assistant_tool_use() {
        let line = make_assistant("msg_003", "tool_use", Some("Read"));
        match dispatch_line(&line) {
            Entry::Assistant(e) => match e.content_type {
                ContentType::ToolUse { tool_name } => assert_eq!(tool_name, "Read"),
                _ => panic!("expected ToolUse"),
            },
            _ => panic!("expected Assistant entry"),
        }
    }

    #[test]
    fn test_sidechain_skip() {
        let line = r#"{"type":"assistant","isSidechain":true,"timestamp":"2026-05-29T10:00:00.000Z","message":{"id":"msg_sc1","model":"claude-opus-4-8","usage":{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"text","text":"sidechain"}]}}"#;
        assert!(matches!(dispatch_line(line), Entry::Skip));
    }

    #[test]
    fn test_meta_user_list_skip() {
        // isMeta with list content (skill load) -> Skip
        let line = r#"{"type":"user","isMeta":true,"message":{"content":[{"type":"text","text":"skill load"}]}}"#;
        assert!(matches!(dispatch_line(line), Entry::Skip));
    }

    #[test]
    fn test_meta_user_string_content_human() {
        // isMeta with string content (stop-hook-delivered prompt) -> UserHuman
        let line = r#"{"type":"user","isMeta":true,"message":{"content":"meta content"}}"#;
        assert!(matches!(dispatch_line(line), Entry::UserHuman));
    }

    #[test]
    fn test_tool_result_user_skip() {
        // D2: user entry with tool_result in list content -> Skip
        let line = r#"{"type":"user","isMeta":false,"message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"output"}]}}"#;
        assert!(matches!(dispatch_line(line), Entry::Skip));
    }

    #[test]
    fn test_user_string_content_human() {
        // D2: string content -> UserHuman
        let line = r#"{"type":"user","isMeta":false,"message":{"content":"Hello Claude"}}"#;
        assert!(matches!(dispatch_line(line), Entry::UserHuman));
    }

    #[test]
    fn test_user_list_no_tool_result_human() {
        // D2: list with no tool_result -> UserHuman
        let line = r#"{"type":"user","isMeta":false,"message":{"content":[{"type":"text","text":"image paste"}]}}"#;
        assert!(matches!(dispatch_line(line), Entry::UserHuman));
    }

    #[test]
    fn test_queue_operation_with_tokens() {
        // D1: queue-operation with subagent_tokens -> Subagent with content
        let line = r#"{"type":"queue-operation","timestamp":"2026-05-29T10:05:00.000Z","operation":"enqueue","content":"<subagent_tokens>5000</subagent_tokens><tool_uses>3</tool_uses><duration_ms>12000</duration_ms>"}"#;
        match dispatch_line(line) {
            Entry::Subagent(content) => {
                assert!(content.contains("<subagent_tokens>"));
                let tokens = extract_u64(&content, "subagent_tokens");
                assert_eq!(tokens, Some(5000));
            }
            _ => panic!("expected Subagent entry"),
        }
    }

    #[test]
    fn test_queue_operation_dequeue_produces_subagent_entry() {
        // dispatch_line returns Subagent for all queue-operations;
        // the D1 gate is in the parse loop (extract_u64 check).
        // Verify dequeue content without token tag yields no extractable tokens.
        let line = r#"{"type":"queue-operation","operation":"dequeue","content":"task dequeued"}"#;
        match dispatch_line(line) {
            Entry::Subagent(content) => {
                // The gate in parse_session checks extract_u64 - should be None
                assert!(extract_u64(&content, "subagent_tokens").is_none());
            }
            _ => panic!("expected Subagent entry from queue-operation"),
        }
    }

    #[test]
    fn test_parse_error_produces_skip() {
        assert!(matches!(dispatch_line("not json at all"), Entry::Skip));
        assert!(matches!(dispatch_line("{incomplete"), Entry::Skip));
    }
}
