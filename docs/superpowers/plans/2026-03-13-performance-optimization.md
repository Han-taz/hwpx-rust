# Performance Optimization Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate unnecessary allocations, redundant scans, and silent data loss across the hwpx-rust parser, viewer, and Python FFI layers.

**Architecture:** Three-phase incremental approach — allocation fixes first (low risk), rendering optimizations second (medium risk), structural changes last (high risk). Each phase is independently testable. All changes must preserve existing snapshot test output.

**Tech Stack:** Rust (quick-xml 0.37, zip 2.2, serde, PyO3 0.22), criterion 0.5 for benchmarks

**Spec:** `docs/superpowers/specs/2026-03-12-performance-optimization-design.md`

---

## File Map

### Phase 1: Allocation Optimization
| File | Action | Responsibility |
|------|--------|---------------|
| `crates/hwp-core/Cargo.toml` | Modify | Add criterion dev-dependency |
| `crates/hwp-core/benches/parse_benchmark.rs` | Create | Benchmark harness |
| `crates/hwp-core/src/parser/hwpx/section.rs` | Modify | Byte comparisons (19 sites), clone→take (2 safe sites), Vec capacity (6 sites), ParseWarnings (8 sites) |
| `crates/hwp-core/src/parser/hwpx/header.rs` | Modify | Byte comparisons (19 sites) |
| `crates/hwp-core/src/parser/hwpx/container.rs` | Modify | Vec capacity for read_file buffer |
| `crates/hwp-core/src/parser/hwpx/bindata.rs` | Modify | ParseWarnings for silent failure |
| `crates/hwp-core/src/parser/hwpx/mod.rs` | Modify | Thread &mut warnings to parser functions |
| `crates/hwp-core/src/viewer/shared.rs` | Modify | BinDataIndex type + refactored lookup functions |
| `crates/hwp-core/src/viewer/markdown/mod.rs` | Modify | Build BinDataIndex at entry point |
| `crates/hwp-core/src/viewer/markdown/document/bodytext/shape_component_picture.rs` | Modify | Accept BinDataIndex |
| `crates/hwp-core/src/viewer/html/common.rs` | Modify | Accept BinDataIndex |
| `crates/hwp-core/src/viewer/html/document.rs` | Modify | Build BinDataIndex at entry point |

### Phase 2: Rendering Optimization
| File | Action | Responsibility |
|------|--------|---------------|
| `crates/hwp-core/src/viewer/markdown/document/bodytext/table.rs` | Modify | Sort consolidation, chars→byte offset |
| `crates/hwp-core/src/viewer/html/document.rs` | Modify | 4-pass→2-pass |
| `crates/hwp-core/src/viewer/html/text.rs` | Modify | format!→buffer |
| `crates/hwp-core/src/viewer/markdown/document/bodytext/para_text.rs` | Modify | char_indices + binary search |

### Phase 3: Structural Improvements
| File | Action | Responsibility |
|------|--------|---------------|
| `crates/hwp-core/src/document/bodytext/mod.rs` | Modify | ParagraphRecord boxing (4 variants) |
| `crates/hwp-core/src/lib.rs` | Modify | Remove `large_enum_variant` suppress |
| ~22 files matching ParagraphRecord | Modify | Update match arms |
| `packages/hwpx-python/src/lib.rs` | Modify | FFI caching, get_text buffer |
| `crates/hwp-core/src/parser/hwpx/container.rs` | Modify | ZIP listing cache |
| 12 files with `#[allow(dead_code)]` | Modify | Dead code cleanup |

---

## Chunk 1: Benchmark Setup + Phase 1

### Task 0: Benchmark Setup

**Files:**
- Modify: `crates/hwp-core/Cargo.toml`
- Create: `crates/hwp-core/benches/parse_benchmark.rs`

- [ ] **Step 1: Add criterion dependency to Cargo.toml**

Add to `crates/hwp-core/Cargo.toml`:
```toml
[dev-dependencies]
insta = "1.43.2"
criterion = "0.5"

[[bench]]
name = "parse_benchmark"
harness = false
```

- [ ] **Step 2: Create benchmark harness**

Create `crates/hwp-core/benches/parse_benchmark.rs`:
```rust
use criterion::{criterion_group, criterion_main, Criterion};
use std::fs;

fn find_hwpx_fixture() -> Option<Vec<u8>> {
    // Look for any .hwpx file in test fixtures
    let paths = [
        "tests/fixtures",
        "tests/snapshots/packages",
        "tests/snapshots/crates",
    ];
    for base in paths {
        if let Ok(entries) = fs::read_dir(base) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == "hwpx").unwrap_or(false) {
                    if let Ok(data) = fs::read(&path) {
                        return Some(data);
                    }
                }
            }
        }
    }
    None
}

fn bench_parse(c: &mut Criterion) {
    if let Some(data) = find_hwpx_fixture() {
        c.bench_function("parse_hwpx", |b| {
            b.iter(|| {
                let _ = hwp_core::parser::hwpx::parse(&data);
            });
        });

        // Also benchmark conversion if parse succeeds
        if let Ok(doc) = hwp_core::parser::hwpx::parse(&data) {
            c.bench_function("to_markdown", |b| {
                let options = hwp_core::viewer::markdown::MarkdownOptions::default();
                b.iter(|| {
                    let _ = hwp_core::viewer::markdown::to_markdown(&doc, &options);
                });
            });
        }
    } else {
        eprintln!("No .hwpx fixture found — skipping benchmarks");
    }
}

criterion_group!(benches, bench_parse);
criterion_main!(benches);
```

- [ ] **Step 3: Verify benchmark compiles**

