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

// ── Smoke test: empty file → "0 turns — no telemetry", exit 0 ──────────────

#[test]
fn test_empty_file_smoke() {
    let path = empty_fixture_path();
    std::fs::write(&path, "").expect("could not write empty fixture");

    let (code, stdout, _stderr) = run_stormglass(&["analyze", &path, "--json"]);
    assert_eq!(code, 0, "exit code should be 0 for empty file");
    assert!(
        stdout.contains("0 turns") && stdout.contains("no telemetry"),
        "expected '0 turns — no telemetry' in stdout, got: {}",
        stdout
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
