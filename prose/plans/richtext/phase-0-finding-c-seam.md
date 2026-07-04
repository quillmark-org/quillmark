# Phase 0 — Spike C finding: seam encoding + determinism

Status: **reported.** Confirms Option A; gates the phase-2 storage cutover.
Runnable evidence: `crates/richtext-spikes/tests/spike_c_seam.rs` (5
assertions).

## Question

Does canonical RichText-JSON serialize byte-deterministically, align with the
storage serialization, and lower trivially to both backends — and where does
island-mint nondeterminism enter the content hash?

## Answer

**Confirmed on all counts. No red flag for Option A.**

### Byte-deterministic serialization

`canonical_json` (`canonical.rs`) produces identical bytes for equal values
across independent serializations, and — critically — is **insensitive to mark
and island discovery order**: shuffling the `marks`/`islands` vectors yields the
same canonical bytes (`q1_canonical_json_is_byte_deterministic`). Three
nondeterminism sources, each closed:

1. **mark/island order** (parser walk) → sorted before emit;
2. **object key order** in island `props` → recursively sorted (the workspace's
   `serde_json` `preserve_order` would otherwise leak insertion order into the
   bytes);
3. **float formatting** → not exercised by the spike's props; flagged for the
   real island types.

### One encoding, not two to keep aligned

Option A's durable claim is that content crosses the seam as the *same* canonical
bytes it is stored as. The spike models seam and storage as the one
`canonical_json` contract and asserts identity, plus that `deserialize ∘
serialize` is a fixed point on the canonical bytes
(`q2_seam_json_is_the_storage_json`). A future split (seam serializer ≠ storage
serializer) is exactly the drift this pins against — there is one encoding to
keep deterministic, not two to keep in sync.

### Dual-lower — trivial to both backends

- **`typst`**: the corpus lowers to markup by escaping text and wrapping marks
  (`#strong[..]`, `#emph[..]`). The spike projects the corpus back to markdown
  and runs the *shipped* `mark_to_typst` — the same converter the engine calls
  in `convert_content_value` — confirming the lowering is available from the
  corpus (`q3a_lowers_to_typst_markup`).
- **`pdfform`**: the lowering is `RichText.text` minus island slots — plaintext,
  marks and structure discarded (`q3b_...`). `sample_form` binds only scalars
  (`body.enabled: false`; verified by inspection of the fixture), so this path
  is never exercised by a fixture today and ships with **zero fixture churn**.

### The island-mint hash boundary

Text, marks, and lines are fully deterministic: two cold imports of the same
markdown hash identically — so **migration is mint-free** (legacy bodies hold no
islands) and the pre-1.0 cutover is a deterministic cold import.

Island IDs are minted at creation. The spike isolates the boundary: two values
identical except the minted `id` hash **differently**, and with the `id` held
fixed an island-bearing value is fully deterministic
(`q4_island_mint_is_the_only_hash_nondeterminism`). This is the single boundary
the content-hash contract must tolerate once tables ship (phase 4): hashing of
island-bearing documents inherits mint-nondeterminism; text stays deterministic
throughout. Options for phase 4 (not decided here): derive island IDs from
content, or exclude minted IDs from the content hash the way `page_hashes`
excludes spans (#801).

## No red flag

Option A serializes deterministically, aligns seam and storage on one encoding,
and dual-lowers with no `pdfform` fixture change. The phase-2 storage cutover is
unblocked. The one thing phase 2+ must carry forward: canonicalize `props`
recursively (keys, and float formatting when non-string props arrive), or the
`preserve_order` build leaks insertion order into the content hash.
