# HWPX Image Attributes Design

## Goal

Preserve HWPX body image rendering attributes that are currently parsed and then lost. The immediate target is `hc:img` inside `hp:pic`, where fixture data already carries `bright`, `contrast`, `effect`, and `alpha`.

## Scope

This slice updates the document model and section parser so `ParagraphRecord::HwpxImage` can serialize optional image attributes alongside `binary_item_ref`.

Included:
- `bright` / `brightness`
- `contrast`
- `effect`
- `alpha`
- Existing unsafe `binaryItemIDRef` rejection behavior
- Existing HTML and Markdown rendering behavior

Excluded:
- `hp:pic` geometry such as `curSz`, `pos`, `imgClip`, and `imgDim`
- Applying visual filters in HTML or Markdown
- Binary HWP picture parsing changes

## Architecture

The current HWPX section parser stores a simplified image record as `ParagraphRecord::HwpxImage { binary_item_ref }`. This design extends that record with optional metadata while keeping the binary reference as the renderer-facing identity.

The parser will collect attributes from `hc:img` into a small temporary image state. When the enclosing `hp:pic` closes, the state is moved into the paragraph or table cell content item. If `binaryItemIDRef` is unsafe, the existing skip diagnostic remains authoritative and no image record is emitted.

## Error Handling

String attributes use existing XML attribute decoding. Numeric attributes use the section parser's recovered diagnostic path, matching existing handling for invalid tab, cell, and style values. Invalid `bright`, `contrast`, or `alpha` values should not abort parsing; they should record diagnostics and omit/default only the invalid field.

## Testing

Use TDD:
- Add a section parser unit test proving non-default `bright`, `contrast`, `effect`, and `alpha` survive in `ParagraphRecord::HwpxImage`.
- Add or update a test for invalid numeric image attributes to prove diagnostics are recorded without parse failure.
- Run targeted tests first, then `hwp-core` tests and quality gates.