Run: `cargo bench --bench parse_benchmark -- --test`
Expected: Compiles successfully (may skip if no fixture found)

- [ ] **Step 4: Commit**

```bash
git add crates/hwp-core/Cargo.toml crates/hwp-core/benches/parse_benchmark.rs
git commit -m "perf: add criterion benchmark harness for parse and conversion"
```

---

### Task 1: XML Byte Comparisons in section.rs (Phase 1-1a)

**Files:**
- Modify: `crates/hwp-core/src/parser/hwpx/section.rs`

This task converts all 19 `String::from_utf8_lossy` calls in section.rs to `&[u8]` byte comparisons. The key rule: element/attribute **keys** stay as `&[u8]`, attribute **values** are converted via `String::from_utf8_lossy` only when they need to be stored or parsed.

- [ ] **Step 1: Convert element name comparisons (3 sites)**

Replace lines 164-165 (Empty event), 280-281 (Start event), 438-439 (End event):

**Before (each site):**
```rust
let name = e.name();
let local_name = String::from_utf8_lossy(name.as_ref());
```

**After:**
```rust
let local_name = e.name();
let local_name = local_name.as_ref();
```

Then update all comparisons on `local_name`:
- `local_name.ends_with(":tab") || local_name == "tab"` → `local_name.ends_with(b":tab") || local_name == b"tab"`
- `local_name.ends_with(":cellSpan") || ...` → `local_name.ends_with(b":cellSpan") || ...`
- All string comparisons in the match/if blocks become byte-slice comparisons
- `local_name.as_ref()` in match statements → compare directly as `&[u8]`

- [ ] **Step 2: Convert attribute parsing blocks (16 sites = 8 key+value pairs)**

For each attribute parsing block (lines ~174-175, ~224-225, ~239-240, ~255-256, ~295-296, ~322+324, ~369-370, ~396-397):

**Before:**
```rust
for attr in e.attributes().flatten() {
    let key = String::from_utf8_lossy(attr.key.as_ref());
    let value = String::from_utf8_lossy(&attr.value);
    match key.as_ref() {
        "leader" => { leader = value.parse().unwrap_or(0); }
        // ...
    }
}
```

**After:**
```rust
for attr in e.attributes().flatten() {
    let key = attr.key.as_ref();
    match key {
        b"leader" => {
            let value = String::from_utf8_lossy(&attr.value);
            leader = value.parse().unwrap_or(0);
        }
        // ... same pattern for other keys
    }
}
```

Key point: `value` conversion moves inside each match arm — only allocate when needed.

- [ ] **Step 3: Run tests**

Run: `cargo test --workspace`
Expected: All tests pass. No output changes.

- [ ] **Step 4: Run clippy**

