# hwpx

Python bindings for HWP/HWPX document parser.

## Installation

```bash
pip install hwpx
```

Or install from source:

```bash
# Install maturin first
pip install maturin

# Build and install
cd packages/hwpx-python
maturin develop
```

## Usage

### Parse a document

```python
import hwpx

# Parse from file path
doc = hwpx.parse_file("document.hwpx")

# Or parse from bytes
with open("document.hwpx", "rb") as f:
    doc = hwpx.parse(f.read())
```

### Convert to Markdown

```python
# Basic conversion
markdown = doc.to_markdown()
print(markdown)

# With options
markdown = doc.to_markdown(
    use_html=True,           # Use HTML tags for tables, etc.
    include_version=True,    # Include document version
    image_output_dir="./images"  # Save images to directory
)
```

### Convert to HTML

```python
html = doc.to_html()

# Save images to directory instead of base64
html = doc.to_html(image_output_dir="./images")
```

### Get plain text

```python
text = doc.get_text()
print(text)
```

### Convert to JSON

```python
json_str = doc.to_json()
print(json_str)
```

### Document properties

```python
# Get document version
print(doc.version)  # e.g., "5.1.0.1"

# Get number of sections
print(doc.section_count)
```

## Supported Formats

- **HWP 5.0**: Binary format (Hangul Word Processor)
- **HWPX**: XML-based format (OWPML standard)

## License

MIT
