"""
hwpxkit - Python bindings for HWP/HWPX document parser

Example usage:
    >>> import hwpxkit
    >>>
    >>> # Parse from file path
    >>> doc = hwpxkit.parse_file("document.hwpx")
    >>>
    >>> # Or parse from bytes
    >>> with open("document.hwpx", "rb") as f:
    ...     doc = hwpxkit.parse(f.read())
    >>>
    >>> # Convert to markdown
    >>> markdown = doc.to_markdown()
    >>> print(markdown)
    >>>
    >>> # Convert to HTML
    >>> html = doc.to_html()
    >>>
    >>> # Get plain text
    >>> text = doc.get_text()
    >>>
    >>> # Convert to JSON
    >>> json_str = doc.to_json()
"""

from ._native import parse, parse_file, Document

__all__ = ["parse", "parse_file", "Document"]
__version__ = "0.2.1"
