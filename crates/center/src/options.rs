//! Command line of the original `WidgetLoaderApp._parse_arguments`.

use std::path::PathBuf;

pub const HELP: &str = "Usage: linexin-center [OPTIONS]

  -w, --widget NAME       Show only this widget (file name with or without extension)
      --voice             Start voice input in widgets that support it
      --compact           With --widget: open the widget's compact voice window
      --conversation ID   Open this conversation in widgets that support it
  -l, --lang CODE         Override the interface language (e.g. de_DE, pl_PL)
  -v, --version           Print the version and exit

Development options:
      --widgets-dir PATH  Load widgets from PATH instead of the default directory
                          (repeatable; the first directory wins for equal file names)
      --list              Print the discovered widgets without opening a window
  -h, --help              Print this help and exit";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// `-w`: single-widget mode (original `hide_sidebar` + `target_widget`).
    pub target_widget: Option<String>,
    pub voice_autostart: bool,
    pub compact_mode: bool,
    pub conversation_id: Option<String>,
    pub language: Option<String>,
    pub widgets_dirs: Vec<PathBuf>,
    pub list: bool,
    /// Arguments the original handed to GTK; GApplication has no use for them.
    pub ignored: Vec<String>,
}

impl Options {
    pub fn hide_sidebar(&self) -> bool {
        self.target_widget.is_some()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Parsed {
    Run(Options),
    /// Print to stdout and exit successfully (`--version`, `--help`).
    Print(String),
    /// Print to stderr and exit with status 1.
    Fail(String),
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Parsed {
    let mut options = Options::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        // Like the original, the value is the next argument whatever it looks like.
        let mut value = |error: &str| args.next().ok_or_else(|| error.to_owned());
        let result = match arg.as_str() {
            "-w" | "--widget" => value("Error: --widget requires a widget name")
                .map(|v| options.target_widget = Some(v)),
            "--voice" => {
                options.voice_autostart = true;
                Ok(())
            }
            "--compact" => {
                options.compact_mode = true;
                Ok(())
            }
            "--conversation" => value("Error: --conversation requires a conversation id")
                .map(|v| options.conversation_id = Some(v)),
            "-l" | "--lang" => value("Error: --lang requires a language code (e.g. de_DE, fr_FR)")
                .map(|v| options.language = Some(v)),
            "-v" | "--version" => {
                return Parsed::Print(format!("Linexin Tools v{}", env!("CARGO_PKG_VERSION")));
            }
            "--widgets-dir" => value("Error: --widgets-dir requires a directory")
                .map(|v| options.widgets_dirs.push(PathBuf::from(v))),
            "--list" => {
                options.list = true;
                Ok(())
            }
            "-h" | "--help" => return Parsed::Print(HELP.to_owned()),
            _ => {
                options.ignored.push(arg);
                Ok(())
            }
        };
        if let Err(message) = result {
            return Parsed::Fail(message);
        }
    }
    Parsed::Run(options)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str]) -> Options {
        match parse(args.iter().map(|s| (*s).to_owned())) {
            Parsed::Run(options) => options,
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn original_options() {
        let options = run(&[
            "-w",
            "a-system-updater",
            "--voice",
            "--compact",
            "--conversation",
            "42",
            "-l",
            "pl_PL",
        ]);
        assert_eq!(options.target_widget.as_deref(), Some("a-system-updater"));
        assert!(options.hide_sidebar());
        assert!(options.voice_autostart && options.compact_mode);
        assert_eq!(options.conversation_id.as_deref(), Some("42"));
        assert_eq!(options.language.as_deref(), Some("pl_PL"));
        let long = run(&["--widget", "x", "--lang", "de_DE"]);
        assert_eq!(long.target_widget.as_deref(), Some("x"));
        assert_eq!(long.language.as_deref(), Some("de_DE"));
        assert!(!run(&[]).hide_sidebar());
    }

    #[test]
    fn version_and_help() {
        let version = format!("Linexin Tools v{}", env!("CARGO_PKG_VERSION"));
        for flag in ["-v", "--version"] {
            assert_eq!(parse([flag.to_owned()]), Parsed::Print(version.clone()));
        }
        assert_eq!(parse(["-h".to_owned()]), Parsed::Print(HELP.to_owned()));
        // Like the original, options after --version are never looked at.
        assert_eq!(
            parse(["-v".to_owned(), "-w".to_owned()]),
            Parsed::Print(version)
        );
    }

    #[test]
    fn missing_values_fail() {
        for flag in [
            "-w",
            "--widget",
            "--conversation",
            "-l",
            "--lang",
            "--widgets-dir",
        ] {
            assert!(
                matches!(parse([flag.to_owned()]), Parsed::Fail(_)),
                "{flag}"
            );
        }
    }

    #[test]
    fn values_are_taken_verbatim_and_unknown_arguments_ignored() {
        let options = run(&["--gapplication-service", "-w", "--voice", "file.txt"]);
        assert_eq!(options.target_widget.as_deref(), Some("--voice"));
        assert!(!options.voice_autostart);
        assert_eq!(options.ignored, ["--gapplication-service", "file.txt"]);
    }

    #[test]
    fn widgets_dirs_are_repeatable_in_order() {
        let options = run(&["--widgets-dir", "/a", "--list", "--widgets-dir", "/b"]);
        assert_eq!(
            options.widgets_dirs,
            [PathBuf::from("/a"), PathBuf::from("/b")]
        );
        assert!(options.list);
    }
}