Run: `cargo clippy --all-targets --all-features -- -D warnings`
Expected: No new warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/hwp-core/src/parser/hwpx/section.rs
git commit -m "perf: replace String::from_utf8_lossy with byte comparisons in section parser"
```

---

### Task 2: XML Byte Comparisons in header.rs (Phase 1-1b)

**Files:**
- Modify: `crates/hwp-core/src/parser/hwpx/header.rs`

Same transformation as Task 1, applied to header.rs (19 sites).

- [ ] **Step 1: Convert element name comparisons (3 sites)**

Lines ~130, ~423 (local_name), and line ~57 (version parsing).

**Line 57** special case — already uses bytes partially:
```rust
// Before:
if let Ok(v) = String::from_utf8_lossy(&attr.value).parse::<u32>() {

// After:
if let Ok(v) = std::str::from_utf8(&attr.value).ok().and_then(|s| s.parse::<u32>().ok()) {
```

**Lines 130, 423** — same pattern as section.rs:
```rust
// Before:
let local_name = String::from_utf8_lossy(name.as_ref());
// After:
let local_name = e.name();
let local_name = local_name.as_ref();
```

Then update all match arms from string to byte comparisons.

- [ ] **Step 2: Convert attribute parsing blocks (16 sites)**

Lines ~147-148, ~239-241, ~259-261, ~281-283, ~355-356, ~377-378, ~392-393, ~405-406.

Same pattern as Task 1 Step 2: move `String::from_utf8_lossy` inside each match arm for values only.

- [ ] **Step 3: Run tests**

Run: `cargo test --workspace`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/hwp-core/src/parser/hwpx/header.rs
git commit -m "perf: replace String::from_utf8_lossy with byte comparisons in header parser"
```

---

### Task 3: BinData HashMap Pre-indexing (Phase 1-2)

**Files:**
- Modify: `crates/hwp-core/src/viewer/shared.rs`
- Modify: `crates/hwp-core/src/viewer/markdown/mod.rs`
- Modify: `crates/hwp-core/src/viewer/html/document.rs`
- Modify: `crates/hwp-core/src/viewer/html/common.rs`
- Modify: `crates/hwp-core/src/viewer/markdown/document/bodytext/shape_component_picture.rs`

- [ ] **Step 1: Add BinDataIndex type and builder to shared.rs**

At the top of `crates/hwp-core/src/viewer/shared.rs`, add:
```rust
use std::collections::HashMap;
use crate::types::WORD;

/// Pre-built index for O(1) BinData lookups by binary_data_id.
pub type BinDataIndex<'a> = HashMap<WORD, &'a crate::document::docinfo::bindata::BinDataEmbedding>;

/// Build a BinData index from the document's docinfo records.
pub fn build_bindata_index(document: &crate::document::HwpDocument) -> BinDataIndex {
    use crate::document::docinfo::bindata::BinDataRecord;
    document.doc_info.bin_data.iter()
        .filter_map(|record| {
            if let BinDataRecord::Embedding { embedding, .. } = record {
                Some((embedding.binary_data_id, embedding))
            } else {
                None
            }
        })
        .collect()
}
```

- [ ] **Step 2: Refactor get_extension_from_bindata_id and get_mime_type_from_bindata_id**

Change signatures to accept `&BinDataIndex` instead of `&HwpDocument`:

```rust
pub fn get_extension_from_bindata_id(index: &BinDataIndex, bindata_id: WORD) -> &str {
    index.get(&bindata_id)
        .map(|e| e.extension.as_str())
        .unwrap_or("jpg")
}

pub fn get_mime_type_from_bindata_id(index: &BinDataIndex, bindata_id: WORD) -> &str {
    index.get(&bindata_id)
        .map(|e| match e.extension.to_lowercase().as_str() {
            "jpg" | "jpeg" => "image/jpeg",
            "png" => "image/png",
            "gif" => "image/gif",
            "bmp" => "image/bmp",
            _ => "image/jpeg",
        })
        .unwrap_or("image/jpeg")
}
```

Also update `save_image_to_file` to accept `&BinDataIndex` instead of `&HwpDocument`.

- [ ] **Step 3: Update callers — markdown entry point**

In `crates/hwp-core/src/viewer/markdown/mod.rs`, at the start of `to_markdown()`:
```rust
let bindata_index = crate::viewer::shared::build_bindata_index(document);
```
Pass `&bindata_index` through the call chain to where shared.rs functions are called.

- [ ] **Step 4: Update callers — HTML entry point and common.rs**

In `crates/hwp-core/src/viewer/html/document.rs` `to_html()`:
```rust
let bindata_index = crate::viewer::shared::build_bindata_index(document);
```

Update `html/common.rs` `get_image_url()` to accept `&BinDataIndex`.

- [ ] **Step 5: Note — shape_component_picture.rs uses different data**

`shape_component_picture.rs` looks up `document.bin_data.items` (BinaryDataItem by index/name), NOT `document.doc_info.bin_data` (BinDataRecord/BinDataEmbedding). These are different data structures — the `BinDataIndex` optimization does not apply here. Leave these lookups unchanged. The `BinDataIndex` only optimizes `shared.rs` functions that search `doc_info.bin_data` for extension/MIME type.

- [ ] **Step 6: Run tests**

Run: `cargo test --workspace`
Expected: All tests pass. Snapshot output unchanged.

- [ ] **Step 7: Commit**

```bash
git add crates/hwp-core/src/viewer/
git commit -m "perf: pre-index BinData for O(1) image lookups"
```

---

### Task 4: clone() to take()/move Conversion (Phase 1-3)

**Files:**
- Modify: `crates/hwp-core/src/parser/hwpx/section.rs`

- [ ] **Step 1: Convert 2 safe clone() sites to take()**

Only 2 of the 5 sites can safely use `take()` without signature changes. The other 3 operate through immutable borrows and must remain as `clone()`.

**Line ~483** — cell text (SAFE to take):
```rust
// Before:
.push(CellContentItem::Text(current_cell.current_text.clone()));
current_cell.current_text.clear();

// After:
.push(CellContentItem::Text(std::mem::take(&mut current_cell.current_text)));
// clear() is no longer needed — take() leaves an empty String
```

**Line ~563** — image ref (SAFE to take — restructure the if-let to consume the Option):
```rust
// Before:
if let Some(ref image_ref) = current_image_ref {
    if in_cell {
        current_cell.content_items.push(CellContentItem::Image(image_ref.clone()));
    } else {
        paragraphs.push(create_image_paragraph(image_ref));
    }
}
current_image_ref = None;

// After:
if let Some(image_ref) = std::mem::take(&mut current_image_ref) {
    if in_cell {
        current_cell.content_items.push(CellContentItem::Image(image_ref));
    } else {
        paragraphs.push(create_image_paragraph(&image_ref));
    }
}
// current_image_ref = None is now redundant (take() already set it to None)
```
Note: Both branches (in-cell and outside-table) must be preserved to avoid silently dropping images.

**Line ~671** — Keep `run_info.text.clone()`. The function `create_paragraph_with_runs` takes `text_runs: &[TextRunInfo]` (immutable borrow), so `take()` is impossible without changing the signature. The clone is on small text strings, so impact is low.

**Line ~760** — Keep `nested_table.clone()`. The function `create_table_from_rows` takes `rows: &[Vec<HwpxCell>]` (immutable borrow), so `take()` is impossible without changing the signature. Changing to owned `Vec<Vec<HwpxCell>>` would have wider ripple effects and is deferred.

**Line 412** — Keep `text.clone()` as-is (text is reused afterward).

- [ ] **Step 2: Run tests**

Run: `cargo test --workspace`
Expected: All tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/hwp-core/src/parser/hwpx/section.rs
git commit -m "perf: replace clone() with take() in section parser hot path"
```

---

### Task 5: Vec Capacity Hints (Phase 1-4)

**Files:**
- Modify: `crates/hwp-core/src/parser/hwpx/section.rs`
- Modify: `crates/hwp-core/src/parser/hwpx/container.rs`

- [ ] **Step 1: Add capacity hints in section.rs**

```rust
// Line ~101: sections typically 1-5
let mut sections = Vec::with_capacity(4);

// Line ~117: paragraphs — use moderate default
let mut paragraphs = Vec::with_capacity(16);

// Line ~141-142: table rows/cells
let mut table_rows: Vec<Vec<HwpxCell>> = Vec::with_capacity(8);
let mut current_row: Vec<HwpxCell> = Vec::with_capacity(4);

// Line ~647: runs typically 1-5 per paragraph
let mut runs: Vec<ParaTextRun> = Vec::with_capacity(4);

// Line ~731: cells
let mut cells = Vec::with_capacity(table_rows.len() * 4);
```

- [ ] **Step 2: Add capacity hint in container.rs read_file()**

```rust
// Before (line ~58):
let mut buffer = Vec::new();

// After — use ZIP entry's uncompressed size:
let mut buffer = Vec::with_capacity(file.size() as usize);
```

Note: `zip::ZipFile::size()` returns the uncompressed size, which is exactly what we need.

- [ ] **Step 3: Run tests**

Run: `cargo test --workspace`
Expected: All pass.

- [ ] **Step 4: Commit**

```bash
git add crates/hwp-core/src/parser/hwpx/section.rs crates/hwp-core/src/parser/hwpx/container.rs
git commit -m "perf: add Vec::with_capacity hints in parser hot paths"
```

---

### Task 6: ParseWarnings Threading (Phase 1-5)

**Files:**
- Modify: `crates/hwp-core/src/parser/hwpx/mod.rs`
- Modify: `crates/hwp-core/src/parser/hwpx/section.rs`
- Modify: `crates/hwp-core/src/parser/hwpx/header.rs`
- Modify: `crates/hwp-core/src/parser/hwpx/bindata.rs`

- [ ] **Step 1: Update function signatures**

**section.rs** — `parse_section_xml`:
```rust
// Before:
fn parse_section_xml(content: &str, index: WORD) -> Result<Section, HwpError>

// After:
fn parse_section_xml(content: &str, index: WORD, warnings: &mut ParseWarnings) -> Result<Section, HwpError>
```

Also update `parse_sections()` (the public function) to accept and pass warnings:
```rust
pub fn parse_sections(container: &mut HwpxContainer, warnings: &mut ParseWarnings) -> Result<BodyText, HwpError>
```

**header.rs** — `parse_doc_info`:
```rust
// Before:
pub fn parse_doc_info(container: &mut HwpxContainer) -> Result<DocInfo, HwpError>

// After:
pub fn parse_doc_info(container: &mut HwpxContainer, warnings: &mut ParseWarnings) -> Result<DocInfo, HwpError>
```

**bindata.rs** — `parse_bindata`:
```rust
// Before:
pub fn parse_bindata(container: &mut HwpxContainer) -> Result<BinData, HwpError>

// After:
pub fn parse_bindata(container: &mut HwpxContainer, warnings: &mut ParseWarnings) -> Result<BinData, HwpError>
```

- [ ] **Step 2: Update callers in mod.rs**

In `crates/hwp-core/src/parser/hwpx/mod.rs` `parse()`:
```rust
// Before:
document.doc_info = header::parse_doc_info(&mut container)?;
document.body_text = section::parse_sections(&mut container)?;
document.bin_data = bindata::parse_bindata(&mut container)?;

// After:
document.doc_info = header::parse_doc_info(&mut container, &mut document.warnings)?;
document.body_text = section::parse_sections(&mut container, &mut document.warnings)?;
document.bin_data = bindata::parse_bindata(&mut container, &mut document.warnings)?;
```

- [ ] **Step 3: Add warnings at silent-failure sites in section.rs (8 sites)**

Replace `parse().unwrap_or()` patterns:
```rust
// Lines ~178, ~181: tab attributes
leader = value.parse().unwrap_or_else(|_| {
    warnings.push(ParseWarning::warning(format!(
        "Invalid tab leader value: {}", value
    )));
    0
});

// Lines ~228, ~231: cell span
// Lines ~243, ~246: cell address
// Line ~299: prIDRef
// Line ~303: styleIDRef
// Same pattern for each
```

Add `use crate::error::{ParseWarning, ParseWarnings};` at top of section.rs.

- [ ] **Step 3b: Add warnings at silent-failure sites in header.rs**

`parse_header_xml_content` also has `unwrap_or` sites for attribute parsing (font sizes, margin values, etc.). Thread `warnings: &mut ParseWarnings` into `parse_header_xml_content` and add warnings at each `parse().unwrap_or()` site.

- [ ] **Step 4: Add warning at silent-failure site in bindata.rs**

```rust
// Before (line ~43-47):
Err(e) => {
    #[cfg(debug_assertions)]
    eprintln!("Warning: Failed to read BinData file {file_path}: {e}");
}

// After:
Err(e) => {
    warnings.push(ParseWarning::recovered_error(format!(
        "Failed to read BinData file {file_path}: {e}"
    )));
}
```

Add `use crate::error::{ParseWarning, ParseWarnings};` at top of bindata.rs.

- [ ] **Step 5: Run tests**

Run: `cargo test --workspace`
Expected: All pass. Warnings are now collected but don't change output.

- [ ] **Step 6: Commit**

```bash
git add crates/hwp-core/src/parser/hwpx/
git commit -m "perf: thread ParseWarnings through parser for silent data loss diagnostics"
```

---

## Chunk 2: Phase 2 — Rendering Optimization

### Task 7: Table Cell Sort Consolidation + chars Processing (Phase 2-1)

**Files:**
- Modify: `crates/hwp-core/src/viewer/markdown/document/bodytext/table.rs`

- [ ] **Step 1: Create a shared sort-once helper**

Add near the top of table.rs:
```rust
fn sort_cells(cells: &[crate::document::bodytext::table::Cell]) -> Vec<&crate::document::bodytext::table::Cell> {
    let mut sorted: Vec<_> = cells.iter().collect();
    sorted.sort_by_key(|cell| (
        cell.cell_attributes.row_address,
        cell.cell_attributes.col_address,
    ));
    sorted
}
```

- [ ] **Step 2: Replace sorted_cells patterns in 3 functions**

**Line ~32 (convert_nested_table_to_text):**
```rust
// Before:
let mut sorted_cells: Vec<_> = table.cells.iter().collect();
sorted_cells.sort_by_key(|cell| { ... });

// After:
let sorted_cells = sort_cells(&table.cells);
```

**Line ~172 (convert_table_to_html):** Same replacement.

**Line ~309 (convert_table_to_markdown_simple):** This one uses `.enumerate()` but the original index (`_original_idx`) is unused (prefixed with `_`). Replace the sort but note: the `else` branch at line ~341 iterates `table.cells` unsorted — only the `all_same_row` branch uses sorted cells.
```rust
// Before:
let mut sorted_cells: Vec<_> = table.cells.iter().enumerate().collect();
sorted_cells.sort_by_key(|(_, cell)| { ... });

// After:
let sorted_cells = sort_cells(&table.cells);
// The enumerate index was unused (_original_idx), so dropping it is safe
```

- [ ] **Step 3: Replace chars().skip().take().collect() with byte-offset slicing**

At lines ~548-552 and ~578 in `fill_cell_content()`, add byte offset computation before the loop:
```rust
// Before the loop that uses chars().skip().take():
let byte_offsets: Vec<usize> = text.char_indices()
    .map(|(i, _)| i)
    .chain(std::iter::once(text.len()))
    .collect();
let char_count = byte_offsets.len() - 1;

// Also replace any text.chars().count() calls inside the loop (lines ~545, ~576)
// with char_count — these are O(T) per call inside the loop.

// Then replace:
// Before:
let text_before: String = text.chars().skip(last_char_pos).take(pos.position - last_char_pos).collect();
// After:
let text_before = &text[byte_offsets[last_char_pos]..byte_offsets[pos.position]];

// Before:
let text_after: String = text.chars().skip(last_char_pos).collect();
// After:
let text_after = &text[byte_offsets[last_char_pos]..];
```

Note: Ensure `text_before` and `text_after` usages are compatible with `&str` instead of `String`. If they are `.push_str()`'d or passed to functions expecting `&str`, no change needed. If they are moved as `String`, add `.to_string()` at the usage site.

- [ ] **Step 4: Run tests**

Run: `cargo test --workspace`
Expected: All pass. Snapshot output unchanged.

- [ ] **Step 5: Commit**

```bash
git add crates/hwp-core/src/viewer/markdown/document/bodytext/table.rs
git commit -m "perf: consolidate table cell sorting and replace chars iteration with byte-offset slicing"
```

---

### Task 8: HTML Converter 4-pass to 2-pass (Phase 2-2)

**Files:**
- Modify: `crates/hwp-core/src/viewer/html/document.rs`

- [ ] **Step 1: Create HtmlPreScanResult struct and pre_scan function**

Add before `to_html()`:
```rust
use crate::document::bodytext::ctrl_header::CtrlHeaderData;

struct HtmlPreScanResult<'a> {
    page_def: Option<&'a PageDef>,
    page_number_position: Option<&'a CtrlHeaderData>,
    para_vertical_positions: Vec<f64>,
}

fn pre_scan<'a>(document: &'a HwpDocument) -> HtmlPreScanResult<'a> {
    let mut page_def = None;
    let mut page_number_position = None;
    let mut para_vertical_positions = Vec::new();

    for section in &document.body_text.sections {
        for paragraph in &section.paragraphs {
            // Collect vertical positions (skip headers/footers/footnotes)
            let control_mask = &paragraph.para_header.control_mask;
            if !control_mask.has_header_footer() && !control_mask.has_footnote_endnote() {
                if let Some(vertical_mm) = paragraph.records.iter().find_map(|record| {
                    if let ParagraphRecord::ParaLineSeg { segments } = record {
                        segments.first().map(|seg| seg.vertical_position as f64 * 25.4 / 7200.0)
                    } else {
                        None
                    }
                }) {
                    para_vertical_positions.push(vertical_mm);
                }
            }

            // Find first PageDef and PageNumberPosition
            for record in &paragraph.records {
                if page_def.is_none() {
                    if let ParagraphRecord::PageDef { page_def: pd } = record {
                        page_def = Some(pd);
                    }
                    if let ParagraphRecord::CtrlHeader { children, .. } = record {
                        for child in children {
                            if let ParagraphRecord::PageDef { page_def: pd } = child {
                                if page_def.is_none() {
                                    page_def = Some(pd);
                                }
                            }
                        }
                    }
                }
                if page_number_position.is_none() {
                    if let ParagraphRecord::CtrlHeader { header, .. } = record {
                        if header.ctrl_id == CtrlId::PAGE_NUMBER
                            || header.ctrl_id == CtrlId::PAGE_NUMBER_POS
                        {
                            if let CtrlHeaderData::PageNumberPosition { .. } = &header.data {
                                page_number_position = Some(&header.data);
                            }
                        }
                    }
                }
            }
        }
    }

    HtmlPreScanResult { page_def, page_number_position, para_vertical_positions }
}
```

- [ ] **Step 2: Replace 3 separate scans in to_html() with pre_scan()**

```rust
// Before (lines ~88-168):
let page_def = find_page_def(document);
let page_number_position = find_page_number_position(document);
// ... later ...
let mut para_vertical_positions: Vec<f64> = Vec::new();
// ... loop collecting positions ...

