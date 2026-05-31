# stormglass

Read the crystals the storm left behind.

A tool for understanding what happens inside Claude Code sessions. Reads the session transcript JSONL and produces per-turn token telemetry, thinking-block analysis, and context burn curves.

## Usage

```bash
# Analyze a single session
stormglass analyze session.jsonl

# JSON output
stormglass analyze session.jsonl --json

# Per-turn CSV
stormglass analyze session.jsonl --csv turns.csv

# Compare sessions
stormglass compare before.jsonl after.jsonl
```

## Building

```bash
cargo build --release
```

## What it measures

- Per-turn: input/output tokens, thinking block count, cache read/write, context burn rate
- Summary: total tokens, thinking ratio, average burn rate, peak burn turn, session duration
- Comparison: side-by-side deltas across sessions

## Key implementation notes

- Streams JSONL line-by-line (handles 50MB+ sessions)
- De-duplicates by message.id (one API call = multiple JSONL entries with duplicated usage)
- Thinking tokens are folded into output_tokens (measures presence/count, not separate cost)
- Subagent tokens tracked separately from main-loop tokens
