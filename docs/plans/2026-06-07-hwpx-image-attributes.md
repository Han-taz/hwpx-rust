# HWPX Image Attributes Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Preserve `hc:img` body image attributes in the HWPX section parser without changing existing HTML or Markdown image rendering.

**Architecture:** Extend the simplified HWPX image record with optional metadata and teach `section.rs` to collect those fields while parsing `hc:img`. Keep `binary_item_ref` as the renderer identity so existing rendering code only needs pattern updates for the expanded enum fields.

**Tech Stack:** Rust, `quick_xml`, serde snapshots, `cargo test`, `cargo clippy`, Python unittest helper scripts.

---

### Task 1: Add RED Coverage For Preserved Image Attributes

**Files:**
- Modify: `crates/hwp-core/src/parser/hwpx/section.rs`

**Step 1: Write the failing test**

Add a test near the existing image tests:

```rust
#[test]
fn image_attributes_are_preserved() {
    let xml = r#"
        <hp:sec>
          <hp:p>
            <hp:run>
              <hp:pic>
                <hc:img binaryItemIDRef="image1" bright="-12" contrast="34" effect="GRAY_SCALE" alpha="128"/>
              </hp:pic>
            </hp:run>
          </hp:p>
        </hp:sec>
    "#;

    let mut warnings = ParseWarnings::new();
    let mut diagnostics = DiagnosticReport::default();
    let section = parse_section_xml(xml, 0, &mut warnings, &mut diagnostics).unwrap();

    let image = section.paragraphs.iter().find_map(|paragraph| {
        paragraph.records.iter().find_map(|record| match record {
            ParagraphRecord::HwpxImage {
                binary_item_ref,
                brightness,
                contrast,
                effect,
                alpha,
            } => Some((binary_item_ref, brightness, contrast, effect, alpha)),
            _ => None,
        })
    });

    let (binary_item_ref, brightness, contrast, effect, alpha) =
        image.expect("image record should be parsed");
    assert_eq!(binary_item_ref, "image1");
    assert_eq!(*brightness, Some(-12));
    assert_eq!(*contrast, Some(34));
    assert_eq!(effect.as_deref(), Some("GRAY_SCALE"));
    assert_eq!(*alpha, Some(128));
    assert!(diagnostics.items.is_empty());
}
```

**Step 2: Run test to verify it fails**

Run:

```bash
PATH=/Users/kevin/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/Users/kevin/.cargo/bin:/opt/homebrew/bin:$PATH cargo test -p hwp-core --locked parser::hwpx::section::tests::image_attributes_are_preserved
```

Expected: compile failure because `ParagraphRecord::HwpxImage` does not yet expose these fields.

### Task 2: Extend The Document Model And Parser

**Files:**
- Modify: `crates/hwp-core/src/document/bodytext/mod.rs`
- Modify: `crates/hwp-core/src/parser/hwpx/section.rs`
- Modify: renderer match sites that destructure `ParagraphRecord::HwpxImage`

**Step 1: Add optional fields to the record**

Update `ParagraphRecord::HwpxImage`:

```rust
HwpxImage {
    binary_item_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    brightness: Option<i16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    contrast: Option<i16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    effect: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    alpha: Option<u8>,
},
```

**Step 2: Add a small image state in `section.rs`**

Use a local struct near `CellContentItem`:

```rust
#[derive(Debug, Clone, Default)]
struct HwpxImage {
    binary_item_ref: String,
    brightness: Option<i16>,
    contrast: Option<i16>,
    effect: Option<String>,
    alpha: Option<u8>,
}
```

Change `CellContentItem::Image(String)` to `Image(HwpxImage)`, and replace `current_image_ref: Option<String>` with `current_image: Option<HwpxImage>`.

**Step 3: Parse `hc:img` attributes**

In the existing `hc:img` branch, keep `binaryItemIDRef` normalization. Also parse:
- `bright` into `brightness: Option<i16>`
- `contrast` into `contrast: Option<i16>`
- `effect` into `effect: Option<String>`
- `alpha` into `alpha: Option<u8>`

Use the existing `parse_numeric_attr_or_default` helper for recovered numeric diagnostics.

**Step 4: Update image record construction**

Change `create_image_paragraph` to accept `&HwpxImage` and emit all fields. Update paragraph and table-cell paths to pass the image state through.

**Step 5: Update renderer patterns**

At each renderer match site, change:

```rust
ParagraphRecord::HwpxImage { binary_item_ref } => { ... }
```

to:

```rust
ParagraphRecord::HwpxImage { binary_item_ref, .. } => { ... }
```

This preserves current rendering behavior.

**Step 6: Run the targeted test**

Run the Task 1 command again.

Expected: PASS.

### Task 3: Add Invalid Attribute Diagnostic Coverage

**Files:**
- Modify: `crates/hwp-core/src/parser/hwpx/section.rs`

**Step 1: Write a test**

Add a test proving invalid numeric image attributes record diagnostics without dropping a valid image reference:

```rust
#[test]
fn invalid_image_numeric_attributes_record_diagnostics() {
    let xml = r#"
        <hp:sec>
          <hp:p>
            <hp:run>
              <hp:pic>
                <hc:img binaryItemIDRef="image1" bright="bad" contrast="bad" alpha="999"/>
              </hp:pic>
            </hp:run>
          </hp:p>
        </hp:sec>
    "#;

    let mut warnings = ParseWarnings::new();
    let mut diagnostics = DiagnosticReport::default();
    let section = parse_section_xml(xml, 2, &mut warnings, &mut diagnostics).unwrap();

    assert!(section.paragraphs.iter().any(|paragraph| {
        paragraph.records.iter().any(|record| {
            matches!(record, ParagraphRecord::HwpxImage { binary_item_ref, .. } if binary_item_ref == "image1")
        })
    }));

    for attribute in ["bright", "contrast", "alpha"] {
        assert!(
            diagnostics.items.iter().any(|item| {
                item.category == DiagnosticCategory::InvalidValue
                    && item.context.section_index == Some(2)
                    && item.context.element.as_deref() == Some("hc:img")
                    && item.context.attribute.as_deref() == Some(attribute)
            }),
            "expected diagnostic for {attribute}"
        );
    }
}
```

**Step 2: Run the targeted test**

Run:

```bash
PATH=/Users/kevin/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/Users/kevin/.cargo/bin:/opt/homebrew/bin:$PATH cargo test -p hwp-core --locked parser::hwpx::section::tests::invalid_image_numeric_attributes_record_diagnostics
```

Expected: PASS after implementation.

### Task 4: Update Snapshots And Quality Gates

**Files:**
- Modify expected HWPX JSON snapshots only if new image metadata appears.

**Step 1: Run full hwp-core tests**

Run:

```bash
PATH=/Users/kevin/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/Users/kevin/.cargo/bin:/opt/homebrew/bin:$PATH cargo test -p hwp-core --locked
```

Expected: tests pass or HWPX JSON snapshots differ only by added image fields.

**Step 2: If snapshots fail, inspect root cause first**

Use `superpowers:systematic-debugging` before accepting snapshots. Confirm differences are only preserved `brightness`, `contrast`, `effect`, and `alpha`.

**Step 3: Run final gates**

Run:

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

Expected: all commands exit 0.

