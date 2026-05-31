// Adversarial tests — correctness guards for known edge cases.
// All bugs in the original roach pressure-test are now fixed; #[ignore] removed.
//
// Cases covered:
//   F1  multibyte sessionId no longer panics (output.rs + compare.rs)
//   F2  directory input exits non-zero promptly (no infinite loop)
//   F3  token-sum overflow saturates (no wrap/panic)

use std::io::Write;
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_stormglass")
}

fn write_tmp(name: &str, contents: &str) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("sg_adv_{}_{}", std::process::id(), name));
    let mut f = std::fs::File::create(&p).unwrap();
    f.write_all(contents.as_bytes()).unwrap();
    p
}

// F1: a sessionId whose 8th byte falls inside a multibyte UTF-8 char panicked
// the human formatter (output.rs) and compare header (compare.rs).
// "aaaaaaa\u{e9}bbbb" -> the 'é' is 2 bytes, so byte-index 8 is mid-character.
// Fixed by using chars().take(8). Both human output and compare must not panic.
#[test]
fn f1_multibyte_session_id_analyze() {
    let line = r#"{"type":"assistant","message":{"id":"m1","model":"x","usage":{"input_tokens":1,"output_tokens":1,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"text","text":"h"}]},"timestamp":"2026-05-29T10:00:00.000Z","sessionId":"aaaaaaaébbbb"}"#;
    let path = write_tmp("f1_analyze.jsonl", &format!("{}\n", line));
    let out = Command::new(bin())
        .arg("analyze")
        .arg(&path)
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(
        out.status.success(),
        "F1 (analyze): multibyte sessionId must not crash. stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn f1_multibyte_session_id_compare() {
    let line = r#"{"type":"assistant","message":{"id":"m1","model":"x","usage":{"input_tokens":1,"output_tokens":1,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"text","text":"h"}]},"timestamp":"2026-05-29T10:00:00.000Z","sessionId":"aaaaaaaébbbb"}"#;
    let path = write_tmp("f1_compare.jsonl", &format!("{}\n", line));
    let out = Command::new(bin())
        .arg("compare")
        .arg(&path)
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(
        out.status.success(),
        "F1 (compare): multibyte sessionId must not crash. stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// F2: passing a directory to `analyze` previously caused an infinite loop.
// Fixed by checking metadata().is_file() in parse_session and breaking on read errors.
// Must exit non-zero promptly.
#[test]
fn f2_directory_input_exits_nonzero() {
    let dir = std::env::temp_dir();
    let out = Command::new(bin())
        .arg("analyze")
        .arg(dir.to_str().unwrap())
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "F2: directory input must exit non-zero. stdout: {} stderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

// F3: token sums near u64::MAX previously wrapped silently (release) or panicked
// (debug). Fixed by using saturating_add. Must exit 0 and not panic.
#[test]
fn f3_token_sum_overflow_saturates() {
    let l1 = r#"{"type":"assistant","message":{"id":"m1","model":"x","usage":{"input_tokens":18446744073709551615,"output_tokens":1,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"text","text":"h"}]},"timestamp":"2026-05-29T10:00:00.000Z","sessionId":"sess1234"}"#;
    let l2 = r#"{"type":"assistant","message":{"id":"m2","model":"x","usage":{"input_tokens":5,"output_tokens":1,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"text","text":"h"}]},"timestamp":"2026-05-29T10:00:01.000Z","sessionId":"sess1234"}"#;
    let path = write_tmp("f3.jsonl", &format!("{}\n{}\n", l1, l2));
    let out = Command::new(bin())
        .arg("analyze")
        .arg(&path)
        .arg("--json")
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(
        out.status.success(),
        "F3: u64::MAX input tokens must not panic. stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // With saturating_add, total_input_tokens must be u64::MAX (not 4 from wrap)
    let json: serde_json::Value = serde_json::from_str(
        &String::from_utf8_lossy(&out.stdout)
    ).expect("output should be valid JSON");
    assert_eq!(
        json["total_input_tokens"].as_u64().unwrap(),
        u64::MAX,
        "F3: saturating_add must clamp total_input_tokens at u64::MAX"
    );
}

// W2: tool name containing a comma must be quoted in the CSV tools_called cell
// so the row stays 18 comma-separated fields. Without the q() escaper the join
// result is written raw, splitting the row on the embedded comma → 19+ fields.
#[test]
fn w2_csv_comma_in_tool_name_stays_18_fields() {
    // A tool_use entry whose name contains a comma
    let line = r#"{"type":"assistant","message":{"id":"m1","model":"x","usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"tool_use","name":"Foo,Bar"}]},"timestamp":"2026-05-29T10:00:00.000Z","sessionId":"sess1234"}"#;
    let jsonl_path = write_tmp("w2.jsonl", &format!("{}\n", line));
    let csv_path = {
        let mut p = std::env::temp_dir();
        p.push(format!("sg_adv_{}_w2.csv", std::process::id()));
        p
    };
    let out = Command::new(bin())
        .args(["analyze", jsonl_path.to_str().unwrap(),
               "--csv", csv_path.to_str().unwrap(), "--quiet"])
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&jsonl_path);

    assert!(out.status.success(), "W2: analyze must succeed. stderr: {}",
            String::from_utf8_lossy(&out.stderr));

    let csv = std::fs::read_to_string(&csv_path).expect("CSV file should exist");
    let _ = std::fs::remove_file(&csv_path);

    // Skip header row; the data row must have exactly 18 comma-separated fields
    let data_row = csv.lines()
        .nth(1)
        .expect("CSV must have a data row after the header");

    // Count top-level comma separators (outside quoted cells) by a simple CSV field parser
    let field_count = count_csv_fields(data_row);
    assert_eq!(
        field_count, 18,
        "W2: CSV row with comma-in-tool-name must have exactly 18 fields, got {}. Row: {}",
        field_count, data_row
    );

    // Also verify the tools_called cell contains the quoted tool name
    assert!(
        data_row.contains("\"Foo,Bar\""),
        "W2: tools_called cell must be quoted when tool name contains a comma. Row: {}",
        data_row
    );
}

/// Count fields in one CSV row, respecting double-quoted cells.
fn count_csv_fields(row: &str) -> usize {
    let mut fields = 1usize;
    let mut in_quotes = false;
    let mut chars = row.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                if in_quotes && chars.peek() == Some(&'"') {
                    chars.next(); // escaped quote — skip
                } else {
                    in_quotes = !in_quotes;
                }
            }
            ',' if !in_quotes => fields += 1,
            _ => {}
        }
    }
    fields
}
