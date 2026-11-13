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


class LifafaMailProvider(unittest.TestCase):
    """Lifafa searches mail, not recent files, so it has its own provider
    (lifafa/src/desktop/shell_search.rs); the same pieces must still agree."""

    DATA = os.path.join(ROOT, "lifafa", "data")
    CODE = os.path.join(ROOT, "lifafa", "src", "desktop", "shell_search.rs")

    def test_ini_service_manifest_and_code_agree(self):
        code = read(self.CODE)
        ini = configparser.ConfigParser()
        ini.read(os.path.join(self.DATA, "co.hyprlab.Hylki.search-provider.ini"))
        section = ini["Shell Search Provider"]
        self.assertEqual(section["BusName"], "co.hyprlab.Hylki")
        self.assertEqual(section["Version"], "2")
        self.assertTrue(os.path.exists(os.path.join(self.DATA, section["DesktopId"])), section["DesktopId"])
        m = re.search(r'OBJECT_PATH: &str = "(/[^"]+)"', code)
        self.assertIsNotNone(m)
        self.assertEqual(section["ObjectPath"], m.group(1))
        self.assertNotEqual(section["ObjectPath"], "/co/hyprlab/Hylki")
        service = configparser.ConfigParser()
        service.read(os.path.join(self.DATA, "co.hyprlab.Hylki.service"))
        self.assertEqual(service["D-BUS Service"]["Name"], "co.hyprlab.Hylki")
        self.assertIn("--gapplication-service", service["D-BUS Service"]["Exec"])
        manifest = read(os.path.join(ROOT, "lifafa", "co.hyprlab.Hylki.yml"))
        self.assertIn("share/gnome-shell/search-providers/co.hyprlab.Hylki.search-provider.ini", manifest)
        self.assertIn("share/dbus-1/services/co.hyprlab.Hylki.service", manifest)

    def test_the_interface_matches_the_one_the_suite_exports(self):
        def methods(path):
            source = read(path)
            xml = re.search(r'const XML: &str = r#"(.*?)"#;', source, re.S).group(1)
            interface = ET.fromstring(xml).find("interface")
            return interface.get("name"), {
                m.get("name"): [(a.get("type"), a.get("direction")) for a in m.findall("arg")]
                for m in interface.findall("method")
            }

        name, found = methods(self.CODE)
        self.assertEqual(name, "org.gnome.Shell.SearchProvider2")
        self.assertEqual(found, EXPECTED_METHODS)
        body = read(self.CODE).split("method_call")[1]
        for method in EXPECTED_METHODS:
            self.assertIn(f'"{method}"', body, method)

    def test_it_is_started_from_main_and_opens_results_as_mid_links(self):
        self.assertIn("desktop::shell_search::export(", read(os.path.join(ROOT, "lifafa", "src", "main.rs")))
        self.assertIn("mid:", read(self.CODE))


class MandelbrotRoomProvider(unittest.TestCase):
    """Mandelbrot searches rooms; its provider (mandelbrot/src/search_provider.rs)
    must still agree with its ini template, service file and meson install."""

    DATA = os.path.join(ROOT, "mandelbrot", "data")
    CODE = os.path.join(ROOT, "mandelbrot", "src", "search_provider.rs")

    def test_ini_template_service_meson_and_code_agree(self):
        code = read(self.CODE)
        text = read(os.path.join(self.DATA, "org.tunaos.mandelbrot.search-provider.ini.in"))
        ini = configparser.ConfigParser()
        ini.read_string(text)
        section = ini["Shell Search Provider"]
        self.assertEqual(section["BusName"], "@application_id@")
        self.assertEqual(section["DesktopId"], "@application_id@.desktop")
        self.assertEqual(section["Version"], "2")
        m = re.search(r'OBJECT_PATH: &str = "(/[^"]+)"', code)
        self.assertIsNotNone(m)
        self.assertEqual(section["ObjectPath"], m.group(1))
        self.assertNotEqual(section["ObjectPath"], "/org/tunaos/mandelbrot")
        service = read(os.path.join(self.DATA, "org.tunaos.mandelbrot.service.in"))
        self.assertIn("--gapplication-service", service)
        self.assertIn("Name=@application_id@", service)
        meson = read(os.path.join(self.DATA, "meson.build"))
        self.assertIn("org.tunaos.mandelbrot.search-provider.ini.in", meson)
        self.assertIn("gnome-shell/search-providers", meson)

    def test_the_interface_matches_the_one_the_suite_exports(self):
        source = read(self.CODE)
        xml = re.search(r'const XML: &str = r#"(.*?)"#;', source, re.S).group(1)
        interface = ET.fromstring(xml).find("interface")
        self.assertEqual(interface.get("name"), "org.gnome.Shell.SearchProvider2")
        found = {m.get("name"): [(a.get("type"), a.get("direction")) for a in m.findall("arg")] for m in interface.findall("method")}
        self.assertEqual(found, EXPECTED_METHODS)
        body = source.split("method_call")[1]
        for method in EXPECTED_METHODS:
            self.assertIn(f'"{method}"', body, method)

    def test_it_is_started_from_startup_and_opens_results_as_matrix_links(self):
        self.assertIn("search_provider::export(", read(os.path.join(ROOT, "mandelbrot", "src", "application.rs")))
        self.assertIn("matrix:roomid/", read(self.CODE))


if __name__ == "__main__":
    unittest.main()
