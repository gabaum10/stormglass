# stormglass

Read the crystals the storm left behind.

A tool for understanding what happens inside Claude Code sessions. Reads the session transcript JSONL and produces per-turn token telemetry, thinking-block analysis, and context burn curves — so you can see exactly how a session spent its tokens.

## Install

```bash
cargo build --release
# Binary at target/release/stormglass

# Or install to PATH:
cargo install --path .
```

## Usage

### analyze — single session

```bash
# Human-readable summary (default)
stormglass analyze path/to/session.jsonl

# JSON output (machine-readable, full SessionSummary object)
stormglass analyze session.jsonl --json

# Per-turn CSV (in addition to human summary)
stormglass analyze session.jsonl --csv turns.csv

# CSV only, suppress human output
stormglass analyze session.jsonl --csv turns.csv --quiet

# JSON + CSV together
stormglass analyze session.jsonl --json --csv turns.csv
```

Example output:

```
stormglass / Session: 4ef9278c (claude-opus-4-6)
mixed session: claude-opus-4-6 (453), claude-opus-4-8 (249)

Duration: 34h 24m  |  702 turns  |  153 user prompts

Tokens
  Input:          36,563  (cache read: 352,748,076 / cache write: 7,367,076)
  Output:        772,334
  Subagent:    4,858,975  (117 tasks)

Burn
  Avg per turn:   21,406 input tokens
  Peak:           turn 87 — 142,301 tokens  (Bash x4, Write x1)
  Final context:  312,440 tokens

Thinking
  Turns with thinking: 127/133  (95.5%)

Tools (top 5)
  Bash                 142
  Read                  89
  Write                 23
  Edit                  14
  mcp__matrix__reply     8
```

### compare — side-by-side sessions

```bash
# Human table (scales to N files)
stormglass compare before.jsonl after.jsonl

# JSON array of SessionSummary objects
stormglass compare a.jsonl b.jsonl c.jsonl --json
```

## Session files

Claude Code writes session transcripts to `~/.claude/projects/<project-id>/<session-id>.jsonl`. Each line is one content block from the conversation. stormglass groups them by `message.id` and emits one row per assistant turn.

## Output formats

**Human** (default): aligned plain text, no headers, thousands-comma formatting.

**JSON** (`--json`): `serde_json` pretty-print of the `SessionSummary` struct. All fields are snake_case. Use `jq` to extract specific fields.

**CSV** (`--csv <path>`): one row per turn. Header:

```
turn,timestamp,model,input_tokens,output_tokens,cache_read,cache_write,has_thinking,
thinking_block_count,content_blocks,tool_count,tools_called,stop_reason,
cumulative_input,burn_delta,skill,elapsed_sec,tokens_per_sec
```

`tools_called` uses `;` as separator (no commas inside the cell).

## What it measures

- **input_tokens**: the non-cached portion of the context window for each turn. Add `cache_read_input_tokens` for the full context size.
- **burn_delta**: signed change in `input_tokens` from the previous turn. Positive = context grew; negative = context shrank (rare, e.g. after compaction).
- **thinking_ratio**: fraction of turns that included at least one thinking block.
- **subagent tokens**: tokens consumed by tasks launched via the agent queue (separate from the main session).
- **user_turns**: human prompt count, excluding tool result feedback.