// After:
let pre_scan_result = pre_scan(document);
let page_def = pre_scan_result.page_def;
let page_number_position = pre_scan_result.page_number_position;
let para_vertical_positions = pre_scan_result.para_vertical_positions;
```

Remove `find_page_def()` and `find_page_number_position()` functions (now dead code).

- [ ] **Step 3: Run tests**

Run: `cargo test --workspace`
Expected: All pass. HTML output unchanged.

- [ ] **Step 4: Commit**

```bash
git add crates/hwp-core/src/viewer/html/document.rs
git commit -m "perf: consolidate HTML converter from 4-pass to 2-pass"
```

---

### Task 9: format!() Loop to Buffer (Phase 2-3)

**Files:**
- Modify: `crates/hwp-core/src/viewer/html/text.rs`

- [ ] **Step 1: Refactor HTML style wrapping in text.rs**

Find the section (lines ~130-144) where CharShape styles are applied via sequential `format!()`. Note: bold is handled via CSS `font-weight:bold`, not tags. The tag-based styles are: **italic, underline, strikethrough, superscript, subscript** (5 styles).

```rust
// Before pattern (each style wraps in a new format!):
if italic { styled_text = format!("<em>{styled_text}</em>"); }
if underline { styled_text = format!("<u>{styled_text}</u>"); }
if strikethrough { styled_text = format!("<s>{styled_text}</s>"); }
if superscript { styled_text = format!("<sup>{styled_text}</sup>"); }
if subscript { styled_text = format!("<sub>{styled_text}</sub>"); }
```

**After — push open tags forward, close tags in reverse (spec's approach):**
```rust
let mut buf = String::with_capacity(styled_text.len() + 40);
// Opening tags in forward order
if italic { buf.push_str("<em>"); }
if underline { buf.push_str("<u>"); }
if strikethrough { buf.push_str("<s>"); }
if superscript { buf.push_str("<sup>"); }
if subscript { buf.push_str("<sub>"); }
buf.push_str(&styled_text);
// Closing tags in REVERSE order (stack discipline)
if subscript { buf.push_str("</sub>"); }
if superscript { buf.push_str("</sup>"); }
if strikethrough { buf.push_str("</s>"); }
if underline { buf.push_str("</u>"); }
if italic { buf.push_str("</em>"); }
let styled_text = buf;
```

Also apply the same optimization to `render_text_runs()` at lines ~224-238 which has the same pattern.

- [ ] **Step 2: Run tests**

Run: `cargo test --workspace`
Expected: All pass. HTML output unchanged (tag nesting order preserved).

- [ ] **Step 3: Commit**

```bash
git add crates/hwp-core/src/viewer/html/text.rs
git commit -m "perf: replace nested format!() with buffer-based HTML style wrapping"
```

---

### Task 10: para_text.rs CharShape Lookup Optimization (Phase 2-4)

**Files:**
- Modify: `crates/hwp-core/src/viewer/markdown/document/bodytext/para_text.rs`

- [ ] **Step 1: Replace text_chars Vec<char> with char_indices byte offsets (3 functions)**

There are **3 functions** that create `text_chars: Vec<char>`:
- Line ~69: `convert_text_with_char_shapes()`
- Line ~533: `convert_para_text_to_markdown_with_hyperlinks()`
- Line ~622: `convert_para_text_to_markdown_with_crossing_hyperlinks()`

Apply the same transformation to all three:

```rust
// Before (each function):
let text_chars: Vec<char> = text.chars().collect();

