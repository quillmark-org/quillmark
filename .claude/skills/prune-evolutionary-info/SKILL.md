---
name: prune-evolutionary-info
description: Rewrite comments and docs to describe the current state, not how the project got there. Use when comments narrate change ("used to", "no longer", "renamed", "removed in 0.x", "legacy") instead of stating what is.
---

## Purpose

Comments and docs should describe the **current state** of the project — not the
path from older states to it. Evolutionary narration ("we used to X, now Y")
adds words, ages badly, and makes a reader reconstruct history to understand the
present. Prune it so prose stays semantically dense.

## Scope

| Surface | Rule |
|---|---|
| Code & test comments | Prune. State the present invariant. |
| `prose/canon/` | Prune. Discuss the current high-level state. |
| `prose/references/`, `prose/proposals/` | Leave. Specs and forward-looking proposals legitimately discuss other states. |
| `docs/` (consumer-facing) | Leave unless the history is essential to *use* the feature. |
| `docs/migrations/` | **Never touch.** Era-accurate and immutable. |

## The judgment call

Not every mention of the past is evolutionary cruft. Three buckets:

1. **Pure narration → delete or restate.** "The heuristics that used to live
   here couldn't keep pace" / "removed in 0.87.0" / "we switched to X". No
   present-state value beyond what the current description already carries.

2. **Current behavior wearing a historical costume → keep the fact, drop the
   framing.** A backward-compat alias that is *still accepted* is current
   behavior. Reframe rather than delete:
   - "the legacy `~~~card-yaml` opener is still accepted but no longer canonical"
     → "`~~~card-yaml` is also accepted as a non-canonical alias"
   - "the `required` axis was removed; cell is implied by `default:`"
     → "cell status is implied by `default:` presence"

3. **Legacy load-bearing for the present → keep.** When understanding the old
   pattern is *required* to understand the current one, the history is the
   documentation. Example: a versioned-storage envelope whose whole purpose is
   to load old formats — the legacy schema *is* current reader behavior.

## Reframing moves

- "used to X, now Y" → assert Y in the present tense.
- "no longer / previously / formerly Z" → "is not Z" / "does not Z", or drop.
- "as of 0.x / removed in 0.x" → state the current rule without the version.
- Rejected-alternative rationale ("earlier API/drafts had X") → "the
  alternative X would be justified only if…" — keeps the *why*, sheds the *when*.
- Regression-test comments → state the invariant guarded, not the bug's history:
  "X must not happen, which would cause Y" instead of "X used to happen".

## Watch for

- Leave **identifiers** alone (function/test names like `..._legacy_..._rejected`)
  — they are out of scope and renaming them is churn.
- Don't delete a fact a consumer still needs (an accepted alias, a tolerated
  input) — reframe it.
- `used to` often means "used in order to" — not evolutionary. Read before cutting.

## Workflow

1. **Sweep** — grep comments/docs for: `used to`, `no longer`, `previously`,
   `formerly`, `historically`, `originally`, `renamed`, `migrated`, `replaced`,
   `removed in`, `as of`, `legacy`, `deprecated`, `we dropped`, `we switched`.
2. **Triage** each hit into the three buckets above.
3. **Rewrite** in place — present tense, minimal, keep load-bearing facts.
4. **Verify** — build/tests still pass; no test asserted the old wording.

## Done when

Comments and `prose/canon/` read as a description of what is. Backward-compat
facts survive as current-state statements. Only `docs/migrations/` (and genuine
load-bearing legacy explanations) still narrate the path here.
