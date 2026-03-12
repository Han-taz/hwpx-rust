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

**Problem:** Every XML event triggers `String::from_utf8_lossy()` conversions (39 sites in section.rs, 25 in header.rs). These allocations occur in the hottest loop of the parser.

**Solution:** Replace String comparisons with `&[u8]` byte comparisons.

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
// Only convert to String when the value is actually stored
if key == b"width" {
    let value = attr.unescape_value().unwrap_or_default();
    width = value.parse().unwrap_or(0);
}
```

**Files:** `parser/hwpx/section.rs` (39 sites), `parser/hwpx/header.rs` (25 sites)

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

**Solution:** Add warnings via existing `ParseWarnings` system. Pass `&mut ParseWarnings` (or equivalent) into parser functions.

```rust
// Before
leader = value.parse().unwrap_or(0);

// After
leader = value.parse().unwrap_or_else(|_| {
    warnings.add_warning(format!("Invalid tab leader value: {}", value));
    0
});
```

**Files:** `parser/hwpx/section.rs` (8 sites), `parser/hwpx/bindata.rs` (1 site)

---

## Phase 2: Rendering Optimization (Medium Risk, Medium Impact)

### 2-1. Table Cell Sort Consolidation + chars Processing

**Problem:** Same `table.cells` sorted 3 times in table.rs (lines 32, 172, 309). `chars().skip().take().collect()` creates O(N^2) behavior in nested loops (lines 548-552, 578).

**Solution:**
1. Sort cells once at table conversion entry point, pass sorted slice to sub-functions.
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
if italic { buf.push_str("<em>"); }
if underline { buf.push_str("<u>"); }
buf.push_str(&styled_text);
if underline { buf.push_str("</u>"); }
if italic { buf.push_str("</em>"); }
```

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

**Impact:** All pattern match sites across parser and viewer must be updated. No output change.

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

Cache is invalidated never (document is immutable after parse). `RefCell` is safe under Python GIL.

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
pub struct HwpxContainer {
    archive: ZipArchive<Cursor<Vec<u8>>>,
    file_list: Vec<String>,
}

impl HwpxContainer {
    pub fn new(data: &[u8]) -> Result<Self, HwpError> {
        let archive = ZipArchive::new(Cursor::new(data.to_vec()))?;
        let file_list: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();
        Ok(Self { archive, file_list })
    }
}
```

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
