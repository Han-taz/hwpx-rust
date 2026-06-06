# HWPX Paragraph Shape Reference Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Preserve real HWPX paragraph shape references by parsing `paraPrIDRef` on `hp:p`.

**Architecture:** Update the centralized `parse_paragraph_shape_style_ids` helper to accept both `paraPrIDRef` and the existing `prIDRef` alias. Keep all downstream paragraph creation paths unchanged because they already consume the returned `para_shape_id`.

**Tech Stack:** Rust, `quick_xml`, serde snapshots, `cargo test`, `cargo clippy`.

---

### Task 1: Add RED Coverage For `paraPrIDRef`

**Files:**
- Modify: `crates/hwp-core/src/parser/hwpx/section.rs`

**Step 1: Write the failing test**

Add near existing paragraph shape/style tests:

```rust
#[test]
fn para_pr_id_ref_sets_paragraph_shape_id() {
    let xml = wrap_section(
        r#"
        <hp:p paraPrIDRef="7" styleIDRef="3">
            <hp:run><hp:t>Styled paragraph</hp:t></hp:run>
        </hp:p>
    "#,
    );

    let section = parse_test_section(&xml);

    assert_eq!(section.paragraphs.len(), 1);
    assert_eq!(section.paragraphs[0].para_header.para_shape_id, 7);
    assert_eq!(section.paragraphs[0].para_header.para_style_id, 3);
}
```

**Step 2: Run test to verify it fails**

Run:

```bash
PATH=/Users/kevin/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/Users/kevin/.cargo/bin:/opt/homebrew/bin:$PATH cargo test -p hwp-core --locked parser::hwpx::section::tests::para_pr_id_ref_sets_paragraph_shape_id
```

Expected: FAIL because `para_shape_id` remains `0`.

### Task 2: Implement Alias Parsing

**Files:**
- Modify: `crates/hwp-core/src/parser/hwpx/section.rs`

**Step 1: Update `parse_paragraph_shape_style_ids`**

Change the match branch so both attributes parse into `para_shape_id`:

```rust
b"paraPrIDRef" | b"prIDRef" => {
    let attribute = if key == b"paraPrIDRef" { "paraPrIDRef" } else { "prIDRef" };
    para_shape_id = parse_numeric_attr_or_default(
        &attr,
        warnings,
        diagnostics,
        NumericAttrIssue {
            source,
            section_index,
            element: "hp:p",
            attribute,
            message_prefix: "Invalid paragraph shape reference value",
        },
        0,
    )?;
}
```

**Step 2: Run targeted test**

Run the Task 1 command again.

Expected: PASS.

### Task 3: Add Invalid `paraPrIDRef` Diagnostic Coverage

**Files:**
- Modify: `crates/hwp-core/src/parser/hwpx/section.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn invalid_para_pr_id_ref_records_diagnostic() {
    let xml = wrap_section(
        r#"
        <hp:p paraPrIDRef="bad" styleIDRef="2">
            <hp:run><hp:t>Styled paragraph</hp:t></hp:run>
        </hp:p>
    "#,
    );
    let mut warnings = ParseWarnings::new();
    let mut diagnostics = DiagnosticReport::default();

    let section = parse_section_xml(&xml, 6, &mut warnings, &mut diagnostics).unwrap();

    assert_eq!(section.paragraphs[0].para_header.para_shape_id, 0);
    assert_eq!(section.paragraphs[0].para_header.para_style_id, 2);
    assert!(diagnostics.items.iter().any(|item| {
        item.category == DiagnosticCategory::InvalidValue
            && item.context.section_index == Some(6)
            && item.context.element.as_deref() == Some("hp:p")
            && item.context.attribute.as_deref() == Some("paraPrIDRef")
            && item.context.value.as_deref() == Some("bad")
    }));
}
```

**Step 2: Run targeted test**

Run:

```bash
PATH=/Users/kevin/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/Users/kevin/.cargo/bin:/opt/homebrew/bin:$PATH cargo test -p hwp-core --locked parser::hwpx::section::tests::invalid_para_pr_id_ref_records_diagnostic
```

Expected: PASS after implementation.

### Task 4: Verify Snapshots And Quality Gates

**Files:**
- Modify HWPX JSON snapshots if real fixture paragraph shape ids now appear.

**Step 1: Run full tests**

Run:

```bash
PATH=/Users/kevin/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/Users/kevin/.cargo/bin:/opt/homebrew/bin:$PATH cargo test -p hwp-core --locked
```

Expected: pass or snapshot-only diffs showing paragraph shape ids changed from `0` to fixture `paraPrIDRef` values.

**Step 2: If snapshots fail, use systematic debugging**

Confirm differences are only `para_shape_id` improvements before accepting them.

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

