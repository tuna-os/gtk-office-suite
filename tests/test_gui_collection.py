"""A duplicate class or method silently removes a Python regression test."""

import ast
from pathlib import Path


def test_gui_test_definitions_are_not_shadowed():
    failures = []
    for path in (Path(__file__).parent / "gui").glob("test_*.py"):
        module = ast.parse(path.read_text(), filename=str(path))
        for scope in [module, *(node for node in ast.walk(module) if isinstance(node, ast.ClassDef))]:
            seen = {}
            for node in scope.body:
                if not isinstance(node, (ast.ClassDef, ast.FunctionDef, ast.AsyncFunctionDef)):
                    continue
                if node.name in seen:
                    failures.append(f"{path.name}:{node.lineno}: {node.name} shadows line {seen[node.name]}")
                seen[node.name] = node.lineno
    assert not failures, "\n".join(failures)
