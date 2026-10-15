# System Information widget

Native Rust port of `z-system-information-widget.py`: the OS logo, name and
hostname header followed by the System, Session, Hardware and Storage & Memory
groups, with the original's layout, CSS classes, icons and values (python-distro,
psutil and the same `ps`, `xprop`, `lspci`, `nvidia-smi` and `modinfo` calls on a
worker thread).

- Install path: `/usr/share/linexin/widgets/z-system-information-widget.so`
  (`liblinexin_system_information.so` renamed), replacing the `.py`.
- Widget id and translation domain: `system_information`, i.e. the existing
  `/usr/share/linexin/widgets/localization/<lang>/system_information_dictionary.py`.
  All strings are the Python widget's English source strings.

For development, `examples/harness.rs` loads the built library through the C ABI
with a stub host and renders it to PNG, `examples/python_reference.py` renders the
original widget the same way, and `examples/compare.sh` runs both in a private
headless KWin (English, Polish, German; light, dark; wide, narrow; top, scrolled
to the end, after a live light/dark switch) and reports differing pixels:

```sh
cargo build -p linexin-system-information --lib --examples
plugins/system-information/examples/compare.sh
```
