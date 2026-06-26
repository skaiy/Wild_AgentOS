# Anchored Summary — Gliding Horse Agent OS

## Goal
Fix empty PA→DA content passing (LLM `content: null` for tool-call-only responses causes PA's plan to reach DA as empty). Build verified.

## Progress
### Done
- **Root cause identified**: LLM providers return `content: null` when `finish_reason="tool_calls"` → `raw_content = ""` → `parsed.content = ""` → all L2 `AgentTurn` nodes store empty `content` fields → `dispatch_agent` reads empty nodes → DA receives empty plan
- **All 5 `TaskResult` return paths in `exec()` fixed** with `best_content_*` fallback:
  1. Hard force-finish (line 737): uses `best_content_str` for summary/output, `best_content_iri` for archive
  2. Normal finish (line 1353): `archive_iri` points to best turn L2 node
  3. Soft-limit force-finish intercept (line 1385): `archive_iri` from `best_content_iri`
  4. PA write-tool force-finish (line 1447): `archive_iri` from `best_content_iri`
  5. Unfinished/fallthrough (line 1820): uses `best_content_str` + `best_content_iri`
- **Synthetic JSON fallback** (line 1030-1058): when LLM returns `content=null` with `tool_calls`, generates JSON like `{"content":"[工具调用] name(args)","summary":"执行: name","action":"tool_call"}` — feeds `parse_llm_response` so `parsed.content` has substantive text
- **`best_content_len/str/iri` tracking** (line 590-594): tracks the turn with longest `parsed.content` across all turns
- **Build verified**: `cargo check` passes with zero errors (rockdb C++ issue fixed by sourcing `/root/.bashrc` for correct GCC/`LD_LIBRARY_PATH`)

### Verified
- All 9 `TaskResult` return points in `exec.rs` reviewed; 4 hook-abort returns (lines 55/85/263/292) are pre-turn with no content — correct as-is
- The fix applies universally to CA→AA and DA→CA via same `exec()` + `dispatch_agent` L2-reading path
- `build_agent_md_from_step` (plan_step path) uses `step.objective` directly — unaffected
- `DispatchAgentSummaries` threshold (>20 chars) satisfied by tool-call synthetic content or best-content turn

## Task Summary
- **Completed**: Investigation → fix design → implementation → verification
- **Open issues**: None. Root cause eliminated at source (content fallback) and backed by recovery layer (best-content tracking) across all return paths