// After:
let byte_offsets: Vec<usize> = text.char_indices()
    .map(|(i, _)| i)
    .chain(std::iter::once(text.len()))
    .collect();
let char_count = byte_offsets.len() - 1;
```

Then replace all `text_chars[start..end].iter().collect::<String>()` with:
```rust
&text[byte_offsets[start]..byte_offsets[end]]
```

Affected sites across all 3 functions: lines ~134, ~159, ~547, ~556, ~573, ~664, ~676, ~715, ~724, ~750, ~759, ~778 and any other `text_chars` indexing.

- [ ] **Step 2: Replace linear CharShape .find() with partition_point (binary search)**

```rust
// Before (lines ~98-109):
let char_shape = sorted_shapes
    .iter()
    .find(|shape| (shape.position as usize) == start)
    .or_else(|| {
        sorted_shapes.iter().rev().find(|shape| (shape.position as usize) < start)
    })

// After — binary search on sorted positions:
let char_shape = {
    let idx = sorted_shapes.partition_point(|shape| (shape.position as usize) <= start);
    if idx > 0 {
        Some(&sorted_shapes[idx - 1])
    } else {
        None  // Match current behavior: return None when start is before all shapes
    }
};
```

`partition_point` returns the index where `start` would be inserted. The shape at `idx - 1` is the last one with `position <= start`. Fallback is `None` (matching current code's behavior when neither `find()` succeeds).

- [ ] **Step 3: Run tests**

Run: `cargo test --workspace`
Expected: All pass.

- [ ] **Step 4: Commit**

```bash
git add crates/hwp-core/src/viewer/markdown/document/bodytext/para_text.rs
git commit -m "perf: use byte-offset slicing and binary search for CharShape lookups"
```

---

## Chunk 3: Phase 3 — Structural Improvements

### Task 11: ParagraphRecord Large Variant Boxing (Phase 3-1)

**Files:**
- Modify: `crates/hwp-core/src/document/bodytext/mod.rs`
- Modify: `crates/hwp-core/src/lib.rs` (remove `large_enum_variant` suppress)
- Modify: ~22 files with ParagraphRecord match arms

This is the highest-risk task. The Rust compiler will guide all required changes via exhaustive match errors.

- [ ] **Step 1: Define data structs for 4 large variants**

In `crates/hwp-core/src/document/bodytext/mod.rs`, add before the ParagraphRecord enum:

```rust
/// Data for ParaText variant (boxed to reduce enum size)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParaTextData {
    pub text: String,
    pub runs: Vec<ParaTextRun>,
    pub control_char_positions: Vec<ControlCharPosition>,
    pub inline_control_params: Vec<InlineControlParam>,
}

