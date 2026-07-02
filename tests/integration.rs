// Integration tests — run stormglass binary against fixtures
//
// ══════════════════════════════════════════════════════════════════════════════
// HAND-COMPUTED EXPECTED VALUES for sample_session.jsonl
// ══════════════════════════════════════════════════════════════════════════════
//
// Fixture: 12 assistant message.id groups → 12 turns
//
// Assistant turns and their content/usage:
// ┌──────┬──────────────────────┬───────────────────────────┬──────────┬──────────┬────────────┬─────────────┬─────────────┐
// │ Turn │ message_id           │ content                   │  input   │  output  │ cache_read │ cache_write │ has_thinking│
// ├──────┼──────────────────────┼───────────────────────────┼──────────┼──────────┼────────────┼─────────────┼─────────────┤
// │  1   │ msg_turn01_aaaa      │ tool_use:Bash             │  5,000   │    300   │         0  │    25,000   │ false       │
// │  2   │ msg_turn02_bbbb      │ thinking+text+tool_use:Rd │  8,000   │    450   │     3,000  │         0   │ true        │ ← 3-line group
// │  3   │ msg_turn03_cccc      │ tool_use:Bash+Write       │ 12,000   │    600   │     5,000  │     1,000   │ false       │
// │  4   │ msg_turn04_dddd      │ tool_use:Read             │ 15,000   │    500   │     8,000  │         0   │ false       │
// │  5   │ msg_turn05_eeee      │ tool_use:Bash             │ 18,000   │    750   │    10,000  │       500   │ false       │
// │  6   │ msg_turn06_ffff      │ thinking+tool_use:Edit    │ 22,000   │    400   │    14,000  │         0   │ true        │
// │  7   │ msg_turn07_gggg      │ thinking+text+Bash+Read   │ 25,000   │    800   │    17,000  │     1,000   │ true        │ ← 4-line group
// │  8   │ msg_turn08_hhhh      │ tool_use:Write            │ 28,000   │    600   │    20,000  │         0   │ false       │
// │  9   │ msg_turn09_iiii      │ tool_use:Bash             │ 32,000   │    900   │    24,000  │       500   │ false       │
// │ 10   │ msg_turn10_jjjj      │ tool_use:Edit             │ 35,000   │    500   │    27,000  │         0   │ false       │
// │ 11   │ msg_turn11_kkkk      │ tool_use:Read             │ 40,000   │    700   │    30,000  │     1,000   │ false       │
// │ 12   │ msg_turn12_llll      │ tool_use:Bash+Write       │ 45,000   │  1,200   │    35,000  │         0   │ false       │
// └──────┴──────────────────────┴───────────────────────────┴──────────┴──────────┴────────────┴─────────────┴─────────────┘
//
// Excluded: msg_sidechain_xxxx (isSidechain:true) → NOT a turn
//
// TOKEN SUMS:
//   total_input_tokens  = 5000+8000+12000+15000+18000+22000+25000+28000+32000+35000+40000+45000
//                       = 285,000
//   total_output_tokens = 300+450+600+500+750+400+800+600+900+500+700+1200
//                       = 7,700
//   total_cache_read    = 0+3000+5000+8000+10000+14000+17000+20000+24000+27000+30000+35000
//                       = 193,000
//   total_cache_write   = 25000+0+1000+0+500+0+1000+0+500+0+1000+0
//                       = 29,000
//
// THINKING:
//   turns_with_thinking = 3 (turns 2, 6, 7)
//   thinking_ratio      = 3/12 = 0.25
//
// SUBAGENT (D1 guard):
//   enqueue w/ subagent_tokens=5000 → SubagentRecord (COUNTS)
//   dequeue (no subagent_tokens)    → no record (DOES NOT COUNT) ← D1 trap
//   enqueue w/ subagent_tokens=8000 → SubagentRecord (COUNTS)
//   subagent_count         = 2   (NOT 3 — the dequeue doesn't count)
//   total_subagent_tokens  = 5000 + 8000 = 13,000
//
// USER TURNS (D2 guard):
//   "Hello, let's start..."        → UserHuman (string)      COUNTS
//   tool_result list               → Skip (D2)               DOES NOT COUNT ← D2 trap
//   "Can you analyze this file?"   → UserHuman (string)      COUNTS
//   tool_result list               → Skip (D2)               DOES NOT COUNT ← D2 trap
//   tool_result list               → Skip (D2)               DOES NOT COUNT ← D2 trap
//   "What do you think...?"        → UserHuman (string)      COUNTS
//   "Please continue..."           → UserHuman (string)      COUNTS
//   user_turns = 4   (NOT 7 — tool_result entries don't count)
//
// MULTI-LINE DEDUP PROOF:
//   Turn 2: msg_turn02_bbbb appears on 3 JSONL lines; usage identical across all 3
//           → usage taken from FIRST line only
//   Turn 7: msg_turn07_gggg appears on 4 JSONL lines; usage identical across all 4
//           → usage taken from FIRST line only
//   Without dedup: output_tokens would be inflated by 2×450 + 3×800 = 3300
//
// ══════════════════════════════════════════════════════════════════════════════

