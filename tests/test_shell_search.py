#!/usr/bin/env python3
"""GNOME Shell search provider data (INTEGRATION-PLAN B1): the .search-provider.ini,
the D-Bus service file, the Flatpak manifest and the code agree with each other.
A provider whose object path or bus name drifts from its ini is silently never
asked, so the pieces are checked against one another."""
import configparser
import json
import os
import re
import unittest
import xml.etree.ElementTree as ET

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FLATPAK = os.path.join(ROOT, "flatpak")
APPS = {"letters": "letters/src/main.rs", "tables": "tables/src/main.rs", "decks": "decks/src/main.rs"}  # apps that export a provider
EXPECTED_METHODS = {
    "GetInitialResultSet": [("as", "in"), ("as", "out")],
    "GetSubsearchResultSet": [("as", "in"), ("as", "in"), ("as", "out")],
    "GetResultMetas": [("as", "in"), ("aa{sv}", "out")],
    "ActivateResult": [("s", "in"), ("as", "in"), ("u", "in")],
    "LaunchSearch": [("as", "in"), ("u", "in")],
}


def read(path):
    with open(path, encoding="utf-8") as f:
        return f.read()


class ShellSearchProvider(unittest.TestCase):
    def test_ini_matches_the_app_and_the_code(self):
        for app, main_rs in APPS.items():
            app_id = f"org.tunaos.{app}"
            ini = configparser.ConfigParser()
            ini.read(os.path.join(FLATPAK, f"{app_id}.search-provider.ini"))
            section = ini["Shell Search Provider"]
            self.assertEqual(section["BusName"], app_id)
            self.assertEqual(section["Version"], "2")
            self.assertTrue(os.path.exists(os.path.join(FLATPAK, section["DesktopId"])), section["DesktopId"])
            # The object path is spelled once in the code and once in the ini.
            m = re.search(r'shell_search::export\([^;]*?"(/[^"]+)"', read(os.path.join(ROOT, main_rs)))
            self.assertIsNotNone(m, f"{main_rs} does not call shell_search::export")
            self.assertEqual(section["ObjectPath"], m.group(1))
            # Not under the application's own object path (it exports actions there).
            self.assertNotEqual(section["ObjectPath"], "/" + app_id.replace(".", "/"))

    def test_the_service_file_starts_the_app_as_a_service(self):
        for app in APPS:
            app_id = f"org.tunaos.{app}"
            service = configparser.ConfigParser()
            service.read(os.path.join(FLATPAK, f"{app_id}.service"))
            section = service["D-BUS Service"]
            self.assertEqual(section["Name"], app_id)
            # Without this a search would open a window.
            self.assertIn("--gapplication-service", section["Exec"])
            self.assertTrue(section["Exec"].startswith(app + " "), section["Exec"])

    def test_the_manifest_installs_both_files(self):
        for app in APPS:
            text = read(os.path.join(FLATPAK, f"org.tunaos.{app}.json"))
            json.loads(text)
            self.assertIn(f"share/gnome-shell/search-providers/org.tunaos.{app}.search-provider.ini", text)
            self.assertIn(f"share/dbus-1/services/org.tunaos.{app}.service", text)

    def test_the_exported_interface_is_searchprovider2(self):
        source = read(os.path.join(ROOT, "suite-common/src/shell_search.rs"))
        xml = re.search(r'const XML: &str = r#"(.*?)"#;', source, re.S).group(1)
        interface = ET.fromstring(xml).find("interface")
        self.assertEqual(interface.get("name"), "org.gnome.Shell.SearchProvider2")
        self.assertIn('pub const INTERFACE: &str = "org.gnome.Shell.SearchProvider2";', source)
        found = {m.get("name"): [(a.get("type"), a.get("direction")) for a in m.findall("arg")] for m in interface.findall("method")}
        self.assertEqual(found, EXPECTED_METHODS)
        # Every method the interface declares is handled.
        for name in EXPECTED_METHODS:
            self.assertIn(f'"{name}"', source.split("method_call")[1], name)


if __name__ == "__main__":
    unittest.main()
