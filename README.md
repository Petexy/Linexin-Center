# Linexin Center

<p align="center">
  <img src="https://i.ibb.co/cc59HQRQ/logo.png" alt="Linexin Center" width="200" height="200"/>
</p>

**Linexin Center** is a modular widget loader built with **Rust**, **GTK4** and
**Libadwaita**. It is the central hub (control center) of Linexin: it finds the
system utility widgets installed on your computer, lists them in a sidebar and
shows the one you pick.

Version 4 is a complete rewrite of the original Python application. It is a
drop-in replacement: same package, same command, same look, same settings and
translations. Native widgets and the existing Python widgets work side by side,
so nothing you have installed stops working.

## 🌟 Key Features

* **Dynamic Widget Loading:** Discovers every widget in
  `/usr/share/linexin/widgets`, native ones (`.so`) and Python ones (`.py`)
  alike. The selected widget opens first; the rest load quietly in the
  background.
* **Modern UI:** GTK4 and Libadwaita with a responsive sidebar and split view,
  a compact mode for small windows, and the Kinexin look on KDE Plasma and
  Hyprland.
* **Your Order, Your Sidebar:** Edit mode lets you reorder and hide widgets and
  create desktop shortcuts to them. Your choices are kept between sessions
  (and carried over from the Python version).
* **Robust Localization:** Every widget brings its own translation
  dictionaries; the Center picks your system language (or the one you ask
  for) and falls back to English.
* **Safety Locking:** While a widget runs a privileged command, the window is
  locked so nothing interrupts a critical operation.
* **Single Widget Mode:** Open one widget on its own, without the sidebar,
  for example from a desktop shortcut.
* **System Integration:** Follows your system's dark/light preference and
  window button layout (close/minimize/maximize placement).
* **LineXinBar Ready:** On a LineXinBar desktop the Center becomes a
  controller-friendly LineXinBar application (see below).

## 🛠️ Dependencies

Linexin Center needs the following installed (the package pulls them in):

* GTK 4 and Libadwaita
* Python 3 and PyGObject (`python-gobject`), for Python widgets
* `python-psutil` and `python-distro`, kept for Python widgets (the System
  Updater uses `distro`)
* Optional: `lxb-toolkit`, for the LineXinBar interface

## 📦 Installation

**On Linexin** the Center is already installed; it comes with the system and is
updated with it.

**On other Arch-based distros** build and install the package from this
repository:

```bash
sudo pacman -S --needed base-devel git
git clone https://github.com/Petexy/Linexin-Center.git
cd Linexin-Center/packaging
makepkg -si          # installs what it needs to build and run, then the package
```

Widgets are separate packages that install into `/usr/share/linexin/widgets`
(for example Linexin Hello, Linpama, Alexy AI, the DaVinci and Affinity
installers). Install the ones you want the same way.

## 🚀 Usage

Start **Linexin Center** from your application menu, or from a terminal:

```bash
linexin-center                       # every widget, with the sidebar
linexin-center -w z-linexin-hello-widget   # one widget alone
linexin-center -l pl_PL              # another language
linexin-center --version
```

The widget name for `-w` is its file name without the extension; case, `-`,
`_` and spaces do not matter. `--voice`, `--compact` and `--conversation ID`
open a voice widget (Alexy AI) listening, in its compact voice window, or on a
saved conversation.

## 🎮 On LineXinBar

On a [LineXinBar](https://github.com/Petexy/LineXinBar) desktop with
`lxb-toolkit` installed, Linexin Center opens in LineXinBar's own style: one
glass pane with the widgets in a sidebar card and the chosen widget's page
beside it, driven by game controller, keyboard or mouse. Widgets that have a
LineXinBar page are listed there; "Edit widgets" at the bottom of the sidebar
shows, hides and reorders them, just like edit mode in the normal window.
Widget icons, pictures, text fields and mouse hover need `lxb-toolkit` 0.9.4.

Without `lxb-toolkit` (or if its window cannot open) you simply get the normal
GTK window, as on any other desktop.

## 📂 Directory Structure

```text
/usr/bin/linexin-center                       # the application
/usr/share/linexin/widgets/
├── localization/                             # translation dictionaries
│   ├── en_US/
│   ├── pl_PL/
│   └── ...
├── z-system-information-widget.so            # a native widget
└── a-system_updater.py                       # a Python widget
~/.config/linexin/linexin-center.json         # your widget order, hidden widgets, compact mode
```

## 🧩 Writing Widgets

Native widgets are shared libraries built against the Center's plugin API
(`crates/plugin-api`); `plugins/demo` is the smallest example and
`plugins/system-information` a complete one. Python widgets keep working as they
always did. See [DEVELOPMENT.md](DEVELOPMENT.md) for the details.

## 🔧 Building from Source

```bash
cargo build --release --workspace
cd packaging && makepkg -si          # or build the package
```

You need Rust (cargo), GTK 4 and Libadwaita with their development files,
`pkg-config`, and Python 3 with PyGObject. Development builds, tests and how the
Center works inside are described in [DEVELOPMENT.md](DEVELOPMENT.md).

## Licence

Copyright (C) 2026 Petexy

This program is free software: you can redistribute it and/or modify it under
the terms of the GNU General Public License as published by the Free Software
Foundation, either version 3 of the License, or (at your option) any later
version. It is distributed in the hope that it will be useful, but WITHOUT ANY
WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A
PARTICULAR PURPOSE. See the GNU General Public License in `LICENSE` for more
details.
