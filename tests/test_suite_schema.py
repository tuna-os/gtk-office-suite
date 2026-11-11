#!/usr/bin/env python3
"""The shared org.tunaos.suite GSettings schema (INTEGRATION-PLAN C2): it is
well formed, every default is one of its choices, and every app's Flatpak
installs it beside its own schema."""
import json
import os
import unittest
import xml.etree.ElementTree as ET

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FLATPAK = os.path.join(ROOT, "flatpak")
SCHEMA = os.path.join(FLATPAK, "org.tunaos.suite.gschema.xml")
APPS = ("letters", "tables", "decks")


class SuiteSchema(unittest.TestCase):
    def test_schema_identity_and_path(self):
        schema = ET.parse(SCHEMA).getroot().find("schema")
        self.assertEqual(schema.get("id"), "org.tunaos.suite")
        self.assertEqual(schema.get("path"), "/org/tunaos/suite/")

    def test_defaults_are_valid_choices_and_keys_are_documented(self):
        schema = ET.parse(SCHEMA).getroot().find("schema")
        keys = schema.findall("key")
        self.assertTrue(keys)
        for key in keys:
            name = key.get("name")
            self.assertIsNotNone(key.find("summary"), name)
            self.assertIsNotNone(key.find("description"), name)
            choices = [c.get("value") for c in key.findall("choices/choice")]
            if choices:
                default = key.find("default").text.strip().strip('"')
                self.assertIn(default, choices, name)

    def test_every_app_installs_it(self):
        for app in APPS:
            with open(os.path.join(FLATPAK, f"org.tunaos.{app}.json")) as f:
                text = f.read()
            self.assertIn("org.tunaos.suite.gschema.xml", text, app)
        # The manifests are still valid JSON after the edit.
        for app in APPS:
            with open(os.path.join(FLATPAK, f"org.tunaos.{app}.json")) as f:
                json.load(f)


if __name__ == "__main__":
    unittest.main()
