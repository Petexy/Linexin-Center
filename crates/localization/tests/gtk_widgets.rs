//! widgets::translate_widget_tree / translate_dialog on real GTK widgets,
//! compared with the original helpers run by PyGObject on the same scene.
//! harness = false: GTK must stay on the thread that initialized it. Needs a
//! display (run under a headless compositor); skipped without one.

mod common;

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use linexin_localization::widgets::{translate_dialog, translate_widget_tree};
use linexin_localization::{Localization, install};

const DOMAIN: &str = "demo";

const DICTIONARY: &str = r#"translations = {
    "Hello": "Cześć",
    "Bold": "Pogrubiony",
    "Open": "Otwórz",
    "Type here": "Pisz tutaj",
    "Search…": "Szukaj…",
    "Password": "Hasło",
    "Title": "Tytuł",
    "Subtitle": "Podtytuł",
    "First": "Pierwszy",
    "Second": "Drugi",
    "Group": "Grupa",
    "Description": "Opis",
    "Page": "Strona",
    "Expander": "Rozwijany",
    "Entry row": "Wiersz wpisu",
    "Status": "Stan",
    "Window": "Okno",
    "Heading": "Nagłówek",
    "Body": "Treść",
    "Cancel": "Anuluj",
    "Downloaded {} files": "Pobrano {} plików",
    "Same": "Same",
    "Nested": "Zagnieżdżony",
}
"#;

/// Must match `build_scene` in tests/oracle/oracle.py.
fn build_scene() -> (gtk::Window, Vec<gtk::Widget>) {
    let window = gtk::Window::builder().title("Window").build();
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let inner = gtk::Box::new(gtk::Orientation::Vertical, 6);
    window.set_child(Some(&outer));
    outer.append(&inner);
    let markup = gtk::Label::new(None);
    markup.set_markup("<b>Bold</b> and <i> Hello </i>");
    let widgets: [gtk::Widget; 15] = [
        gtk::Label::new(Some("Hello")).upcast(),
        gtk::Label::with_mnemonic("_Open").upcast(),
        markup.upcast(),
        gtk::Label::new(Some("Downloaded 5 files")).upcast(),
        gtk::Label::new(Some("Downloaded Nested files")).upcast(),
        gtk::Label::new(Some("Same")).upcast(),
        gtk::Button::with_label("Open").upcast(),
        gtk::Button::with_mnemonic("_Cancel").upcast(),
        gtk::ToggleButton::with_label("Open").upcast(),
        gtk::CheckButton::with_label("Hello").upcast(),
        gtk::Frame::new(Some("Group")).upcast(),
        gtk::Entry::builder()
            .placeholder_text("Type here")
            .build()
            .upcast(),
        gtk::SearchEntry::builder()
            .placeholder_text("Search…")
            .build()
            .upcast(),
        gtk::PasswordEntry::builder()
            .placeholder_text("Password")
            .build()
            .upcast(),
        adw::Banner::new("Hello").upcast(),
    ];
    for widget in &widgets {
        inner.append(widget);
    }
    let model = gtk::StringList::new(&["First", "Second", "Third"]);
    let combo = adw::ComboRow::builder()
        .title("Title")
        .model(&model)
        .build();
    combo.set_selected(1);
    let group = adw::PreferencesGroup::builder()
        .title("Group")
        .description("Description")
        .build();
    let rows: [gtk::Widget; 5] = [
        adw::ActionRow::builder()
            .title("Title")
            .subtitle("Subtitle")
            .build()
            .upcast(),
        combo.upcast(),
        adw::SwitchRow::builder()
            .title("Title")
            .subtitle("Subtitle")
            .build()
            .upcast(),
        adw::ExpanderRow::builder()
            .title("Expander")
            .subtitle("Subtitle")
            .build()
            .upcast(),
        adw::EntryRow::builder().title("Entry row").build().upcast(),
    ];
    for row in &rows {
        group.add(row);
    }
    outer.append(&group);
    outer.append(&adw::PreferencesPage::builder().title("Page").build());
    let status = adw::StatusPage::builder()
        .title("Status")
        .description("Description")
        .build();
    status.set_child(Some(&gtk::Label::new(Some("Nested"))));
    outer.append(&status);

    let alert = adw::AlertDialog::new(Some("Heading"), Some("Body"));
    alert.add_response("cancel", "Cancel");
    alert.set_extra_child(Some(&gtk::Label::new(Some("Nested"))));
    #[allow(deprecated)]
    let message = {
        let message = adw::MessageDialog::new(
            None::<&gtk::Window>,
            Some("Heading"),
            Some("Downloaded 5 files"),
        );
        message.add_response("cancel", "_Cancel");
        message
    };
    (window, vec![alert.upcast(), message.upcast()])
}