use std::process::Command;

fn fixture_path() -> String {
    let manifest = env!("CARGO_MANIFEST_DIR");
    format!("{}/tests/fixtures/sample_session.jsonl", manifest)
}

fn empty_fixture_path() -> String {
    let manifest = env!("CARGO_MANIFEST_DIR");
    format!("{}/tests/fixtures/empty.jsonl", manifest)
}

/// Run stormglass with the given args; return (exit_status_code, stdout, stderr)
fn run_stormglass(args: &[&str]) -> (i32, String, String) {
    let bin = env!("CARGO_BIN_EXE_stormglass");
    let output = Command::new(bin)
        .args(args)
        .output()
        .expect("failed to run stormglass binary");
    let code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (code, stdout, stderr)
}

// ── Smoke tests: empty file → exit 0; notice on stdout only in human mode ──

#[test]
fn test_empty_file_smoke() {
    let path = empty_fixture_path();
    std::fs::write(&path, "").expect("could not write empty fixture");

    let (code, stdout, _stderr) = run_stormglass(&["analyze", &path]);
    assert_eq!(code, 0, "exit code should be 0 for empty file");
    assert!(
        stdout.contains("0 turns") && stdout.contains("no telemetry"),
        "expected '0 turns — no telemetry' in stdout, got: {}",
        stdout
    );
}

#[test]
fn test_empty_file_json_stdout_is_valid_json() {
    let path = empty_fixture_path();
    std::fs::write(&path, "").expect("could not write empty fixture");

    let (code, stdout, stderr) = run_stormglass(&["analyze", &path, "--json", "--quiet"]);
    assert_eq!(code, 0, "exit code should be 0 for empty file");
    // Entire stdout must parse as one JSON document — no notice text mixed in.
    let summary: serde_json::Value =
        serde_json::from_str(&stdout).expect("--json stdout was not valid JSON for empty file");
    assert_eq!(summary["total_turns"], 0);
    assert!(
        stderr.contains("0 turns"),
        "human notice should move to stderr under --json, got stderr: {}",
        stderr
    );
}

// ── Regression: --json --quiet must emit the JSON document on stdout ────────
// Consumers capture stdout (`$(stormglass ... --json --quiet 2>/dev/null)`)
// and pipe it into a JSON parser. Anything on stdout that isn't the JSON
// document — or the document landing on stderr — breaks them silently.

#[test]
fn test_json_quiet_stdout_is_exactly_one_json_document() {
    let path = fixture_path();
    let (code, stdout, _stderr) = run_stormglass(&["analyze", &path, "--json", "--quiet"]);
    assert_eq!(code, 0, "stormglass exited with code {}", code);
    assert!(!stdout.trim().is_empty(), "--json stdout must not be empty");
    // Parsing the FULL stdout (not a substring) proves nothing else leaked in.
    let summary: serde_json::Value = serde_json::from_str(&stdout)
        .expect("--json --quiet stdout was not a single valid JSON document");
    assert!(
        summary["total_turns"].as_u64().unwrap_or(0) > 0,
        "summary should carry real data"
    );
}

// ── Main fixture tests ───────────────────────────────────────────────────────

fn parse_summary_json() -> serde_json::Value {
    let path = fixture_path();
    let (code, stdout, stderr) = run_stormglass(&["analyze", &path, "--json"]);
    assert_eq!(
        code, 0,
        "stormglass exited with code {} — stderr: {}",
        code, stderr
    );
    serde_json::from_str(&stdout).expect("stdout was not valid JSON")
}

