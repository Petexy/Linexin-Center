//! The Center on a LineXinBar session: an lxb-toolkit application in
//! LineXinBar's own style instead of the GTK window. The widgets that have a
//! LineXinBar page stand in a glass column, each with its own icon; the
//! chosen one's page is beside it. Under them, set apart, "Edit widgets"
//! opens the page that shows, hides and orders them (the GTK window's edit
//! mode); a widget's Options hold the same, and its desktop shortcut. GTK is
//! never initialized here, and Python widgets (which only have a GTK UI) are
//! not listed.

use crate::catalog::{self, Kind, WidgetFile};
use crate::config::{self, ConfigFile};
use crate::native::{LxbPage, NativeWidget};
use crate::options::Options;
use crate::paths::{self, Paths};
use crate::shortcut;
use linexin_localization::{HOST_DOMAIN, Localization, detect_language, global, tr};
use linexin_lxb::{
    Action, App, Button, Column, ColumnContent, ColumnEntry, Emblem, Flow, Hands, Hint, Legend,
    Overlay, PAGE_SPOT, Page, Panes, Split, hand_back_sound, host_frame, icon_file, raw_actions,
};
use linexin_plugin_api::LXB_PAGE_LEAVE;
use serde_json::Value;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};

const FALLBACK_MARK: &str = "category-system";
/// The mark of "Edit widgets" (the shell's own for putting things in order).
const EDIT_MARK: &str = "setting-order";
/// What the page of "Edit widgets" says above the widgets.
const EDIT_NOTE: &str =
    "Choose which widgets the sidebar shows. Options moves the chosen one up or down.";
const FIRST_LAUNCH_WIDGET: &str = "Linexin Hello";
/// `LINEXIN_LXB_SHOT=path[,WIDTHxHEIGHT][,state][,wait=S][,frames=N][,act=F:ACTION]...`:
/// draw one settled frame to a PNG instead of opening the window (tests; no
/// display needed). With `frames`, the picture is of the Nth frame, and each
/// `act` hands the window an action (`accept`, `down`, `menu`, ...) on frame
/// F, as a key would.
const SHOT_VARIABLE: &str = "LINEXIN_LXB_SHOT";

static STOP: AtomicBool = AtomicBool::new(false);
/// SIGUSR1: the wake-word daemon asks the shown page for voice input again.
static REACTIVATE: AtomicBool = AtomicBool::new(false);

