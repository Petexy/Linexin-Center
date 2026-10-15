# Linexin Center

<p align="center">
  <img src="https://i.ibb.co/cc59HQRQ/logo.png" alt="LinexinCenter" with="200" height="200"/>
</p>

**Linexin Center** is a modular, dynamic widget loader application built with Python, GTK4, and Libadwaita. It serves as a centralized hub (control center) that dynamically loads, displays, and manages system utility widgets from a specific directory.

Designed to be the core interface for the Linexin OS/Tooling ecosystem, it features a robust localization system, safety locking for subprocesses, and a responsive user interface that respects GNOME system settings.

## 🌟 Key Features

* **Dynamic Widget Loading:** Automatically discovers and loads Python-based widgets from `/usr/share/linexin/widgets`.
* **Modern UI:** Built with GTK4 and Libadwaita for a native GNOME look and feel, featuring a responsive sidebar and split-view layout.
* **Robust Localization (L10n):** Custom localization engine that supports per-widget translation dictionaries, recursive pattern matching (e.g., handling variables inside translated strings), and dynamic text updates.
* **Safety Locking:** Automatically locks the UI and window controls when a widget executes a subprocess (via monkey-patched `subprocess` calls) to prevent user interference during critical operations.
* **Single Widget Mode:** Can be launched via command line to display a specific widget in a standalone window without the sidebar.
* **System Integration:** Respects system button layouts (close/minimize/maximize placement) and follows system dark/light mode preferences.

## 🛠️ Dependencies

To run Linexin Center, you need the following system dependencies installed:

* Python 3.8+
* GTK 4
* Libadwaita (`libadwaita-1`)
* PyGObject (`python3-gi`)

## 📂 Directory Structure

The application relies on a specific file structure to function correctly:

```text
/usr/share/linexin/
├── widgets/                    # Place widget .py files here
│   ├── localization/           # Translation files
│   │   ├── en_US/
│   │   ├── pl_PL/
│   │   └── ...
│   ├── my_utility.py
│   └── system_monitor.py
└── linexin-center.py           # Main application entry point