#[test]
fn test_total_turns() {
    let summary = parse_summary_json();
    assert_eq!(
        summary["total_turns"].as_u64().unwrap(),
        12,
        "total_turns should be 12"
    );
}

#[test]
fn test_turns_with_thinking() {
    let summary = parse_summary_json();
    assert_eq!(
        summary["turns_with_thinking"].as_u64().unwrap(),
        3,
        "exactly 3 turns have thinking blocks (turns 2, 6, 7)"
    );
}

#[test]
fn test_thinking_ratio() {
    let summary = parse_summary_json();
    let ratio = summary["thinking_ratio"].as_f64().unwrap();
    // 3/12 = 0.25 exactly
    assert!(
        (ratio - 0.25).abs() < 1e-9,
        "thinking_ratio should be 0.25, got {}",
        ratio
    );
}

// ── Thinking token volume ────────────────────────────────────────────────────
//
// Thinking turns: 2 (output=450), 6 (output=400), 7 (output=800)
//   total_thinking_output     = 450 + 400 + 800 = 1,650
//   total_non_thinking_output = 7,700 - 1,650   = 6,050
//   avg_thinking_output_per_turn = 1650 / 3     = 550.0

#[test]
fn test_total_thinking_output() {
    let summary = parse_summary_json();
    assert_eq!(
        summary["total_thinking_output"].as_u64().unwrap(),
        1_650,
        "total_thinking_output must be 1650 (turns 2+6+7: 450+400+800)"
    );
}

#[test]
fn test_total_non_thinking_output() {
    let summary = parse_summary_json();
    assert_eq!(
        summary["total_non_thinking_output"].as_u64().unwrap(),
        6_050,
        "total_non_thinking_output must be 6050 (7700 - 1650)"
    );
}

#[test]
fn test_avg_thinking_output_per_turn() {
    let summary = parse_summary_json();
    let avg = summary["avg_thinking_output_per_turn"].as_f64().unwrap();
    // 1650 / 3 = 550.0 exactly
    assert!(
        (avg - 550.0).abs() < 1e-9,
        "avg_thinking_output_per_turn should be 550.0, got {}",
        avg
    );
}

// ── D1 correctness gate ──────────────────────────────────────────────────────

#[test]
fn test_subagent_count_d1_gate() {
    // D1: only count queue-ops that carry <subagent_tokens>
    // Fixture has 2 such ops + 1 dequeue with no token tag
    // If D1 is implemented correctly: subagent_count == 2
    // If wrong (counts all queue-ops / all enqueues): subagent_count would be 3
    let summary = parse_summary_json();
    assert_eq!(
        summary["subagent_count"].as_u64().unwrap(),
        2,
        "D1: subagent_count must be 2 (only token-bearing ops count; dequeue without subagent_tokens must NOT count)"
    );
}

#[test]
fn test_subagent_tokens_d1_gate() {
    // total_subagent_tokens = 5000 + 8000 = 13000
    let summary = parse_summary_json();
    assert_eq!(
        summary["total_subagent_tokens"].as_u64().unwrap(),
        13_000,
        "D1: total_subagent_tokens must be 13000 (5000 + 8000 from the two token-bearing ops)"
    );
}

// ── D2 correctness gate ──────────────────────────────────────────────────────

#[test]
fn test_user_turns_d2_gate() {
    // D2: tool_result list-content user entries must NOT count as user_turns
    // Fixture has 4 string-content human prompts + 3 tool_result list entries
    // If D2 is correct: user_turns == 4
    // If wrong (counts all non-meta user entries): user_turns would be 7
    let summary = parse_summary_json();
    assert_eq!(
        summary["user_turns"].as_u64().unwrap(),
        4,
        "D2: user_turns must be 4 (string-content only; 3 tool_result user entries must NOT count)"
    );
}

// ── Token sum assertions (dedup proof embedded) ──────────────────────────────