extern "C" fn stop(_signal: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

extern "C" fn reactivate(_signal: libc::c_int) {
    REACTIVATE.store(true, Ordering::SeqCst);
}

/// Whether this process uses the LineXinBar front end: `LINEXIN_FRONTEND`
/// (`lxb` or `gtk`) decides when set, otherwise a LineXinBar session does.
pub fn wanted() -> bool {
    choose(
        std::env::var("LINEXIN_FRONTEND").ok().as_deref(),
        linexin_lxb::in_session(),
    )
}

fn choose(frontend: Option<&str>, in_session: bool) -> bool {
    match frontend.map(str::trim) {
        Some(value) if value.eq_ignore_ascii_case("lxb") => true,
        Some(value) if value.eq_ignore_ascii_case("gtk") => false,
        _ => in_session,
    }
}

/// Run the LineXinBar front end. `Err` when it cannot show itself: LineXinBar's
/// libraries cannot be opened (lxb-toolkit is not installed, or is too old),
/// or its window could not open (no display, no graphics adapter). The caller
/// then falls back to the GTK window.
pub fn run(options: &Options) -> Result<ExitCode, String> {
    linexin_lxb::load()?;
    // Said once the window has opened: a fallback's GTK window says it too.
    let (paths, source_tree) = Paths::locate(&options.widgets_dirs);
    if let Some(target) = &options.target_widget {
        widget_has_page(&paths, target)?;
    }
    // Python widgets are not shown here, so no interpreter evaluates the
    // dictionaries that are code rather than data.
    let language = detect_language(options.language.as_deref());
    let localization = Localization::load(&paths.localization_dirs(), &language, None);
    let title = localization.get_text("Linexin Center", HOST_DOMAIN);
    let mut app = App::new(paths::APP_ID, &title)?.plain().driven();
    // Nothing that lasts for the process (the global catalog, the widgets,
    // the signal handlers) is set up before the window has opened: until
    // then the GTK window can still take over.
    let mut localization = Some(localization);
    let mut start = || {
        if let Some(source_tree) = &source_tree {
            paths::say_running_from_source(source_tree);
        }
        if let Some(localization) = localization.take() {
            linexin_localization::install(localization);
        }
        println!("System language detected: {}", global().language());
        Center::new(&paths, options)
    };
    let mut center = None;
    let outcome = match std::env::var(SHOT_VARIABLE) {
        Ok(spec) => {
            let shot = Shot::parse(&spec);
            let center = center.insert(start());
            center.prepare_shot(shot.state, shot.wait);
            match shot.frames {
                Some(frames) => {
                    let mut frame = 0;
                    app.shot_after(&shot.path, shot.width, shot.height, 10.0, frames, |page| {
                        for (_, action) in shot.acts.iter().filter(|(at, _)| *at == frame) {
                            page.act(*action);
                        }
                        frame += 1;
                        center.frame(page);
                    })
                }
                None => app.shot(&shot.path, shot.width, shot.height, 10.0, |page| {
                    center.frame(page)
                }),
            }
        }
        Err(_) => app.run(|page| {
            // The first frame: the window has opened.
            let center = center.get_or_insert_with(|| {
                // SAFETY: the handler only stores into an atomic.
                unsafe {
                    libc::signal(libc::SIGINT, stop as *const () as libc::sighandler_t);
                    libc::signal(libc::SIGTERM, stop as *const () as libc::sighandler_t);
                    libc::signal(libc::SIGUSR1, reactivate as *const () as libc::sighandler_t);
                }
                start()
            });
            if STOP.load(Ordering::SeqCst) {
                page.quit();
                return;
            }
            // Low-end mode draws once a second while nothing moves; look
            // for a signal a little more often than that.
            page.redraw_within(0.5);
            center.frame(page);
        }),
    };
    let Some(center) = center else {
        // Not a frame was drawn: the window never opened.
        return match outcome {
            Ok(()) => Ok(ExitCode::SUCCESS),
            Err(why) => Err(format!("LineXinBar's window could not open: {why}")),
        };
    };
    // Widgets release their pages (and whatever threads they run) first.
    drop(center);
    if let Some(manager) = linexin_services::sudo::existing() {
        manager.cleanup();
    }
    match outcome {
        Ok(()) => Ok(ExitCode::SUCCESS),
        Err(error) => {
            eprintln!("Linexin Center: {error}");
            Ok(ExitCode::FAILURE)
        }
    }
}

/// One widget with a LineXinBar page.
struct Entry {
    file_name: String,
    /// Untranslated `widgetname`.
    name: String,
    /// What the column says: the name through the widget's own dictionary.
    label: String,
    mark: String,
    /// The widget's own icon (`icon`), where a file of it is found.
    picture: Option<PathBuf>,
    native: NativeWidget,
    /// Created the first time the page is shown, kept for the process.
    page: Option<Result<LxbPage, String>>,
    /// Draws the error page when `page` could not be created.
    message: Flow,
    /// `--voice`/`--compact` and `--conversation`, for the `-w` widget.
    voice_autostart: bool,
    conversation_id: Option<String>,
}

impl Entry {
    fn new(file: &WidgetFile, native: NativeWidget, paths: &Paths) -> Self {
        let label = global().get_text(&native.name, &native.widget_id);
        let mark = if native.lxb_mark.is_empty() {
            FALLBACK_MARK.to_owned()
        } else {
            native.lxb_mark.clone()
        };
        // Where the GTK window finds it (a source tree's `share` included),
        // then an icon theme's name or a staged `/usr/share`.
        let picture = paths
            .icon_file(&native.icon)
            .or_else(|| icon_file(&native.icon));
        Self {
            file_name: file.file_name.clone(),
            name: native.name.clone(),
            label,
            mark,
            picture,
            native,
            page: None,
            message: Flow::new(),
            voice_autostart: false,
            conversation_id: None,
        }
    }

    /// The page, created the first time it is needed.
    fn page(&mut self, single: bool) -> &mut Result<LxbPage, String> {
        let native = &self.native;
        let (voice, conversation) = (self.voice_autostart, self.conversation_id.as_deref());
        self.page.get_or_insert_with(|| {
            let page = native.create_lxb_page(single, voice, conversation);
            if let Err(error) = &page {
                eprintln!("{}: {error}", native.path.display());
            }
            page
        })
    }

    /// Draw the page (creating it on first use); whether it gave the light back.
    fn frame(
        &mut self,
        page: &Page,
        rect: [f32; 4],
        lit: bool,
        actions: &[Action],
        single: bool,
    ) -> bool {
        match self.page(single) {
            Ok(widget) => {
                let raw = raw_actions(actions);
                let frame = host_frame(page, rect, lit, &raw);
                widget.frame(&frame) == LXB_PAGE_LEAVE
            }
            Err(error) => {
                let error = error.clone();
                let input = if lit {
                    self.message.input(page, actions)
                } else {
                    Default::default()
                };
                let mut flow = self.message.begin(page, rect, lit);
                flow.head(
                    Emblem::with_picture(self.picture.as_deref(), &self.mark),
                    &self.label,
                );
                // Original `_create_error_widget`.
                flow.text(&format!(
                    "{} '{}': {error}",
                    tr("Error loading"),
                    self.label
                ));
                flow.finish();
                input.leave.is_some()
            }
        }
    }
}

/// What a widget's Options offer (the GTK window's edit mode and sidebar
/// menu, as commands).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    MoveUp,
    MoveDown,
    Hide,
    Show,
    CreateShortcut,
    RemoveShortcut,
    /// Go to the page of "Edit widgets".
    Edit,
}

