# HWPX Line Segments Design

## Goal

Preserve HWPX paragraph layout line segments from `hp:linesegarray/hp:lineseg` so HWPX documents can reuse the existing `LineSegmentInfo` model and HTML line-segment renderer.

## Scope

Included:
- Parse `hp:lineseg` attributes into `LineSegmentInfo`.
- Attach parsed segments to the current paragraph as `ParagraphRecord::ParaLineSeg`.
- Support top-level paragraphs and table-cell paragraphs.
- Record recovered diagnostics for invalid numeric line-segment attributes.

Excluded:
- Reworking the HTML renderer.
- Changing the binary HWP `ParaLineSeg` parser.
- Preserving non-layout `hp:linesegarray` container metadata because current fixtures do not carry such metadata.

## Architecture

HWP already has a shared `LineSegmentInfo` representation and the HTML renderer already consumes `ParagraphRecord::ParaLineSeg`. HWPX should therefore translate XML attributes directly into the existing structure instead of introducing an HWPX-specific layout model.

The section parser should collect `hp:lineseg` entries while inside a paragraph. On paragraph close, it appends one `ParaLineSeg` record after text/table/image records if any line segments were parsed. This mirrors binary HWP record ordering closely enough for renderers, while keeping paragraph text creation unchanged.

Numeric parsing should use the existing XML attribute helper and diagnostic path. Invalid values should not abort the whole section; they should fall back to `0` and emit `InvalidValue` diagnostics with element `hp:lineseg`.

## Testing

Use TDD:
- Add a focused parser test for one top-level paragraph with two `hp:lineseg` entries.
- Add a focused table-cell test proving cell paragraph line segments are preserved.
- Add an invalid numeric test for `hp:lineseg` diagnostics.
- Run full `hwp-core` tests and inspect HWPX HTML/JSON snapshots because existing HTML rendering should start using real line layout.
