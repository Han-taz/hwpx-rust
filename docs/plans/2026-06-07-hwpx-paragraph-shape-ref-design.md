# HWPX Paragraph Shape Reference Design

## Goal

Preserve paragraph shape references from real HWPX section XML. Current fixtures use `paraPrIDRef`, while the section parser currently recognizes only `prIDRef`.

## Scope

Included:
- Parse `hp:p paraPrIDRef="N"` into `Paragraph.para_header.para_shape_id`.
- Keep existing `hp:p prIDRef="N"` support as a compatibility alias.
- Continue parsing `styleIDRef` as before.
- Record recovered diagnostics for invalid numeric values.

Excluded:
- Changing paragraph style semantics.
- Reworking paragraph layout or renderer behavior.
- Rejecting documents that use both aliases unless duplicate XML attribute validation already applies.

## Architecture

The change belongs in `parse_paragraph_shape_style_ids`, which centralizes paragraph style id parsing for top-level and table-cell paragraphs. The parser should match both `paraPrIDRef` and `prIDRef` and write the same `para_shape_id` field.

For diagnostics, the reported attribute name should match the source attribute that failed. This keeps error reports actionable for real HWPX files.

## Testing

Use TDD:
- Add a focused section parser test proving `paraPrIDRef` populates `para_shape_id`.
- Add a focused invalid-value test proving invalid `paraPrIDRef` records `InvalidValue`.
- Run full `hwp-core` tests and update HWPX JSON snapshots only if fixture paragraph shape ids now correctly appear.