impl Command {
    fn label(self) -> String {
        tr(match self {
            Command::MoveUp => "Move up",
            Command::MoveDown => "Move down",
            Command::Hide => "Hide widget",
            Command::Show => "Show widget",
            Command::CreateShortcut => "Create a shortcut",
            Command::RemoveShortcut => "Remove shortcut",
            Command::Edit => "Edit widgets",
        })
    }
}

/// The menu standing over the window, and what its commands are about.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenMenu {
    commands: Vec<Command>,
    /// The widget (an index of `entries`) the commands act on.
    entry: usize,
    /// Raised on the page of "Edit widgets" rather than in the column.
    from_editor: bool,
}

/// What the question standing over the window asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Question {
    /// Leave or stay: the first answer quits.
    Leave,
    /// Something to read, with OK.
    Told,
}

struct Center {
    paths: Paths,
    /// Every widget file in sidebar order, Python ones included, so that a
    /// reorder here keeps the GTK front end's order of the rest.
    order: Vec<String>,
    /// Widgets with a LineXinBar page, in that order.
    entries: Vec<Entry>,
    hidden: Vec<String>,
    config: ConfigFile,
    column: Column,
    split: Split,
    page_lit: bool,
    single: bool,
    question: Option<Question>,
    menu: Option<OpenMenu>,
    /// The page of "Edit widgets", under the widgets in the column.
    editor: Flow,
    /// A shortcut being written or removed as root: the widget's name, and
    /// where the outcome arrives.
    shortcut_job: Option<(String, Receiver<shortcut::Outcome>)>,
    /// The page shown when nothing can be: no widgets, or `-w` found none.
    message: Option<String>,
    message_flow: Flow,
    /// Raise the context menu on the next frame (shots).
    raise_menu: bool,
    /// What the hands are on, for the legend's pictures.
    hands: Hands,
    /// The row along the foot saying what the buttons do.
    legend: Legend,
    /// Whether the last frame's sidebar and page slid as one view (then the
    /// light does not follow the pointer from one to the other).
    slides: bool,
}

/// `Err` when the widget `-w` names is installed but has no LineXinBar page
/// (a Python widget, or a native one with only a GTK face, as Desktop
/// Presets): the GTK window shows it then, as a desktop shortcut to it
/// expects. A name that matches nothing is told on the LineXinBar page.
fn widget_has_page(paths: &Paths, target: &str) -> Result<(), String> {
    let files = catalog::sidebar_order(&paths.widgets_dirs, Vec::new());
    let Some(file) = catalog::find_target(&files, target) else {
        return Ok(());
    };
    let has_page = file.kind == Kind::Native
        && NativeWidget::load(&file.path).is_ok_and(|native| native.has_lxb_page());
    if has_page {
        Ok(())
    } else {
        Err(format!("'{}' has no LineXinBar page", file.file_name))
    }
}

impl Center {
    fn new(paths: &Paths, options: &Options) -> Self {
        let config = ConfigFile::from_env();
        let saved = config.load();
        let files = catalog::sidebar_order(
            &paths.widgets_dirs,
            config::names(&saved, config::WIDGET_ORDER),
        );
        let mut center = Self {
            paths: paths.clone(),
            order: files.iter().map(|file| file.file_name.clone()).collect(),
            entries: vec![],
            hidden: config::names(&saved, config::HIDDEN_WIDGETS),
            config,
            column: Column::new(),
            split: Split::new(),
            page_lit: false,
            single: options.target_widget.is_some(),
            question: None,
            menu: None,
            editor: Flow::new(),
            shortcut_job: None,
            message: None,
            message_flow: Flow::new(),
            raise_menu: false,
            hands: Hands::new(),
            legend: Legend::new(),
            slides: false,
        };
        match &options.target_widget {
            Some(target) => {
                center.load_target(&files, target);
                // There is no `--compact` window on LineXinBar: the page
                // itself listens.
                if let Some(entry) = center.entries.first_mut() {
                    entry.voice_autostart = options.voice_autostart || options.compact_mode;
                    entry.conversation_id = options.conversation_id.clone();
                }
            }
            None => center.load_all(&files),
        }
        center
    }

