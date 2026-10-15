# Rust Demo widget

The smallest complete native widget for Linexin Center, meant as the starting
point for new widgets. It exports the version-1 C ABI from `crates/plugin-api`:

- a static `WidgetDescriptorV1` (widget id `rust_demo`, untranslated name
  `Rust Demo`, icon) exported with `export_widget!`;
- a `create` callback that validates the context, calls `gtk::init()` (the
  library has its own gtk-rs state) and returns an unparented widget with an
  owned reference (`into_glib_ptr()`);
- `catch_unwind` around `create` and every GTK callback, because a panic must
  never unwind into C.

Every user-visible string goes through `tr()`, which asks the Center to look it
up in `localization/<lang>/rust_demo_dictionary.py`, the same dictionaries
Python widgets use. The sidebar title is translated by the Center from the same
domain.

```sh
cargo build -p linexin-demo
sudo install -Dm644 target/debug/liblinexin_demo.so /usr/share/linexin/widgets/rust-demo.so
sudo install -Dm644 plugins/demo/localization/pl_PL/rust_demo_dictionary.py \
    /usr/share/linexin/widgets/localization/pl_PL/rust_demo_dictionary.py
```

The root, counter label and buttons have stable widget names
(`linexin-demo-root`, `linexin-demo-count`, `linexin-demo-increment`,
`linexin-demo-reset`) for the Center's smoke tests.