#[test]
fn test_total_input_tokens() {
    // Hand-computed: 5000+8000+12000+15000+18000+22000+25000+28000+32000+35000+40000+45000 = 285000
    // Dedup proof: turn 2 has 3 lines with identical usage; turn 7 has 4 lines.
    // Without dedup, input would be inflated by: 2*8000 + 3*25000 = 91000 → 376000
    // The correct answer 285000 proves dedup works.
    let summary = parse_summary_json();
    assert_eq!(
        summary["total_input_tokens"].as_u64().unwrap(),
        285_000,
        "total_input_tokens must be 285000 (dedup of multi-line groups)"
    );
}

#[test]
fn test_total_output_tokens() {
    // Hand-computed: 300+450+600+500+750+400+800+600+900+500+700+1200 = 7700
    // Without dedup: turn 2 adds 450*2=900 extra, turn 7 adds 800*3=2400 extra → 11000
    let summary = parse_summary_json();
    assert_eq!(
        summary["total_output_tokens"].as_u64().unwrap(),
        7_700,
        "total_output_tokens must be 7700 (dedup of multi-line groups)"
    );
}

#[test]
fn test_total_cache_read() {
    // Hand-computed: 0+3000+5000+8000+10000+14000+17000+20000+24000+27000+30000+35000 = 193000
    let summary = parse_summary_json();
    assert_eq!(
        summary["total_cache_read"].as_u64().unwrap(),
        193_000,
        "total_cache_read must be 193000"
    );
}

#[test]
fn test_total_cache_write() {
    // Hand-computed: 25000+0+1000+0+500+0+1000+0+500+0+1000+0 = 29000
    let summary = parse_summary_json();
    assert_eq!(
        summary["total_cache_write"].as_u64().unwrap(),
        29_000,
        "total_cache_write must be 29000"
    );
}

// ── --from-turn tests ────────────────────────────────────────────────────────
//
// From the truth table above, turns 5-12 token sums:
//   input:       18000+22000+25000+28000+32000+35000+40000+45000 = 245,000
//   output:        750+ 400+  800+  600+  900+ 500+  700+ 1200  =   5,850
//   cache_read:  10000+14000+17000+20000+24000+27000+30000+35000 = 177,000
//   cache_write:   500+    0+ 1000+    0+  500+   0+ 1000+    0  =   3,000
//   total_turns: 8
//
// Context sizes (input + cache_read + cache_write):
//   Turn 4: 15000+8000+0 = 23000
//   Turn 5: 18000+10000+500 = 28500  → burn_delta = 28500 - 23000 = 5500

fn parse_summary_json_from_turn(from_turn: u32) -> serde_json::Value {
    let path = fixture_path();
    let (code, stdout, stderr) = run_stormglass(&[
        "analyze",
        &path,
        "--json",
        "--from-turn",
        &from_turn.to_string(),
    ]);
    assert_eq!(
        code, 0,
        "stormglass exited with code {} — stderr: {}",
        code, stderr
    );
    serde_json::from_str(&stdout).expect("stdout was not valid JSON")
}

#[test]
fn test_from_turn_5_token_sums() {
    let summary = parse_summary_json_from_turn(5);
    assert_eq!(
        summary["total_turns"].as_u64().unwrap(),
        8,
        "--from-turn 5: total_turns should be 8"
    );
    assert_eq!(
        summary["total_input_tokens"].as_u64().unwrap(),
        245_000,
        "--from-turn 5: total_input_tokens should be 245000"
    );
    assert_eq!(
        summary["total_output_tokens"].as_u64().unwrap(),
        5_850,
        "--from-turn 5: total_output_tokens should be 5850"
    );
    assert_eq!(
        summary["total_cache_read"].as_u64().unwrap(),
        177_000,
        "--from-turn 5: total_cache_read should be 177000"
    );
    assert_eq!(
        summary["total_cache_write"].as_u64().unwrap(),
        3_000,
        "--from-turn 5: total_cache_write should be 3000"
    );
}