    fn load_all(&mut self, files: &[WidgetFile]) {
        self.add_widgets(files);
        self.select_first_widget();
    }

    fn add_widgets(&mut self, files: &[WidgetFile]) {
        for file in files.iter().filter(|file| file.kind == Kind::Native) {
            match NativeWidget::load(&file.path) {
                Ok(native) if native.has_lxb_page() => {
                    self.entries.push(Entry::new(file, native, &self.paths))
                }
                Ok(_) => {}
                Err(error) => eprintln!("{}: {error}", file.path.display()),
            }
        }
        self.message = self
            .entries
            .is_empty()
            .then(|| tr("No widgets are available on LineXinBar."));
    }

    /// A widget asked for a rescan (`refresh_widgets`): add the files that
    /// appeared since, after the others, as the GTK window does.
    fn refresh(&mut self) {
        if self.single {
            return;
        }
        let mut new: Vec<WidgetFile> = catalog::discover(&self.paths.widgets_dirs)
            .into_iter()
            .filter(|file| !self.order.contains(&file.file_name))
            .collect();
        catalog::sort_by_mtime(&mut new);
        self.order
            .extend(new.iter().map(|file| file.file_name.clone()));
        self.add_widgets(&new);
    }

    /// Single-widget mode: the target's page fills the window.
    fn load_target(&mut self, files: &[WidgetFile], target: &str) {
        let native = catalog::find_target(files, target)
            .filter(|file| file.kind == Kind::Native)
            .and_then(|file| Some((file, NativeWidget::load(&file.path).ok()?)))
            .filter(|(_, native)| native.has_lxb_page());
        match native {
            Some((file, native)) => self.entries.push(Entry::new(file, native, &self.paths)),
            None => {
                let text = format!("Widget '{target}' not found or failed to load");
                self.message = Some(global().translate(&text, HOST_DOMAIN));
            }
        }
    }

    fn is_hidden(&self, file_name: &str) -> bool {
        config::hidden_names(file_name).any(|name| self.hidden.contains(&name))
    }

    /// Indices of the entries the column shows.
    fn visible(&self) -> Vec<usize> {
        (0..self.entries.len())
            .filter(|&index| self.single || !self.is_hidden(&self.entries[index].file_name))
            .collect()
    }

    /// Original `_auto_select_first_widget`: Linexin Hello on the first
    /// launch, otherwise the first widget.
    fn select_first_widget(&mut self) {
        let mut config = self.config.load();
        if config::flag(&config, config::HAS_LAUNCHED_BEFORE) {
            return;
        }
        config.insert(config::HAS_LAUNCHED_BEFORE.into(), Value::Bool(true));
        self.config.save(&config);
        let visible = self.visible();
        if let Some(position) = visible
            .iter()
            .position(|&index| self.entries[index].name == FIRST_LAUNCH_WIDGET)
        {
            self.column.select(position);
        }
    }

    /// Whether the column ends with "Edit widgets": always, but for a widget
    /// shown alone or nothing to edit.
    fn editable(&self) -> bool {
        !self.single && !self.entries.is_empty()
    }

    /// Whether the column's chosen entry is "Edit widgets".
    fn editing(&self, visible: &[usize]) -> bool {
        self.editable() && self.column.selected() == visible.len()
    }

