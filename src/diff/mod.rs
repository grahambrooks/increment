//! The engine layer: text in, aligned rows out.
//!
//! The pipeline, in order — see `design/002-architecture-and-plan.md` §2:
//!
//! 1. `tokens` interns lines (outer diff) and unicode words (inner diff)
//!    through one interner, so one engine serves both.
//! 2. `engine` runs the histogram algorithm over those tokens (`imara-diff`),
//!    behind a trait so the choice stays replaceable.
//! 3. `align` produces the display rows: filler rows where one side has no
//!    counterpart, and — the part that makes the JetBrains view work —
//!    pairing of old and new lines inside a change block by similarity, so a
//!    pair renders as *modified* rather than as a delete next to an add.
//! 4. `inline` computes the word-level spans inside each modified pair.
//! 5. `fold` collapses runs of unchanged rows beyond the context window.
//! 6. `moves` (phase 5) recognises a block deleted here and added there as one
//!    move rather than two changes.
//!
//! The invariant every stage preserves, and which the property test asserts:
//! every line of both inputs appears exactly once across the rows. Alignment
//! silently dropping or duplicating content is the one defect that would make
//! the tool untrustworthy.
