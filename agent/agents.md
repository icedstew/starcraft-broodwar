# Delegation (token budget)

Opus (the main session) chooses targets, does hard reverse engineering (register-passed
arguments, data layouts), reviews diffs and commits. Bulk work goes to cheaper subagents
defined in `.claude/agents/`:

| Agent | Model | Use for | Returns |
|---|---|---|---|
| `triage` | Haiku | addresses from `--missing both`: purpose, callers, existing Rust equivalent, size | ≤8 lines per address |
| `porter` | Sonnet | port one function / small cluster, build | ≤10 lines: what, where, doubts |
| `verifier` | Haiku | suite / replay compare / AI scenario on a copied binary | one line per check |

Loop for porting:
1. `re/coverage.py ... --missing both` → pick 5–10 addresses (same file / area).
2. `triage` on them → keep "unported, trivial/small, no blocking deps" and the
   "equivalent exists" ones.
3. Opus checks claimed equivalents itself (cheap: read the cited Rust function and
   the decompile) before documenting them; `porter` takes the rest, one cluster each.
4. Opus reviews the diff (`git diff`), `verifier` runs the suite, commit with status.md.

Keep the main context small: don't paste decompiles or long outputs into it; ask agents
for summaries; read files by line range.
