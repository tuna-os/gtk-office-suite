"""Producer detection and link finding in tools/render-lab/publisher_pdfs.py (#1534)."""

import os
import sys
import unittest
import zlib

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "tools", "render-lab"))
import publisher_pdfs as pp  # noqa: E402


class PublisherPdfsTest(unittest.TestCase):
    def test_info_dictionary_producer(self):
        pdf = b"%PDF-1.7\n1 0 obj<</Producer(Microsoft\\256 Word for Microsoft 365)>>endobj"
        found = pp.producers(pdf)
        self.assertTrue(any(pp.OFFICE.search(f) for f in found), found)

    def test_producer_inside_compressed_stream(self):
        body = zlib.compress(b"<</Producer<FEFF004D006900630072006F0073006F00660074>>>")
        pdf = b"%PDF\nstream\n" + body + b"\nendstream"
        self.assertEqual(pp.producers(pdf), ["Microsoft"])

    def test_xmp_producer(self):
        pdf = b"<pdf:Producer>Microsoft: Print To PDF</pdf:Producer>"
        self.assertEqual(pp.producers(pdf), ["Microsoft: Print To PDF"])
        pdf = b"<xmp:CreatorTool>Microsoft Word 16</xmp:CreatorTool>"
        self.assertTrue(pp.OFFICE.search(pp.producers(pdf)[0]))

    def test_other_producers_do_not_count(self):
        for name in ("LibreOffice 7.6", "Adobe PDF Library 17", "Microsoft Print To PDF"):
            self.assertFalse(pp.OFFICE.search(name), name)

    def test_no_producer_is_empty(self):
        self.assertEqual(pp.producers(b"%PDF-1.4\n%%EOF"), [])

    def test_pdf_links_are_absolute_and_unique(self):
        html = '<a href="/a/b.PDF">x</a><a href="/a/b.PDF">y</a><a href="c.pdf?v=2">z</a><a href="d.docx">n</a>'
        self.assertEqual(
            pp.pdf_links("https://example.org/p/q", html),
            ["https://example.org/a/b.PDF", "https://example.org/p/c.pdf?v=2"],
        )


if __name__ == "__main__":
    unittest.main()