#[test]
fn test_from_turn_1_equals_full_session() {
    // --from-turn 1 must produce identical output to no flag
    let summary_default = parse_summary_json();
    let summary_from1 = parse_summary_json_from_turn(1);

    assert_eq!(
        summary_default["total_turns"], summary_from1["total_turns"],
        "--from-turn 1: total_turns must match default"
    );
    assert_eq!(
        summary_default["total_input_tokens"], summary_from1["total_input_tokens"],
        "--from-turn 1: total_input_tokens must match default"
    );
    assert_eq!(
        summary_default["total_output_tokens"], summary_from1["total_output_tokens"],
        "--from-turn 1: total_output_tokens must match default"
    );
    assert_eq!(
        summary_default["total_cache_read"], summary_from1["total_cache_read"],
        "--from-turn 1: total_cache_read must match default"
    );
    assert_eq!(
        summary_default["total_cache_write"], summary_from1["total_cache_write"],
        "--from-turn 1: total_cache_write must match default"
    );
}

#[test]
fn test_from_turn_999_graceful() {
    // --from-turn 999: no turns in range — must exit 0 with "0 turns — no telemetry"
    let path = fixture_path();
    let (code, stdout, stderr) = run_stormglass(&["analyze", &path, "--from-turn", "999"]);
    assert_eq!(
        code, 0,
        "--from-turn 999 must exit 0, got {} — stderr: {}",
        code, stderr
    );
    assert!(
        stdout.contains("0 turns") && stdout.contains("no telemetry"),
        "--from-turn 999: expected '0 turns — no telemetry', got: {}",
        stdout
    );
}

#[test]
fn test_from_turn_5_burn_delta_first_turn() {
    // The first included turn (turn 5) must have a non-zero, correct burn_delta.
    // burn_delta[5] = ctx[5] - ctx[4] = 28500 - 23000 = 5500
    // We use --csv to get per-turn data and parse the result.
    let path = fixture_path();

    let csv_path = {
        let mut p = std::env::temp_dir();
        p.push("stormglass_from5_burn_test.csv");
        p.to_string_lossy().to_string()
    };

    let (code, _stdout, stderr) = run_stormglass(&[
        "analyze",
        &path,
        "--from-turn",
        "5",
        "--csv",
        &csv_path,
        "--quiet",
    ]);
    assert_eq!(
        code, 0,
        "--from-turn 5 --csv failed with code {} — stderr: {}",
        code, stderr
    );

    let csv_content = std::fs::read_to_string(&csv_path).expect("could not read CSV output");

    // Find the header line to locate the turn and burn_delta column indices
    let mut lines = csv_content.lines();
    let header = lines.next().expect("CSV has no header");
    let cols: Vec<&str> = header.split(',').collect();
    let turn_col = cols
        .iter()
        .position(|&c| c == "turn")
        .expect("no 'turn' column in CSV");
    let burn_col = cols
        .iter()
        .position(|&c| c == "burn_delta")
        .expect("no 'burn_delta' column in CSV");

    // Find turn 5's row
    let turn5_row = lines
        .find(|line| {
            let fields: Vec<&str> = line.splitn(turn_col + 2, ',').collect();
            fields.get(turn_col).map(|v| v.trim()) == Some("5")
        })
        .expect("turn 5 not found in CSV");

    let fields: Vec<&str> = turn5_row.split(',').collect();
    let burn_delta: i64 = fields
        .get(burn_col)
        .expect("burn_delta column missing in turn 5 row")
        .trim()
        .parse()
        .expect("burn_delta is not a valid i64");

    assert_ne!(
        burn_delta, 0,
        "turn 5 burn_delta must be non-zero (got 0 — bug: first-slice-turn excluded)"
    );
    assert_eq!(
        burn_delta, 5500,
        "turn 5 burn_delta must be 5500 (28500 - 23000), got {}",
        burn_delta
    );

    // Clean up
    let _ = std::fs::remove_file(&csv_path);
}

// ── Sidechain exclusion proof ────────────────────────────────────────────────

#[test]
fn test_sidechain_excluded() {
    // The fixture contains msg_sidechain_xxxx with isSidechain:true and
    // usage { input:99999, output:9999, cache_read:9999, cache_write:9999 }.
    // If the sidechain entry were counted, total_input would be 384999, not 285000.
    // total_turns would be 13, not 12.
    let summary = parse_summary_json();
    assert_eq!(
        summary["total_turns"].as_u64().unwrap(),
        12,
        "sidechain entry must be excluded from turn count"
    );
    assert_eq!(
        summary["total_input_tokens"].as_u64().unwrap(),
        285_000,
        "sidechain tokens must be excluded from total_input_tokens"
    );
}