    fn frame(&mut self, page: &Page) {
        if crate::host_api::take_refresh_request() {
            self.refresh();
        }
        self.collect_shortcut(page);
        if REACTIVATE.swap(false, Ordering::SeqCst)
            && let Some(&index) = self.visible().get(self.column.selected())
            && let Some(Ok(widget)) = self.entries[index].page.as_mut()
        {
            widget.reactivate_voice();
        }
        let mut actions = page.actions();
        self.hands.follow(page, &actions);
        // A click on the legend is a press of the button it pictures.
        if let Some(button) = self.legend.pressed(page) {
            self.hands.clicked();
            actions.push(button.action());
        }
        // Only the Center's own questions are answered here; a widget's
        // menus and dialogs are read by the widget.
        if let Some(question) = self.question
            && let Some(answer) = page.answered()
        {
            self.question = None;
            if question == Question::Leave && answer == 0 {
                page.quit();
            }
        }
        if self.menu.is_some()
            && let Some(chosen) = page.chose()
            && let Some(menu) = self.menu.take()
        {
            self.on_menu(menu, chosen);
        }

        let visible = self.visible();
        let rows = visible.len() + usize::from(self.editable());
        let column_has_it = !self.page_lit && !self.single;
        // The light follows the pointer from the sidebar into the page and
        // back where the two stand side by side.
        self.column.set_hover_reclaims(!self.slides);
        let taken = self.column.input(
            page,
            if column_has_it { &actions } else { &[] },
            rows,
            column_has_it,
        );
        let mut for_page = if column_has_it {
            taken.for_page.clone()
        } else {
            actions.clone()
        };
        if taken.reclaimed {
            self.page_lit = false;
            for_page.clear();
        }
        if taken.enter {
            self.page_lit = true;
        }
        if taken.back {
            self.ask_to_leave(page);
        }
        if column_has_it && (taken.unhandled.contains(&Action::Menu) || self.raise_menu) {
            self.raise_menu = false;
            if let Some(&index) = visible.get(self.column.selected()) {
                self.open_menu(page, index, false);
            }
        }
        let pointed_in =
            page.pressed(PAGE_SPOT) || (!self.slides && page.hovered() == Some(PAGE_SPOT));
        if !self.page_lit && !self.single && pointed_in && rows > 0 {
            self.page_lit = true;
        }

        let edit_label = tr("Edit widgets");
        let entries: Vec<ColumnEntry> = visible
            .iter()
            .map(|&index| {
                let entry = &self.entries[index];
                ColumnEntry {
                    mark: &entry.mark,
                    picture: entry.picture.as_deref(),
                    label: &entry.label,
                    ..ColumnEntry::default()
                }
            })
            .collect();
        // At the bottom of the sidebar, as the GTK window's Edit button is.
        let foot = self.editable().then(|| ColumnEntry {
            mark: EDIT_MARK,
            label: &edit_label,
            ..ColumnEntry::default()
        });
        let content = ColumnContent {
            entries: &entries,
            foot,
        };
        let panes = if self.single {
            Panes::whole(page)
        } else {
            self.split
                .set_column_width(Some(Column::fitting_width(page, &content)));
            self.split.layout(page, self.page_lit)
        };
        self.slides = panes.slides;
        page.draw_pane(panes.window, Overlay::Dialog);
        if !self.single {
            self.column
                .draw(page, panes.column, &content, !self.page_lit);
        }
        let lit = self.page_lit || self.single;
        if !lit {
            page.draw_spot(PAGE_SPOT, panes.page);
        }
        let page_actions = if lit { for_page } else { vec![] };

        let chosen = visible.get(self.column.selected()).copied();
        let left = match (chosen, &self.message) {
            (Some(index), _) => {
                self.entries[index].frame(page, panes.content, lit, &page_actions, self.single)
            }
            (None, _) if self.editing(&visible) => {
                self.editor_frame(page, panes.content, lit, &page_actions)
            }
            (None, message) => {
                let message = message.clone().unwrap_or_default();
                let input = if lit {
                    self.message_flow.input(page, &page_actions)
                } else {
                    Default::default()
                };
                let mut flow = self.message_flow.begin(page, panes.content, lit);
                flow.head(FALLBACK_MARK, &tr("Linexin Center"));
                flow.text(&message);
                flow.finish();
                input.leave.is_some()
            }
        };
        if left {
            if self.single {
                self.ask_to_leave(page);
            } else {
                self.page_lit = false;
                page.play(hand_back_sound(&page_actions));
            }
        }

        // The legend is drawn last: its targets stand over whatever the page
        // left under them.
        if self.hands.shown() {
            let hints = self.hints(&self.visible());
            let foot = panes.foot;
            self.legend.draw(
                page,
                foot[0] + foot[2],
                foot[1] + foot[3] * 0.5,
                &hints,
                self.hands.pad(),
            );
        } else {
            self.legend.clear();
        }
    }

