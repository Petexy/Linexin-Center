#!/usr/bin/env python3
"""Exercise the native widget loader without a display or system changes.

Build the Center first (``cargo build -p linexin-center``), then run:
    python3 tests/native_failures.py [--binary target/debug/linexin-center]

Fixtures in tests/fixtures/native are compiled with plain rustc into a
temporary directory. The Center runs with ``--list`` (no GTK), explicit
``--widgets-dir`` directories and a temporary XDG_CONFIG_HOME, so installed
widgets and the user's configuration are never touched.
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


PROJECT_ROOT = Path(__file__).resolve().parent.parent
FIXTURES = Path(__file__).resolve().parent / "fixtures" / "native"


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def result_text(result: subprocess.CompletedProcess[str]) -> str:
    return f"exit status: {result.returncode}\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}"


class Runner:
    def __init__(self, binary: Path, temporary: Path) -> None:
        self.binary = binary
        self.temporary = temporary
        self.built: dict[tuple, Path] = {}

    def compile(self, source: str, destination: Path, **descriptor: str) -> Path:
        """Compile fixture `source` to `destination` (a widget file name)."""
        key = (source, tuple(sorted(descriptor.items())))
        if key not in self.built:
            output = self.temporary / "build" / f"{len(self.built)}-{source}.so"
            output.parent.mkdir(parents=True, exist_ok=True)
            env = os.environ.copy()
            command = ["rustc", "--edition=2024", "--crate-type=cdylib"]
            if source == "label_widget":
                command += ["-l", "gtk-4", "-l", "gobject-2.0"]
                env.update(
                    FIXTURE_ID=descriptor.get("id", "fixture"),
                    FIXTURE_NAME=descriptor.get("name", "Fixture"),
                    FIXTURE_ICON=descriptor.get("icon", ""),
                    FIXTURE_TEXT=descriptor.get("name", "Fixture") + " content",
                )
            command += [str(FIXTURES / f"{source}.rs"), "-o", str(output)]
            result = subprocess.run(command, text=True, capture_output=True, env=env, timeout=120, check=False)
            require(result.returncode == 0, f"Compiling {source} failed:\n{result_text(result)}")
            self.built[key] = output
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(self.built[key], destination)
        return destination

    def list(self, *directories: Path, config: dict | None = None) -> subprocess.CompletedProcess[str]:
        config_home = Path(tempfile.mkdtemp(prefix="config-", dir=self.temporary))
        if config is not None:
            (config_home / "linexin").mkdir()
            (config_home / "linexin" / "linexin-center.json").write_text(json.dumps(config))
        command = [str(self.binary), "--list"]
        for directory in directories:
            command += ["--widgets-dir", str(directory)]
        env = os.environ.copy()
        env.pop("DISPLAY", None)
        env.pop("WAYLAND_DISPLAY", None)
        env["XDG_CONFIG_HOME"] = str(config_home)
        result = subprocess.run(command, text=True, capture_output=True, env=env, timeout=60, check=False)
        require(result.returncode in (0, 1), f"--list crashed:\n{result_text(result)}")
        return result


def listed(result: subprocess.CompletedProcess[str]) -> list[list[str]]:
    return [line.split("\t") for line in result.stdout.splitlines() if line]


def test_rejected_descriptors(run: Runner) -> None:
    cases = {
        "incompatible_abi": "unsupported abi version 77",
        "truncated_descriptor": "descriptor is too small",
        "null_descriptor": "null descriptor",
        "missing_symbol": "missing entry point linexin_widget_v1",
        "null_create": "no create function",
        "null_name": "field name is null",
        "bad_widget_id": "invalid widget_id",
    }
    for fixture, expected in cases.items():
        directory = run.temporary / fixture
        library = run.compile(fixture, directory / f"{fixture}.so")
        good = run.compile("label_widget", directory / "good.so", id="good", name="Good")
        result = run.list(directory)
        require(result.returncode == 1, f"{fixture}: a rejected widget must fail --list:\n{result_text(result)}")
        require(expected in result.stderr.lower(), f"{fixture}: expected {expected!r}:\n{result_text(result)}")
        require(str(library) in result.stderr, f"{fixture}: diagnostic must name the file:\n{result_text(result)}")
        rows = listed(result)
        require(
            [row[0] for row in rows] == [good.name],
            f"{fixture}: only the valid widget may be listed:\n{result_text(result)}",
        )
        print(f"PASS rejected {fixture}")


def test_valid_widget(run: Runner) -> None:
    directory = run.temporary / "valid"
    run.compile("label_widget", directory / "z-system-information-widget.so", id="system_information", name="System Information")
    result = run.list(directory)
    require(result.returncode == 0 and not result.stderr, result_text(result))
    row = listed(result)[0]
    require(
        row[:4] == ["z-system-information-widget.so", "native", "system_information", "System Information"],
        result_text(result),
    )
    print("PASS valid descriptor")


def test_first_directory_wins(run: Runner) -> None:
    first, second = run.temporary / "first", run.temporary / "second"
    run.compile("label_widget", first / "shared.so", id="first", name="First")
    run.compile("label_widget", second / "shared.so", id="second", name="Second")
    for directories, winner in (((first, second), "First"), ((second, first), "Second")):
        rows = listed(run.list(*directories))
        require(len(rows) == 1 and rows[0][3] == winner, f"expected only {winner}: {rows}")
    print("PASS the first widgets directory wins for equal file names")


def test_native_replaces_python_with_same_stem(run: Runner) -> None:
    same, native_dir, python_dir = (run.temporary / name for name in ("same", "native-dir", "python-dir"))
    run.compile("label_widget", same / "ported-widget.so", id="ported", name="Ported")
    (same / "ported-widget.py").write_text("# legacy\n")
    (same / "other.py").write_text("# legacy\n")
    rows = listed(run.list(same))
    require([row[0] for row in rows] == ["other.py", "ported-widget.so"], f"same directory: {rows}")
    run.compile("label_widget", native_dir / "ported-widget.so", id="ported", name="Ported")
    (python_dir / "ported-widget.py").parent.mkdir(parents=True, exist_ok=True)
    (python_dir / "ported-widget.py").write_text("# legacy\n")
    for directories in ((python_dir, native_dir), (native_dir, python_dir)):
        rows = listed(run.list(*directories))
        require([row[0] for row in rows] == ["ported-widget.so"], f"{directories}: {rows}")
    print("PASS a .so replaces the .py with the same stem")


def test_ordering(run: Runner) -> None:
    directory = run.temporary / "ordering"
    directory.mkdir()
    for index, name in enumerate(["c.py", "a.py", "d.py"]):
        path = directory / name
        path.write_text("# widget\n")
        os.utime(path, (1_000_000 + index, 1_000_000 + index))  # c oldest, d newest
    run.compile("label_widget", directory / "b.so", id="b", name="B")
    os.utime(directory / "b.so", (2_000_000, 2_000_000))

    names = [row[0] for row in listed(run.list(directory))]
    require(names == ["a.py", "b.so", "c.py", "d.py"], f"alphabetical without a saved order: {names}")

    # Saved by the Python package: b.py now refers to the native b.so.
    config = {"widget_order": ["d.py", "gone.py", "b.py"], "unknown": 1}
    names = [row[0] for row in listed(run.list(directory, config=config))]
    require(names == ["d.py", "b.so", "c.py", "a.py"], f"saved order, then by mtime: {names}")
    print("PASS ordering (saved order with .py -> .so mapping, then mtime; else alphabetical)")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--binary", type=Path, default=PROJECT_ROOT / "target" / "debug" / "linexin-center")
    args = parser.parse_args()
    binary = args.binary.resolve()
    if not binary.is_file():
        parser.error(f"Missing {binary}; run cargo build -p linexin-center first")
    if shutil.which("rustc") is None:
        parser.error("rustc is required to compile the native fixtures")
    with tempfile.TemporaryDirectory(prefix="linexin-native-tests-") as directory:
        run = Runner(binary, Path(directory))
        test_rejected_descriptors(run)
        test_valid_widget(run)
        test_first_directory_wins(run)
        test_native_replaces_python_with_same_stem(run)
        test_ordering(run)
    print("All native loader tests passed.")


if __name__ == "__main__":
    main()
