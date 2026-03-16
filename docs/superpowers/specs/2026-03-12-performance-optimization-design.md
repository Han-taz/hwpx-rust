# hwpx-rust Performance Optimization Design

## Context

hwpx-rust is a Rust library that parses HWPX (XML-based Korean word processor) files and converts them to Markdown, HTML, and JSON. It is consumed via Python wheel (PyO3 bindings). A comprehensive audit identified 30+ performance bottlenecks and processing issues across the parser, viewer, and Python FFI layers.

### Goals
- Improve parsing throughput (reduce unnecessary allocations in hot paths)
- Reduce memory pressure (eliminate redundant clones and repeated scans)
- Improve diagnostic coverage (surface silent data loss via ParseWarnings)
- All changes must pass existing snapshot tests without output changes

### Non-Goals
- API breaking changes to Python bindings
- HWP binary parser changes (frozen)
- New features or format support

---

## Phase 1: Allocation Optimization (Low Risk, High Impact)

### 1-1. XML Event Loop String Allocation Removal

**Problem:** Every XML event triggers `String::from_utf8_lossy()` conversions (19 sites in section.rs, 19 in header.rs). These allocations occur in the hottest loop of the parser.

**Solution:** Replace String comparisons with `&[u8]` byte comparisons.

**Design rule:** Element/attribute *keys* are compared as `&[u8]` (never allocated). Attribute *values* are converted to `Cow<str>` via `unescape_value()` only when they need to be stored or parsed as numbers. All HWPX namespace prefixes (e.g., `hp:`, `hc:`) are ASCII-only, so byte comparison is semantically identical.

**Before:**
```rust
let local_name = String::from_utf8_lossy(name.as_ref());
if local_name.ends_with(":tab") || local_name == "tab" { ... }

let key = String::from_utf8_lossy(attr.key.as_ref());
let value = String::from_utf8_lossy(&attr.value);
```

**After:**
```rust
let local_name = name.as_ref();
if local_name.ends_with(b":tab") || local_name == b"tab" { ... }

let key = attr.key.as_ref();
// Only convert when the value needs to be stored or parsed
if key == b"width" {
    // unescape_value() returns Cow<str> — no allocation if no escapes
    let value = attr.unescape_value().unwrap_or_default();
    width = value.parse().unwrap_or(0);
}
```

**Files:** `parser/hwpx/section.rs` (19 sites), `parser/hwpx/header.rs` (19 sites)

**Verification:** All existing unit tests + snapshot tests must pass unchanged.

---

### 1-2. BinData HashMap Pre-indexing

**Problem:** Image rendering performs O(N) linear scan through `document.doc_info.bin_data` on every lookup (shared.rs:17-22, 33-46, html/common.rs:21-27). With N images and M bindata records, total cost is O(N*M).

**Solution:** Build a `HashMap<WORD, &BinDataEmbedding>` once at conversion entry point, pass to lookup functions.

**Before:**
```rust
pub fn get_extension_from_bindata_id(document: &HwpDocument, bindata_id: WORD) -> String {
    for record in &document.doc_info.bin_data {
        if let BinDataRecord::Embedding { embedding, .. } = record {
            if embedding.binary_data_id == bindata_id {
                return embedding.extension.clone();
            }
        }
    }
    "jpg".to_string()
}
```

**After:**
```rust
pub type BinDataIndex<'a> = HashMap<WORD, &'a BinDataEmbedding>;

pub fn build_bindata_index(document: &HwpDocument) -> BinDataIndex {
    document.doc_info.bin_data.iter()
        .filter_map(|record| {
            if let BinDataRecord::Embedding { embedding, .. } = record {
                Some((embedding.binary_data_id, embedding))
            } else { None }
        })
        .collect()
}

pub fn get_extension_from_bindata_id(index: &BinDataIndex, bindata_id: WORD) -> &str {
    index.get(&bindata_id)
        .map(|e| e.extension.as_str())
        .unwrap_or("jpg")
}
```

**Files:** `viewer/shared.rs`, `viewer/markdown/mod.rs` (entry point), `viewer/html/mod.rs` (entry point), all callers of shared.rs functions

