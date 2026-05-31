use std::collections::HashMap;

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub cache_creation_input_tokens: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Turn {
    pub turn: u32,
    pub timestamp: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    /// Real context size: input_tokens + cache_read_tokens + cache_write_tokens.
    /// This is the total tokens in the context window, not just the uncached fraction.
    pub context_tokens: u64,
    pub has_thinking: bool,
    pub thinking_block_count: u32,
    pub content_blocks: u32,
    pub tools_called: Vec<String>,
    pub tool_count: u32,
    pub stop_reason: String,
    pub cumulative_context: u64,
    pub burn_delta: i64, // signed; 0 for turn 1; based on context_tokens
    pub skill: String,
    pub elapsed_sec: f64,    // 0.0 for turn 1
    pub tokens_per_sec: f64, // 0.0 if elapsed == 0.0
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SubagentRecord {
    pub timestamp: String,
    pub tokens: u64,
    pub tool_uses: u32,
    pub duration_ms: u64,
}

#[derive(Debug, Default)]
pub struct SummaryAccumulator {
    pub total_input: u64,
    pub total_output: u64,
    pub total_cache_read: u64,
    pub total_cache_write: u64,
    pub turns_with_thinking: u32,
    pub total_turns: u32,
    pub burn_sum_excl_first: i64, // sum of burn_delta for turns 2..N (context-based)
    pub peak_burn_value: i64,
    pub peak_burn_turn: u32,
    pub peak_burn_tools: Vec<String>, // tools of the peak-burn turn (not serialized)
    pub tool_counts: HashMap<String, u32>,
    pub models_seen: HashMap<String, u32>,
    pub final_context_size: u64, // last turn's context_tokens
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub model: String,            // most frequent model
    pub models_seen: Vec<String>, // all distinct, sorted; for "mixed session" note
    pub start_time: String,
    pub end_time: String,
    pub duration_sec: f64,
    pub total_turns: u32,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_read: u64,
    pub total_cache_write: u64,
    pub turns_with_thinking: u32,
    pub thinking_ratio: f64,
    pub avg_burn_rate: f64,
    pub peak_burn_turn: u32,
    pub peak_burn_value: i64,
    pub tool_frequency: Vec<(String, u32)>, // all tools, count desc, ties by name asc
    pub top_tools: Vec<(String, u32)>,      // first 5 of tool_frequency
    pub total_subagent_tokens: u64,
    pub subagent_count: u32,
    pub subagent_total_duration_ms: u64,
    pub user_turns: u32,
    pub avg_output_per_turn: f64,
    pub final_context_size: u64,
}

/// Parse an ISO 8601 / RFC 3339 timestamp to milliseconds since epoch.
/// Returns None on parse failure.
pub fn parse_ts_ms(ts: &str) -> Option<i64> {
    use chrono::DateTime;
    DateTime::parse_from_rfc3339(ts)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

/// Flush a TurnAccumulator into a Turn record, updating cumulative state.
/// prev_* are None for turn 1.
pub fn flush_turn(
    acc: crate::parse::TurnAccumulator,
    turn_num: u32,
    prev_context_tokens: Option<u64>,
    prev_timestamp_ms: Option<i64>,
    cum_context: &mut u64,
) -> Turn {
    let usage = acc.usage.unwrap_or_else(|| {
        eprintln!(
            "warn: turn {} missing usage data, defaulting to 0",
            turn_num
        );
        Usage::default()
    });

    let input_tokens = usage.input_tokens;
    let output_tokens = usage.output_tokens;
    let cache_read_tokens = usage.cache_read_input_tokens;
    let cache_write_tokens = usage.cache_creation_input_tokens;

    // Real context size: uncached input + everything served from cache
    let context_tokens = input_tokens
        .saturating_add(cache_read_tokens)
        .saturating_add(cache_write_tokens);

    *cum_context = cum_context.saturating_add(context_tokens);
    let cumulative_context = *cum_context;

    // burn_delta: how much the context window grew (or shrank) since the previous turn.
    // Use i128 intermediate to avoid silent wraparound on large values.
    let burn_delta = match prev_context_tokens {
        None => 0i64, // turn 1
        Some(prev) => {
            let delta = context_tokens as i128 - prev as i128;
            delta.clamp(i64::MIN as i128, i64::MAX as i128) as i64
        }
    };

    let elapsed_sec = match prev_timestamp_ms {
        None => 0.0f64, // turn 1
        Some(prev_ms) => match parse_ts_ms(&acc.timestamp) {
            Some(this_ms) => ((this_ms - prev_ms) as f64 / 1000.0).max(0.0),
            None => 0.0,
        },
    };

    let tokens_per_sec = if elapsed_sec > 0.0 {
        output_tokens as f64 / elapsed_sec
    } else {
        0.0
    };

    let has_thinking = acc.thinking_count > 0;
    let tool_count = acc.tool_names.len() as u32;
    let content_blocks = acc.thinking_count + acc.text_count + tool_count;

    Turn {
        turn: turn_num,
        timestamp: acc.timestamp,
        model: acc.model,
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        context_tokens,
        has_thinking,
        thinking_block_count: acc.thinking_count,
        content_blocks,
        tools_called: acc.tool_names,
        tool_count,
        stop_reason: acc.stop_reason.unwrap_or_default(),
        cumulative_context,
        burn_delta,
        skill: acc.skill.unwrap_or_default(),
        elapsed_sec,
        tokens_per_sec,
    }
}

/// Build a SessionSummary from accumulated state.
pub fn build_summary(
    acc: SummaryAccumulator,
    subagents: &[SubagentRecord],
    session_id: &str,
    start_time: &str,
    end_time: &str,
    user_turns: u32,
) -> SessionSummary {
    let total_turns = acc.total_turns;

    let thinking_ratio = if total_turns == 0 {
        0.0
    } else {
        acc.turns_with_thinking as f64 / total_turns as f64
    };

    let avg_burn_rate = if total_turns > 1 {
        acc.burn_sum_excl_first as f64 / (total_turns - 1) as f64
    } else {
        0.0
    };

    let avg_output_per_turn = if total_turns == 0 {
        0.0
    } else {
        acc.total_output as f64 / total_turns as f64
    };

    let duration_sec = match (parse_ts_ms(start_time), parse_ts_ms(end_time)) {
        (Some(s), Some(e)) => (e - s) as f64 / 1000.0,
        _ => 0.0,
    };

    // Sort tool_frequency: count desc, name asc for ties
    let mut tool_frequency: Vec<(String, u32)> = acc.tool_counts.into_iter().collect();
    tool_frequency.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let top_tools = tool_frequency.iter().take(5).cloned().collect();

    // model = most frequent; ties broken by name asc
    let model = acc
        .models_seen
        .iter()
        .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
        .map(|(k, _)| k.clone())
        .unwrap_or_default();

    let mut models_seen_vec: Vec<String> = acc.models_seen.into_keys().collect();
    models_seen_vec.sort();

    let total_subagent_tokens: u64 = subagents.iter().map(|s| s.tokens).sum();
    let subagent_count = subagents.len() as u32;
    let subagent_total_duration_ms: u64 = subagents.iter().map(|s| s.duration_ms).sum();

    // peak_burn_turn: fallback to 1 if never updated (0 or 1 turn)
    let peak_burn_turn = if acc.peak_burn_turn == 0 && total_turns >= 1 {
        1
    } else {
        acc.peak_burn_turn
    };

    SessionSummary {
        session_id: session_id.to_owned(),
        model,
        models_seen: models_seen_vec,
        start_time: start_time.to_owned(),
        end_time: end_time.to_owned(),
        duration_sec,
        total_turns,
        total_input_tokens: acc.total_input,
        total_output_tokens: acc.total_output,
        total_cache_read: acc.total_cache_read,
        total_cache_write: acc.total_cache_write,
        turns_with_thinking: acc.turns_with_thinking,
        thinking_ratio,
        avg_burn_rate,
        peak_burn_turn,
        peak_burn_value: acc.peak_burn_value,
        tool_frequency,
        top_tools,
        total_subagent_tokens,
        subagent_count,
        subagent_total_duration_ms,
        user_turns,
        avg_output_per_turn,
        final_context_size: acc.final_context_size,
    }
}