    /// The page of "Edit widgets": every widget with a switch saying whether
    /// the column shows it, in the column's order (the GTK edit mode's rows
    /// with their eye buttons). Accept turns a switch over; Options moves the
    /// widget, and holds its shortcut. Whether it gave the light back.
    fn editor_frame(&mut self, page: &Page, rect: [f32; 4], lit: bool, actions: &[Action]) -> bool {
        let mut flow = std::mem::take(&mut self.editor);
        let input = if lit {
            flow.input(page, actions)
        } else {
            Default::default()
        };
        let mut toggled = None;
        let mut frame = flow.begin(page, rect, lit);
        frame.head(EDIT_MARK, &tr("Edit widgets"));
        frame.note(&tr(EDIT_NOTE));
        let switches = page.draws_switches();
        let (shown_word, hidden_word) = (tr("Shown"), tr("Hidden"));
        for (index, entry) in self.entries.iter().enumerate() {
            let emblem = Emblem::with_picture(entry.picture.as_deref(), &entry.mark);
            let shown = !self.is_hidden(&entry.file_name);
            let pressed = if switches {
                frame.switch(emblem, &entry.label, shown)
            } else {
                let word = if shown { &shown_word } else { &hidden_word };
                frame.entry(emblem, &entry.label, Some(word))
            };
            if pressed {
                toggled = Some(index);
            }
        }
        frame.finish();
        // The widgets are the page's only controls: the light's index is the
        // widget's.
        let focused = flow.focused().filter(|index| *index < self.entries.len());
        self.editor = flow;
        if let Some(index) = toggled {
            let hidden = !self.is_hidden(&self.entries[index].file_name);
            self.set_hidden(index, hidden);
            self.column.select(self.visible().len());
        }
        if input.unhandled.contains(&Action::Menu)
            && let Some(index) = focused
        {
            self.open_menu(page, index, true);
        }
        input.leave.is_some()
    }

    /// What the buttons do where the light is: two pairs at most, and only
    /// buttons that do something there (nothing about moving).
    fn hints(&self, visible: &[usize]) -> Vec<Hint> {
        if self.single || self.entries.is_empty() {
            return vec![Hint::new(tr("Close"), Button::Back)];
        }
        let editing = self.editing(visible);
        match (self.page_lit, editing) {
            (true, true) => {
                let turn = self
                    .editor
                    .focused()
                    .and_then(|index| self.entries.get(index))
                    .map(|entry| {
                        if self.is_hidden(&entry.file_name) {
                            tr("Show widget")
                        } else {
                            tr("Hide widget")
                        }
                    });
                turn.map(|turn| Hint::new(turn, Button::Accept))
                    .into_iter()
                    .chain([Hint::new(tr("Options"), Button::Options)])
                    .collect()
            }
            (true, false) => vec![Hint::new(tr("Back"), Button::Back)],
            (false, true) => vec![Hint::new(tr("Open"), Button::Accept)],
            (false, false) => vec![
                Hint::new(tr("Open"), Button::Accept),
                Hint::new(tr("Options"), Button::Options),
            ],
        }
    }

    fn ask_to_leave(&mut self, page: &Page) {
        page.ask(
            &tr("Leave Linexin Center?"),
            &tr("The window closes."),
            &[&tr("Leave"), &tr("Stay")],
        );
        self.question = Some(Question::Leave);
    }

    /// Something to read, answered with OK.
    fn tell(&mut self, page: &Page, title: &str, body: &str) {
        page.ask(title, body, &[&tr("OK")]);
        self.question = Some(Question::Told);
    }

    /// A widget's Options (Menu key, right button, north face button), in
    /// the column or on the page of "Edit widgets": the LineXinBar form of
    /// the GTK window's edit mode and sidebar menu.
    fn open_menu(&mut self, page: &Page, entry: usize, from_editor: bool) {
        let Some(widget) = self.entries.get(entry) else {
            return;
        };
        let mut commands = vec![Command::MoveUp, Command::MoveDown];
        commands.push(if self.is_hidden(&widget.file_name) {
            Command::Show
        } else {
            Command::Hide
        });
        commands.push(if shortcut::exists(&widget.file_name) {
            Command::RemoveShortcut
        } else {
            Command::CreateShortcut
        });
        if !from_editor {
            commands.push(Command::Edit);
        }
        let labels: Vec<String> = commands.iter().map(|command| command.label()).collect();
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        page.menu(Some(&widget.label), &labels);
        self.menu = Some(OpenMenu {
            commands,
            entry,
            from_editor,
        });
    }