**Verification:** Snapshot tests must produce identical output.

---

### 1-3. clone() to take()/move Conversion

**Problem:** 5 clone() calls in parser hot path, including deep clone of nested tables.

| Location | Current | Change |
|----------|---------|--------|
| section.rs:412 | `hyperlink_state.url = text.clone()` | Keep (text reused after) |
| section.rs:483 | `current_cell.current_text.clone()` | `std::mem::take()` |
| section.rs:563 | `image_ref.clone()` | `std::mem::take(&mut current_image_ref).unwrap()` |
| section.rs:671 | `run_info.text.clone()` | `std::mem::take(&mut run_info.text)` |
| section.rs:760 | `nested_table.clone()` | `std::mem::take(&mut nested_table)` |

**Verification:** Each site must be verified that the source variable is not used after take(). Unit tests confirm.

---

### 1-4. Vec Capacity Hints

**Changes:**
```rust
// container.rs:read_file() — use ZIP entry size
let mut buffer = Vec::with_capacity(file.size() as usize);

// section.rs:101
let mut sections = Vec::with_capacity(4);

// section.rs:647
let mut runs: Vec<ParaTextRun> = Vec::with_capacity(4);
```

**Risk:** None. Over-estimation wastes trivial memory; under-estimation just falls back to dynamic growth.

---

### 1-5. ParseWarnings for Silent Data Loss

**Problem:** 8 sites use `parse().unwrap_or(0)` or `unwrap_or_default()`, silently discarding parse failures. 1 site in bindata.rs silently drops failed file reads.

**Solution:** Add warnings via existing `ParseWarnings` system. Thread `&mut ParseWarnings` into parser functions.

`ParseWarning` is a struct with severity levels, constructed via `ParseWarning::warning(message)`, `ParseWarning::info(message)`, or `ParseWarning::recovered_error(message)`. Warnings are added via `ParseWarnings::push()`.

**Function signature changes required:**
- `parse_section_xml(content, index)` → `parse_section_xml(content, index, warnings: &mut ParseWarnings)`
- `parse_header_xml_content(content, doc_info)` → `parse_header_xml_content(content, doc_info, warnings: &mut ParseWarnings)`
- `parse_bindata(container, doc)` → `parse_bindata(container, doc, warnings: &mut ParseWarnings)`
- Callers in `parser/hwpx/mod.rs` pass `&mut document.warnings`

```rust
// Before
leader = value.parse().unwrap_or(0);

// After
leader = value.parse().unwrap_or_else(|_| {
    warnings.push(ParseWarning::warning(format!("Invalid tab leader value: {}", String::from_utf8_lossy(&attr.value))));
    0
});
```

**Files:** `parser/hwpx/section.rs` (8 sites), `parser/hwpx/bindata.rs` (1 site), `parser/hwpx/mod.rs` (callers)

**Ordering constraint:** Phase 1-5 modifies function signatures in section.rs and header.rs, which Phase 1-1 also modifies. Implement 1-1 first, then 1-5. Do not parallelize these two.

---

## Phase 2: Rendering Optimization (Medium Risk, Medium Impact)

### 2-1. Table Cell Sort Consolidation + chars Processing

**Problem:** `table.cells` is sorted in 3 different functions (table.rs lines 32, 172, 309). These are in separate code paths (`convert_nested_table_to_text`, `convert_table_to_html`, `convert_table_to_markdown_simple`), so a single table is sorted at most twice (once in a main path + once recursively for nested tables). Additionally, `chars().skip().take().collect()` at lines 548-552 and 578 creates O(T*B) behavior where T=text length and B=break count, due to `skip()` reiterating from the start each time.

**Solution:**
1. Sort cells once at table conversion entry point, pass sorted slice to sub-functions — eliminates re-sort on recursive nested table calls.
2. Replace `chars().skip().take().collect()` with `char_indices()` pre-computation + `&str` slicing.

**Before:**
```rust
let text_before: String = text.chars().skip(last_char_pos).take(pos - last_char_pos).collect();
```