const DUMPED_PROPERTIES: [&str; 10] = [
    "label",
    "title",
    "subtitle",
    "description",
    "placeholder-text",
    "heading",
    "body",
    "text",
    "use-markup",
    "use-underline",
];

/// Must match `dump` in tests/oracle/oracle.py.
fn dump(widget: &gtk::Widget, depth: usize, lines: &mut Vec<String>) {
    let mut fields = vec![widget.type_().name().to_owned()];
    for name in DUMPED_PROPERTIES {
        let Some(pspec) = widget.find_property(name) else {
            continue;
        };
        if pspec.value_type() == glib::Type::STRING {
            let value = widget.property::<Option<String>>(name);
            fields.push(format!(
                "{name}={}",
                value.map_or("-".to_owned(), |value| common::hex(&value))
            ));
        } else if pspec.value_type() == glib::Type::BOOL {
            fields.push(format!(
                "{name}={}",
                if widget.property::<bool>(name) {
                    "T"
                } else {
                    "F"
                }
            ));
        }
    }
    if let Some(combo) = widget.downcast_ref::<adw::ComboRow>()
        && let Some(model) = combo.model().and_downcast::<gtk::StringList>()
    {
        let items: Vec<String> = (0..model.n_items())
            .map(|position| {
                model
                    .string(position)
                    .map_or(String::new(), |item| common::hex(&item))
            })
            .collect();
        fields.push(format!(
            "items={} selected={}",
            items.join(","),
            combo.selected()
        ));
    }
    lines.push(format!("{}{}", "  ".repeat(depth), fields.join(" ")));
    let mut child = widget.first_child();
    while let Some(current) = child {
        dump(&current, depth + 1, lines);
        child = current.next_sibling();
    }
}

fn descendants(widget: &gtk::Widget) -> Vec<gtk::Widget> {
    let mut all = vec![widget.clone()];
    let mut child = widget.first_child();
    while let Some(current) = child {
        all.extend(descendants(&current));
        child = current.next_sibling();
    }
    all
}

fn main() {
    if gtk::init().is_err() || adw::init().is_err() {
        eprintln!("SKIPPED: no display for GTK");
        return;
    }
    let root = common::temp_dir("gtk-localization");
    common::write_dictionaries(&root, &[("pl_PL", DOMAIN, DICTIONARY)]);
    let localization = Localization::load(std::slice::from_ref(&root), "pl_PL", None);
    assert!(
        localization.diagnostics().is_empty(),
        "{:?}",
        localization.diagnostics()
    );
    install(localization);

    let (window, dialogs) = build_scene();
    let labels: Vec<gtk::Label> = descendants(window.upcast_ref())
        .into_iter()
        .filter_map(|widget| widget.downcast().ok())
        .collect();
    let same = labels
        .iter()
        .find(|label| label.label() == "Same")
        .expect("Same label");
    let notifications = Rc::new(Cell::new(0));
    let counter = notifications.clone();
    same.connect_notify_local(Some("label"), move |_, _| counter.set(counter.get() + 1));

    translate_dialog(&dialogs[0], "");
    assert_eq!(
        dialogs[0].property::<Option<String>>("heading").as_deref(),
        Some("Heading")
    );

    translate_widget_tree(window.upcast_ref(), DOMAIN);
    for dialog in &dialogs {
        translate_dialog(dialog, DOMAIN);
    }
    assert_eq!(
        notifications.get(),
        0,
        "an unchanged translation must not be set again"
    );
    assert_eq!(window.title().as_deref(), Some("Okno"));
    assert_eq!(
        dialogs[0].property::<Option<String>>("heading").as_deref(),
        Some("Nagłówek")
    );
    assert_eq!(dialogs[1].property::<String>("body"), "Pobrano 5 plików");
    let combo = descendants(window.upcast_ref())
        .into_iter()
        .find_map(|widget| widget.downcast::<adw::ComboRow>().ok())
        .expect("combo row");
    assert_eq!(
        combo.selected(),
        1,
        "selection survives re-splicing the model"
    );

    let mut lines = Vec::new();
    for widget in std::iter::once(window.upcast_ref::<gtk::Widget>()).chain(&dialogs) {
        dump(widget, 0, &mut lines);
    }
    if !common::python_available() {
        return;
    }
    let expected = match common::try_oracle(&["gtk", "pl_PL", &root.to_string_lossy()], "") {
        Ok(output) => output,
        Err(error) => {
            eprintln!("SKIPPED PyGObject comparison: {error}");
            return;
        }
    };
    let expected: Vec<&str> = expected.lines().collect();
    for (index, (actual, expected)) in lines.iter().zip(&expected).enumerate() {
        assert_eq!(
            actual, expected,
            "widget #{index} differs from the original"
        );
    }
    assert_eq!(lines.len(), expected.len());
    eprintln!(
        "gtk_widgets: {} widgets in identical state to the original's translation",
        lines.len()
    );
}