/// Data for CtrlHeader variant (boxed to reduce enum size)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CtrlHeaderRecordData {
    pub header: CtrlHeader,
    pub children: Vec<ParagraphRecord>,
    pub paragraphs: Vec<Paragraph>,
}

/// Data for ListHeader variant (boxed to reduce enum size)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListHeaderRecordData {
    pub header: ListHeader,
    pub paragraphs: Vec<Paragraph>,
}

/// Data for ShapeComponent variant (boxed to reduce enum size)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShapeComponentRecordData {
    pub shape_component: ShapeComponent,
    pub children: Vec<ParagraphRecord>,
}
```

- [ ] **Step 2: Update enum variants to use Box — MUST use struct variants (not tuple)**

**CRITICAL:** `ParagraphRecord` uses `#[serde(tag = "type")]` (internally-tagged enum). Serde's internal tagging does NOT support tuple variants — only struct variants and unit variants. Using `ParaText(Box<ParaTextData>)` will fail at compile/runtime.

Use `#[serde(flatten)]` on a boxed struct field instead:

```rust
pub enum ParagraphRecord {
    // Before:
    // ParaText { text: String, runs: Vec<ParaTextRun>, ... }
    // After:
    ParaText {
        #[serde(flatten)]
        data: Box<ParaTextData>,
    },

    // Before:
    // CtrlHeader { header: CtrlHeader, children: Vec<ParagraphRecord>, paragraphs: Vec<Paragraph> }
    // After:
    CtrlHeader {
        #[serde(flatten)]
        data: Box<CtrlHeaderRecordData>,
    },

    // Before:
    // ListHeader { header: ListHeader, paragraphs: Vec<Paragraph> }
    // After:
    ListHeader {
        #[serde(flatten)]
        data: Box<ListHeaderRecordData>,
    },

    // Before:
    // ShapeComponent { shape_component: ShapeComponent, children: Vec<ParagraphRecord> }
    // After:
    ShapeComponent {
        #[serde(flatten)]
        data: Box<ShapeComponentRecordData>,
    },

    // All other variants stay unchanged
    // ...
}
```