**After:**
```rust
let byte_offsets: Vec<usize> = text.char_indices().map(|(i, _)| i).chain(std::iter::once(text.len())).collect();
let text_before = &text[byte_offsets[last_char_pos]..byte_offsets[pos]];
```

**Files:** `viewer/markdown/document/bodytext/table.rs`

---

### 2-2. HTML Converter 4-pass to 1-pass Consolidation

**Problem:** `to_html()` traverses the document 4 times: find_page_def, find_page_number_position, collect vertical positions, then render.

**Solution:** Merge first 3 passes into a single pre-scan function.

```rust
struct HtmlPreScanResult {
    page_def: Option<PageDef>,
    page_number_position: Option<PageNumberPosition>,
    para_vertical_positions: Vec<f64>,
}

fn pre_scan(document: &HwpDocument) -> HtmlPreScanResult {
    // Single traversal collecting all three results
}
```

**Files:** `viewer/html/document.rs`

**Verification:** HTML snapshot tests must produce identical output.

---

### 2-3. format!() Loop to write! Buffer

**Problem:** HTML text styling wraps text in nested format!() calls per style attribute (html/text.rs:72-160), creating N intermediate Strings for N style levels.

**Solution:** Push open/close tags to a single buffer.

**Before:**
```rust
if char_shape.attributes.italic {
    styled_text = format!("<em>{styled_text}</em>");
}
if char_shape.attributes.underline_type > 0 {
    styled_text = format!("<u>{styled_text}</u>");
}
```

**After:**
```rust
let mut buf = String::with_capacity(text.len() * 2);
// Opening tags in forward order
if italic { buf.push_str("<em>"); }
if underline { buf.push_str("<u>"); }
buf.push_str(&styled_text);
// Closing tags in REVERSE order (stack discipline — innermost closes first)
if underline { buf.push_str("</u>"); }
if italic { buf.push_str("</em>"); }
```

**Note:** Tag ordering must follow stack discipline: opening tags in forward order, closing tags in reverse. The current `format!()` chain naturally produces this via nesting, but a buffer approach requires explicit reverse ordering.

**Files:** `viewer/html/text.rs`, `viewer/markdown/document/bodytext/para_text.rs` (similar pattern)

---

### 2-4. para_text.rs CharShape Lookup Optimization

**Problem:** Linear `.find()` search through sorted shapes inside segment loop — O(M) per segment, O(S*M) total. Full `chars().collect()` creates Vec<char> for every text block.

**Solution:**
1. Replace `text.chars().collect::<Vec<char>>()` with `char_indices()` + byte offset map.
2. Replace linear `.find()` with `binary_search` on sorted positions.

**Files:** `viewer/markdown/document/bodytext/para_text.rs`

---

## Phase 3: Structural Improvements (High Risk, Long-term Impact)

### 3-1. ParagraphRecord Large Variant Boxing

**Problem:** 30-variant enum with `#[allow(clippy::large_enum_variant)]` suppressed. Largest variants (ParaText, CtrlHeader, ListHeader, ShapeComponent) contain multiple Vecs, inflating all variants to the largest size.

**Solution:** Box the 4 largest variants into separate structs.

```rust
// Before
CtrlHeader { header: CtrlHeader, children: Vec<ParagraphRecord>, paragraphs: Vec<Paragraph> }

// After
CtrlHeader(Box<CtrlHeaderData>),

struct CtrlHeaderData {
    header: CtrlHeader,
    children: Vec<ParagraphRecord>,
    paragraphs: Vec<Paragraph>,
}
```

**Targets:** `ParaText`, `CtrlHeader`, `ListHeader`, `ShapeComponent`

**Impact:** ~271 `ParagraphRecord::` match arms across the codebase must be updated. The Rust compiler enforces exhaustive matching, so all sites will produce compile errors until fixed — no silent breakage possible.

**Files:** `document/bodytext/mod.rs`, all files matching against ParagraphRecord variants

---

### 3-2. Python FFI Conversion Caching

**Problem:** Each call to `to_markdown()`, `to_html()`, `to_json()` re-traverses the entire document. Python users often call multiple methods on the same document.