    fn on_menu(&mut self, menu: OpenMenu, chosen: usize) {
        let Some(&command) = menu.commands.get(chosen) else {
            return;
        };
        let index = menu.entry;
        if index >= self.entries.len() {
            return;
        }
        match command {
            Command::MoveUp | Command::MoveDown => {
                let step = if command == Command::MoveUp { -1 } else { 1 };
                // On the page every widget has a place, hidden ones too; in
                // the column a widget moves past the one shown beside it.
                let list: Vec<usize> = if menu.from_editor {
                    (0..self.entries.len()).collect()
                } else {
                    self.visible()
                };
                if let Some(moved) = self.move_entry(&list, index, step) {
                    if menu.from_editor {
                        self.editor.focus(moved);
                    } else if let Some(position) = self.visible().iter().position(|&i| i == moved) {
                        self.column.select(position);
                    }
                }
            }
            Command::Hide | Command::Show => {
                self.set_hidden(index, command == Command::Hide);
                if menu.from_editor {
                    self.column.select(self.visible().len());
                } else {
                    let count = self.visible().len();
                    self.column
                        .select(self.column.selected().min(count.saturating_sub(1)));
                }
            }
            Command::CreateShortcut | Command::RemoveShortcut => {
                self.start_shortcut(index, command == Command::CreateShortcut);
            }
            Command::Edit => {
                self.column.select(self.visible().len());
                self.page_lit = true;
            }
        }
    }

    /// Move the widget `index` past its neighbour in `list` (indices of
    /// `entries`, in order) by `step`. The full order (with the widgets this
    /// front end does not show) keeps every other position, and `entries`
    /// follows it. The widget's new index.
    fn move_entry(&mut self, list: &[usize], index: usize, step: isize) -> Option<usize> {
        let position = list.iter().position(|&i| i == index)?;
        let &neighbour = position
            .checked_add_signed(step)
            .and_then(|target| list.get(target))?;
        let name = self.entries[index].file_name.clone();
        let target = self.entries[neighbour].file_name.clone();
        self.order.retain(|file| *file != name);
        let at = self
            .order
            .iter()
            .position(|file| *file == target)
            .map_or(self.order.len(), |at| if step > 0 { at + 1 } else { at });
        self.order.insert(at, name.clone());
        let order = &self.order;
        self.entries.sort_by_key(|entry| {
            order
                .iter()
                .position(|file| *file == entry.file_name)
                .unwrap_or(usize::MAX)
        });
        self.save();
        self.entries
            .iter()
            .position(|entry| entry.file_name == name)
    }

    /// Hide or show a widget, under the names the GTK edit mode uses.
    fn set_hidden(&mut self, index: usize, hidden: bool) {
        let file_name = self.entries[index].file_name.clone();
        if hidden {
            if !self.is_hidden(&file_name) {
                self.hidden.push(file_name);
            }
        } else {
            let names: Vec<String> = config::hidden_names(&file_name).collect();
            self.hidden.retain(|name| !names.contains(name));
        }
        self.save();
    }

    /// Same keys and rules as the GTK edit mode.
    fn save(&self) {
        let (order, hidden) = (self.order.clone(), self.hidden.clone());
        self.config.update(|config| {
            config::set_names(config, config::WIDGET_ORDER, &order);
            config::set_names(config, config::HIDDEN_WIDGETS, &hidden);
        });
    }

    /// Write or remove a widget's desktop shortcut as root, on a thread: the
    /// session's polkit agent asks for the password (the GTK window's own
    /// password dialog has no counterpart here).
    fn start_shortcut(&mut self, index: usize, create: bool) {
        if self.shortcut_job.is_some() {
            return;
        }
        let entry = &self.entries[index];
        let (file_name, name, icon) = (
            entry.file_name.clone(),
            entry.label.clone(),
            entry.native.icon.clone(),
        );
        let (sender, receiver) = mpsc::channel();
        let job_name = name.clone();
        let spawned = std::thread::Builder::new()
            .name("linexin-shortcut".into())
            .spawn(move || {
                let icon = Some(icon.as_str()).filter(|icon| !icon.is_empty());
                let _ = sender.send(shortcut::change(&file_name, &name, icon, create, pkexec));
            });
        if spawned.is_ok() {
            self.shortcut_job = Some((job_name, receiver));
        }
    }

    /// Say how changing a shortcut went, once it has.
    fn collect_shortcut(&mut self, page: &Page) {
        let Some((name, receiver)) = &self.shortcut_job else {
            return;
        };
        let outcome = match receiver.try_recv() {
            Ok(outcome) => outcome,
            Err(TryRecvError::Empty) => {
                page.redraw_within(0.2);
                return;
            }
            Err(TryRecvError::Disconnected) => Err("the operation failed unexpectedly".into()),
        };
        let name = name.clone();
        self.shortcut_job = None;
        match outcome {
            Ok(Some((heading, body))) => {
                self.tell(page, &tr(heading), &tr(body).replacen("{}", &name, 1));
            }
            // Nothing to remove, or the password was not given.
            Ok(None) => {}
            Err(error) if error.is_empty() => {}
            Err(error) => {
                eprintln!("Error updating shortcut: {error}");
                self.tell(page, &tr("Error"), &error);
            }
        }
    }

