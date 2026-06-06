# HWPX Line Segments Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Preserve HWPX `hp:linesegarray/hp:lineseg` layout data by emitting `ParagraphRecord::ParaLineSeg`.

**Architecture:** Reuse the existing `LineSegmentInfo` and `ParagraphRecord::ParaLineSeg` model. The HWPX section parser will collect line segments for the active paragraph and append the record when the paragraph is completed.

**Tech Stack:** Rust, `quick_xml`, existing HWP bodytext models, insta snapshots, `cargo test`, `cargo clippy`.

---

### Task 1: Add RED Coverage For Top-Level HWPX Line Segments

**Files:**
- Modify: `crates/hwp-core/src/parser/hwpx/section.rs`

**Step 1: Write the failing test**

Add near HWPX section parser tests:

```rust
#[test]
fn paragraph_line_segments_are_preserved() {
    let xml = wrap_section(
        r#"
        <hp:p>
            <hp:run><hp:t>abcdef</hp:t></hp:run>
            <hp:linesegarray>
                <hp:lineseg textpos="0" vertpos="10" vertsize="100" textheight="80" baseline="70" spacing="20" horzpos="30" horzsize="400" flags="393216"/>
                <hp:lineseg textpos="3" vertpos="110" vertsize="120" textheight="90" baseline="75" spacing="25" horzpos="40" horzsize="410" flags="1441792"/>
            </hp:linesegarray>
        </hp:p>
    "#,
    );

    let section = parse_test_section(&xml);

    let segments = section.paragraphs[0]
        .records
        .iter()
        .find_map(|record| match record {
            ParagraphRecord::ParaLineSeg { segments } => Some(segments),
            _ => None,
        })
        .expect("line segments should be preserved");

    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].text_start_position, 0);
    assert_eq!(segments[0].vertical_position, 10);
    assert_eq!(segments[0].line_height, 100);
    assert_eq!(segments[0].text_height, 80);
    assert_eq!(segments[0].baseline_distance, 70);
    assert_eq!(segments[0].line_spacing, 20);
    assert_eq!(segments[0].column_start_position, 30);
    assert_eq!(segments[0].segment_width, 400);
    assert!(segments[0].tag.is_first_segment_of_line);
    assert!(segments[0].tag.is_last_segment_of_line);
    assert_eq!(segments[1].text_start_position, 3);
    assert!(segments[1].tag.has_indentation);
}
```

**Step 2: Run test to verify it fails**

Run:

```bash
PATH=/Users/kevin/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/Users/kevin/.cargo/bin:/opt/homebrew/bin:$PATH cargo test -p hwp-core --locked parser::hwpx::section::tests::paragraph_line_segments_are_preserved
```

Expected: FAIL because no `ParaLineSeg` record is emitted for HWPX.

### Task 2: Implement Line Segment Collection

**Files:**
- Modify: `crates/hwp-core/src/parser/hwpx/section.rs`

**Step 1: Add parser state**

Track current top-level and table-cell line segment buffers:

```rust
let mut current_line_segments: Vec<LineSegmentInfo> = Vec::new();
let mut current_cell_line_segments: Vec<LineSegmentInfo> = Vec::new();
```

**Step 2: Add XML attribute parser helper**

Create `parse_hwpx_line_segment(...) -> Result<LineSegmentInfo, HwpError>` that maps:
- `textpos` -> `text_start_position`
- `vertpos` -> `vertical_position`
- `vertsize` -> `line_height`
- `textheight` -> `text_height`
- `baseline` -> `baseline_distance`
- `spacing` -> `line_spacing`
- `horzpos` -> `column_start_position`
- `horzsize` -> `segment_width`
- `flags` -> `LineSegmentTag::from_bits`

Use `parse_numeric_attr_or_default` and report element `hp:lineseg`.

**Step 3: Collect `hp:lineseg` events**

In `Event::Empty`, when the local name is `hp:lineseg`, parse it and push into the current paragraph buffer:
- If inside a table cell paragraph, push to `current_cell_line_segments`.
- Otherwise push to `current_line_segments`.

**Step 4: Emit `ParaLineSeg` on paragraph completion**

When creating a top-level or cell paragraph, append `ParagraphRecord::ParaLineSeg { segments }` if the relevant buffer is non-empty, then clear the buffer.

### Task 3: Add Table-Cell And Diagnostic Coverage

**Files:**
- Modify: `crates/hwp-core/src/parser/hwpx/section.rs`

**Step 1: Add table-cell preservation test**

Create a small table with one cell paragraph containing `hp:linesegarray`. Assert the cell paragraph has a `ParaLineSeg` record.

**Step 2: Add invalid numeric diagnostic test**

Use `textpos="bad"` on `hp:lineseg` and assert:
- the segment defaults `text_start_position` to `0`
- diagnostics contain `InvalidValue`
- context element is `hp:lineseg`
- context attribute is `textpos`
- context value is `bad`

**Step 3: Run targeted tests**

Run all three new tests with exact filters first.

Expected: PASS after implementation.

### Task 4: Full Verification And Snapshot Review

**Files:**
- Modify HWPX snapshots if line segment records or HTML layout now reflect fixture XML.

**Step 1: Run full hwp-core tests**

```bash
PATH=/Users/kevin/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/Users/kevin/.cargo/bin:/opt/homebrew/bin:$PATH cargo test -p hwp-core --locked
```

Expected: pass or snapshot-only diffs caused by real HWPX line segments.

**Step 2: If snapshots fail, use systematic debugging**

Confirm JSON diffs add `para_line_seg` records and HTML diffs are the corresponding `hls` line layout output before accepting them.

**Step 3: Run final gates**

```bash
PATH=/Users/kevin/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/Users/kevin/.cargo/bin:/opt/homebrew/bin:$PATH cargo fmt --all -- --check
PATH=/Users/kevin/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/Users/kevin/.cargo/bin:/opt/homebrew/bin:$PATH cargo clippy -p hwp-core --all-targets --all-features --locked -- -D warnings
python3 -m unittest discover -s packages/hwpx-python/scripts -p '*_test.py'
python3 packages/hwpx-python/scripts/check_no_production_panics.py
python3 packages/hwpx-python/scripts/check_no_production_debug_output.py
python3 packages/hwpx-python/scripts/check_no_production_todos.py
git diff --check
if rg --files | rg '\.snap\.new$'; then exit 1; else exit 0; fi
```

Expected: all commands exit `0`.
