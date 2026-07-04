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
  Subagent (agent files):
    input:            892,441
    output:            41,208
    cache read:     8,204,553
    cache write:      932,411
    (24 agent transcripts)

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

Claude Code writes session transcripts to `~/.claude/projects/<project-id>/<session-id>.jsonl`. Each line is one content block from the conversation. stormglass groups them by `message.id` and emits one row per assistant turn, keeping the **first** usage-bearing line per group (usage is constant across a group's blocks in the main transcript).

Subagent (Task tool) work leaves a second trail: `<session-id>/subagents/agent-<task-id>.jsonl`, one file per dispatched agent, sitting in a directory derived from the main transcript's path (strip the `.jsonl` suffix, append `/subagents`). Every line in these files carries `isSidechain:true`, which is exactly why the main transcript parse skips them — stormglass reads them in a separate pass. The grouping rule here is the *inverse* of the main transcript's: usage is still repeated per `message.id`, but `output_tokens` **grows** across a group's blocks, so stormglass keeps the **last/max** value per group instead of the first. Missing or relocated `subagents/` directories yield an all-zero split, never an error.

## Output formats

**Human** (default): aligned plain text, no headers, thousands-comma formatting.

**JSON** (`--json`): `serde_json` pretty-print of the `SessionSummary` struct. All fields are snake_case. Use `jq` to extract specific fields. New fields are appended, not inserted — existing keys and values are unchanged, so the object is not byte-identical to pre-W448 output, but nothing that existed before has moved or changed.

The subagent-file split adds six new fields:

- `subagent_input_tokens`, `subagent_output_tokens`, `subagent_cache_read_tokens`, `subagent_cache_write_tokens` — aggregated from `subagents/agent-*.jsonl`, grouped by `message.id`, last/max-wins on `output_tokens` (see "Session files" above and "What it measures" below).
- `subagent_agent_file_count` — number of `agent-*.jsonl` files that yielded at least one usable turn; the denominator behind the split.
- `subagent_usage_total_tokens` — input + output + cache_read + cache_write; an auditable checksum for the four fields above. **Not comparable** to `total_subagent_tokens` (different source, different scope — see below).

**CSV** (`--csv <path>`): one row per turn. Header:

```
turn,timestamp,model,input_tokens,output_tokens,cache_read,cache_write,context_tokens,has_thinking,
thinking_block_count,content_blocks,tool_count,tools_called,stop_reason,
cumulative_context,burn_delta,skill,elapsed_sec,tokens_per_sec
```

`tools_called` uses `;` as separator (no commas inside the cell).

## What it measures

- **input_tokens**: the non-cached portion of the context window for each turn. Add `cache_read_input_tokens` for the full context size.
- **burn_delta**: signed change in `input_tokens` from the previous turn. Positive = context grew; negative = context shrank (rare, e.g. after compaction).
- **thinking_ratio**: fraction of turns that included at least one thinking block.
- **subagent tokens — two sources, may legitimately diverge**:
  - `subagent_count` / `total_subagent_tokens` (retained, backward-compatible): parsed from queue-operation `<subagent_tokens>` notifications in the main transcript. Undifferentiated — one lump per task, no input/output/cache breakdown, and the harness can re-notify the same task-id more than once (e.g. on resume), so this count is not guaranteed 1:1 with agent files.
  - `subagent_input_tokens` / `subagent_output_tokens` / `subagent_cache_read_tokens` / `subagent_cache_write_tokens` / `subagent_agent_file_count` / `subagent_usage_total_tokens`: aggregated directly from the `subagents/agent-*.jsonl` transcripts. **Authoritative for cost extrapolation** — it's the only source that separates input from output from cache, which are priced very differently. Requires the transcript to still be in its original directory (see "Session files"); if the `subagents/` dir isn't found, these fields are 0 and a note is printed to stderr — not an error.
  - The two do not reconcile and aren't expected to: the lump excludes cache tokens entirely and measures a different thing (task-queue notifications vs. actual per-agent usage). Prefer the file-aggregated fields for cost work; keep the lump around for continuity with older tooling.
- **user_turns**: human prompt count, excluding tool result feedback.

## Out of scope / deferred

The downstream consumer of these new fields — a `session_burns` table migration (`ALTER TABLE ... +4 cols`) and a pocket `SKILL.md` writer in the `soren` repo — is a separate, later build. Docs for that side (schema.sql, SKILL.md) will lag until that follow-on lands; this README documents only stormglass's own output.