**Solution:** `RefCell<HashMap<CacheKey, String>>` cache inside Document struct.

```rust
#[pyclass]
struct Document {
    inner: HwpDocument,
    cache: RefCell<HashMap<CacheKey, String>>,
}

#[derive(Hash, Eq, PartialEq)]
enum CacheKey {
    Markdown { use_html: bool, include_version: bool, image_dir: Option<String> },
    Html { include_version: bool, image_dir: Option<String> },
    Json,
    Text,
}
```

Cache is invalidated never (document is immutable after parse). `RefCell` is safe under Python GIL. Use `#[pyclass(unsendable)]` to prevent cross-thread access, which would cause `RefCell` to panic.

**Files:** `packages/hwpx-python/src/lib.rs`

---

### 3-3. get_text() Single Buffer Optimization

**Problem:** Builds `Vec<String>` with no capacity, then `.join("\n")` creates another String.

**Solution:** Write directly to single `String` buffer.

```rust
fn get_text(&self) -> String {
    let mut result = String::new();
    let mut first = true;
    for section in &self.inner.body_text.sections {
        for paragraph in &section.paragraphs {
            for record in &paragraph.records {
                if let ParagraphRecord::ParaText { text, .. } = record {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        if !first { result.push('\n'); }
                        result.push_str(trimmed);
                        first = false;
                    }
                }
            }
        }
    }
    result
}
```

**Files:** `packages/hwpx-python/src/lib.rs`

---

### 3-4. Dead Code Cleanup

Remove 29 `#[allow(dead_code)]` instances across 12 files. Truly unused code is deleted; code needed for future features moves behind `#[cfg(feature = "...")]` or `#[cfg(test)]`.

---

### 3-5. Container ZIP Listing Cache

**Problem:** `list_files()`, `file_exists()`, `get_section_files()`, `get_bindata_files()` each iterate all ZIP entries.

**Solution:** Cache file list at container creation.

```rust
pub struct HwpxContainer<'a> {
    archive: ZipArchive<Cursor<&'a [u8]>>,
    file_list: Vec<String>,  // cached at creation
}

impl<'a> HwpxContainer<'a> {
    pub fn new(data: &'a [u8]) -> Result<Self, HwpError> {
        let archive = ZipArchive::new(Cursor::new(data))?;
        let file_list: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();
        Ok(Self { archive, file_list })
    }
}
```

**Note:** Preserves existing `&'a [u8]` borrowed lifetime pattern — no ownership change.

**Files:** `parser/hwpx/container.rs`

---

## Risk Assessment

| Phase | Items | Risk | Mitigation |
|-------|-------|------|------------|
| 1 | 1-1 ~ 1-5 | Low | Byte comparisons are semantically identical; snapshot tests verify output |
| 2 | 2-1 ~ 2-4 | Medium | Rendering logic changes; snapshot tests + manual review of complex tables |
| 3 | 3-1 ~ 3-5 | High (3-1), Low (rest) | 3-1 requires all match sites updated; compiler enforces exhaustiveness |

## Testing Strategy

- All phases: `cargo test --workspace` must pass
- Phase 1-2: `cargo insta accept --workspace` only if output intentionally changes (should not)
- Phase 3-1: Compiler errors guide all match site updates; no output change
- Phase 3-2: New unit tests for cache hit/miss behavior
- All phases: `cargo clippy --all-targets --all-features -- -D warnings`

## Benchmarking

Add `criterion` benchmarks before Phase 1 begins to establish baseline and validate impact:

```toml
# crates/hwp-core/Cargo.toml
[dev-dependencies]
criterion = "0.5"

[[bench]]
name = "parse_benchmark"
harness = false
```

Benchmark targets:
- **Parse:** HWPX file parse time (end-to-end from bytes to HwpDocument)
- **Markdown conversion:** `to_markdown()` on parsed document
- **HTML conversion:** `to_html()` on parsed document
- **Text extraction:** `get_text()` equivalent

Run before/after each phase to measure actual impact. Use a representative HWPX fixture with mixed content (tables, images, styled text).