    fn prepare_shot(&mut self, state: ShotState, wait: f32) {
        match state {
            ShotState::Column => {}
            ShotState::Page => self.page_lit = true,
            ShotState::Menu => self.raise_menu = true,
            ShotState::Edit => {
                self.column.select(self.visible().len());
                self.page_lit = true;
            }
        }
        // A picture has no clock of its own to wait on a page's worker with:
        // create the chosen page now and give it this long.
        if wait > 0.0 {
            let single = self.single;
            if let Some(&index) = self.visible().get(self.column.selected()) {
                let _ = self.entries[index].page(single);
            }
            std::thread::sleep(std::time::Duration::from_secs_f32(wait));
        }
    }
}

/// Run a command's words as root through the session's polkit agent. A
/// password that was not given (pkexec's 126) is an empty error: nothing
/// to say about it.
fn pkexec(argv: &[&str]) -> Result<(), String> {
    match std::process::Command::new("pkexec").args(argv).status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) if status.code() == Some(126) => Err(String::new()),
        Ok(status) => Err(shortcut::failed(argv, status.code())),
        Err(error) => Err(error.to_string()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShotState {
    Column,
    Page,
    Menu,
    /// The page of "Edit widgets", lit.
    Edit,
}

#[derive(Debug, PartialEq)]
struct Shot {
    path: String,
    width: u32,
    height: u32,
    state: ShotState,
    /// `wait=SECONDS`: let the chosen page load first.
    wait: f32,
    /// `frames=N`: the picture is of the Nth frame.
    frames: Option<u32>,
    /// `act=FRAME:ACTION`: actions handed to the window on those frames.
    acts: Vec<(u32, Action)>,
}

impl Shot {
    fn parse(spec: &str) -> Self {
        let mut parts = spec.split(',');
        let path = parts.next().unwrap_or_default().to_owned();
        let mut shot = Self {
            path,
            width: 1600,
            height: 900,
            state: ShotState::Column,
            wait: 0.0,
            frames: None,
            acts: vec![],
        };
        for part in parts {
            if let Some(wait) = part.strip_prefix("wait=") {
                shot.wait = wait.parse().unwrap_or(0.0);
            } else if let Some(frames) = part.strip_prefix("frames=") {
                shot.frames = frames.parse().ok();
            } else if let Some(act) = part.strip_prefix("act=") {
                let action = act.split_once(':').and_then(|(frame, name)| {
                    let action = Action::ALL
                        .iter()
                        .copied()
                        .find(|action| action.key() == name)?;
                    Some((frame.parse().ok()?, action))
                });
                shot.acts.extend(action);
            } else if let Some((width, height)) = part.split_once('x')
                && let (Ok(width), Ok(height)) = (width.parse(), height.parse())
            {
                (shot.width, shot.height) = (width, height);
            } else {
                shot.state = match part {
                    "page" => ShotState::Page,
                    "menu" => ShotState::Menu,
                    "edit" => ShotState::Edit,
                    _ => ShotState::Column,
                };
            }
        }
        shot
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_session_decides_unless_overridden() {
        assert!(choose(None, true));
        assert!(!choose(None, false));
        assert!(choose(Some("lxb"), false));
        assert!(choose(Some(" LXB "), false));
        assert!(!choose(Some("gtk"), true));
        assert!(choose(Some("something else"), true));
    }

    #[test]
    fn shot_specifications() {
        assert_eq!(
            Shot::parse("/tmp/a.png"),
            Shot {
                path: "/tmp/a.png".into(),
                width: 1600,
                height: 900,
                state: ShotState::Column,
                wait: 0.0,
                frames: None,
                acts: vec![],
            }
        );
        assert_eq!(
            Shot::parse("/tmp/a.png,700x1000,page,wait=1.5"),
            Shot {
                path: "/tmp/a.png".into(),
                width: 700,
                height: 1000,
                state: ShotState::Page,
                wait: 1.5,
                frames: None,
                acts: vec![],
            }
        );
        let shot = Shot::parse("/tmp/a.png,edit,frames=40,act=10:accept,act=12:down,act=x:menu");
        assert_eq!(shot.state, ShotState::Edit);
        assert_eq!(shot.frames, Some(40));
        assert_eq!(shot.acts, vec![(10, Action::Accept), (12, Action::Down)]);
    }
}