**Verification:** After this change, run `cargo test --workspace` and specifically check that `to_json()` output is byte-identical to before. The `#[serde(flatten)]` should produce the same JSON structure as the original named fields. If it doesn't, implement custom Serialize/Deserialize instead.

- [ ] **Step 3: Try to compile — collect all error sites**

Run: `cargo build --workspace 2>&1 | head -200`

The compiler will list every file/line where the old destructuring pattern is used. This is your work list.

- [ ] **Step 4: Fix match arms across the codebase**

For each compiler error, update the pattern:

```rust
// Before:
ParagraphRecord::ParaText { text, runs, control_char_positions, .. } => { ... }

// After:
ParagraphRecord::ParaText { data } => {
    let ParaTextData { text, runs, control_char_positions, .. } = data.as_ref();
    // ... rest unchanged
}

// Or destructure directly:
ParagraphRecord::ParaText { data } => {
    let text = &data.text;
    // ...
}
```

For construction sites:
```rust
// Before:
ParagraphRecord::ParaText { text: t, runs: r, control_char_positions: c, inline_control_params: i }

// After:
ParagraphRecord::ParaText { data: Box::new(ParaTextData { text: t, runs: r, control_char_positions: c, inline_control_params: i }) }
```

Repeat until `cargo build --workspace` compiles successfully.

- [ ] **Step 5: Remove clippy suppress in lib.rs**

In `crates/hwp-core/src/lib.rs`, remove:
```rust
#![allow(clippy::large_enum_variant)]
```

- [ ] **Step 6: Run full test suite + clippy**

Run: `cargo test --workspace && cargo clippy --all-targets --all-features -- -D warnings`
Expected: All pass, no clippy warnings.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "refactor: box large ParagraphRecord variants to reduce enum size"
```

---

### Task 12: Python FFI Caching + get_text Buffer (Phase 3-2 + 3-3)

**Files:**
- Modify: `packages/hwpx-python/src/lib.rs`

- [ ] **Step 1: Add CacheKey enum and cache field**

```rust
use std::cell::RefCell;
use std::collections::HashMap;

#[derive(Hash, Eq, PartialEq)]
enum CacheKey {
    Markdown { use_html: bool, include_version: bool, image_dir: Option<String> },
    Html { image_dir: Option<String> },  // include_version not exposed in Python API
    Json,
    Text,
}

#[pyclass(unsendable)]
struct Document {
    inner: HwpDocument,
    cache: RefCell<HashMap<CacheKey, String>>,
}
```

- [ ] **Step 2: Update Document construction sites**

In `parse()` and `parse_file()`:
```rust
// Before:
Ok(Document { inner: doc })

