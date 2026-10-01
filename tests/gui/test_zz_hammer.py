import subprocess, time
from framework import BaseGUITestCase
from test_smoke import TablesColumnMenuSmoke

class Hammer(TablesColumnMenuSmoke):
    def test_sort_and_filter_from_the_column_menu(self):
        pass

    def test_hammer(self):
        from dogtail import rawinput
        subprocess.run(["gapplication", "action", "org.tunaos.tables", "new-document"])
        self._wait_for_a_new_document()
        self._put("A1", "30")
        self._put("A2", "10")
        self._put("A3", "20")
        for i in range(60):
            self._to_grid("A1")
            rawinput.keyCombo("<Alt>Down")
            self.wait_until(lambda: self._button("Sort Ascending" if i % 2 == 0 else "Sort Descending"), bool, description="menu")
            self._button("Sort Ascending" if i % 2 == 0 else "Sort Descending").do_action(0)
            for _ in range(5):
                self.app.findChildren(lambda c: c.roleName == "table cell")
            if self.process.poll() is not None:
                self.fail(f"crashed at iteration {i}: {self.process.returncode}")
        print("survived 60")