// After:
Ok(Document { inner: doc, cache: RefCell::new(HashMap::new()) })
```

- [ ] **Step 3: Add caching to to_markdown()**

```rust
fn to_markdown(&self, use_html: Option<bool>, include_version: Option<bool>, image_output_dir: Option<String>) -> PyResult<String> {
    let key = CacheKey::Markdown {
        use_html: use_html.unwrap_or(true),
        include_version: include_version.unwrap_or(false),
        image_dir: image_output_dir.clone(),
    };

    if let Some(cached) = self.cache.borrow().get(&key) {
        return Ok(cached.clone());
    }

    let options = MarkdownOptions { /* ... existing code ... */ };
    let result = hwp_core::viewer::markdown::to_markdown(&self.inner, &options);

    self.cache.borrow_mut().insert(key, result.clone());
    Ok(result)
}
```

- [ ] **Step 4: Add caching to to_html(), to_json(), get_text()**

Same pattern for each method. For `get_text()`, also apply the single-buffer optimization:

```rust
fn get_text(&self) -> String {
    if let Some(cached) = self.cache.borrow().get(&CacheKey::Text) {
        return cached.clone();
    }

    let mut result = String::new();
    let mut first = true;
    for section in &self.inner.body_text.sections {
        for paragraph in &section.paragraphs {
            for record in &paragraph.records {
                // Post-boxing pattern (Task 11 runs before Task 12)
                if let ParagraphRecord::ParaText { data } = record {
                    let trimmed = data.text.trim();
                    if !trimmed.is_empty() {
                        if !first { result.push('\n'); }
                        result.push_str(trimmed);
                        first = false;
                    }
                }
            }
        }
    }

    self.cache.borrow_mut().insert(CacheKey::Text, result.clone());
    result
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test --workspace`
Expected: All pass.

- [ ] **Step 6: Commit**

```bash
git add packages/hwpx-python/src/lib.rs
git commit -m "perf: add FFI conversion caching and single-buffer get_text"
```

---

### Task 13: Container ZIP Listing Cache (Phase 3-5)

**Files:**
- Modify: `crates/hwp-core/src/parser/hwpx/container.rs`

- [ ] **Step 1: Add file_list cache to HwpxContainer**

```rust
pub struct HwpxContainer<'a> {
    archive: ZipArchive<Cursor<&'a [u8]>>,
    file_list: Vec<String>,
}

impl<'a> HwpxContainer<'a> {
    pub fn open(data: &'a [u8]) -> Result<Self, HwpError> {
        let cursor = Cursor::new(data);
        let archive =
            ZipArchive::new(cursor).map_err(|e| HwpError::ZipParseError(e.to_string()))?;

        let file_list: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();

        Ok(Self { archive, file_list })
    }
```

- [ ] **Step 2: Update list_files, file_exists, get_section_files, get_bindata_files**

```rust
    pub fn list_files(&self, prefix: &str) -> Vec<String> {
        self.file_list.iter()
            .filter(|name| name.starts_with(prefix))
            .cloned()
            .collect()
    }

    pub fn file_exists(&self, path: &str) -> bool {
        self.file_list.iter().any(|name| name == path)
    }

    pub fn get_section_files(&self) -> Vec<String> {
        let mut sections: Vec<String> = self.file_list.iter()
            .filter(|name| name.starts_with("Contents/section") && name.ends_with(".xml"))
            .cloned()
            .collect();
        sections.sort_by(|a, b| {
            let num_a = extract_section_number(a).unwrap_or(0);
            let num_b = extract_section_number(b).unwrap_or(0);
            num_a.cmp(&num_b)
        });
        sections
    }

    pub fn get_bindata_files(&self) -> Vec<String> {
        self.file_list.iter()
            .filter(|name| name.starts_with("BinData/"))
            .cloned()
            .collect()
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test --workspace`
Expected: All pass.

- [ ] **Step 4: Commit**

```bash
git add crates/hwp-core/src/parser/hwpx/container.rs
git commit -m "perf: cache ZIP file listing at container creation"
```

---

### Task 14: Dead Code Cleanup (Phase 3-4)

**Files:**
- Modify: ~12 files with `#[allow(dead_code)]`

- [ ] **Step 1: Identify truly dead code vs planned-use code**

Run: `cargo clippy --all-targets --all-features 2>&1 | grep dead_code` (after removing all `#[allow(dead_code)]`)

Categorize each of the 29+ instances:
- **Delete:** Functions/structs that are never called and have no clear future use
- **Keep with `#[cfg(test)]`:** Test-only utilities
- **Keep with clear comment:** Functions planned for future features (e.g., hyperlink rendering)

Key candidates for deletion (from audit):
- `parser/hwpx/bindata.rs`: `get_extension()`, `get_mime_type()` — duplicated by `viewer/shared.rs`
- `viewer/html/styles.rs`: `mm_to_int32()`, `colorref_to_rgb()` — unused utilities
- `viewer/html/text.rs`: `FIELD_CONTENT_START`, `HtmlHyperlinkRegion` — incomplete feature code

- [ ] **Step 2: Remove dead code and `#[allow(dead_code)]` attributes**

Remove each `#[allow(dead_code)]` and either delete the code or move behind `#[cfg(test)]`.

- [ ] **Step 3: Compile and run tests**

Run: `cargo test --workspace && cargo clippy --all-targets --all-features -- -D warnings`
Expected: All pass, no dead_code warnings.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "cleanup: remove dead code and #[allow(dead_code)] suppressions"
```

---

## Final Verification

After all tasks complete:

- [ ] **Run full test suite:** `cargo test --workspace`
- [ ] **Run clippy:** `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] **Run benchmarks (if fixtures exist):** `cargo bench --bench parse_benchmark`
- [ ] **Verify snapshots:** `cargo insta test --workspace` (should report 0 changes)
- [ ] **Build Python wheel:** `cd packages/hwpx-python && maturin build --release`
