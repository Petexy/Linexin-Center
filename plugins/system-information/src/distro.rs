//! The part of python-distro (1.9) the original widget uses: `distro.name(pretty=True)`
//! and, when that is empty, `f"{distro.id()} {distro.version()}"`.

use std::cell::OnceCell;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;

use regex::Regex;

use crate::info;

type Props = HashMap<String, String>;

const DISTRO_RELEASE_BASENAMES: &[&str] = &[
    "SuSE-release",
    "altlinux-release",
    "arch-release",
    "base-release",
    "centos-release",
    "fedora-release",
    "gentoo-release",
    "mageia-release",
    "mandrake-release",
    "mandriva-release",
    "mandrivalinux-release",
    "manjaro-release",
    "oracle-release",
    "redhat-release",
    "rocky-release",
    "sl-release",
    "slackware-version",
];

const DISTRO_RELEASE_IGNORE_BASENAMES: &[&str] = &[
    "debian_version",
    "lsb-release",
    "oem-release",
    "os-release",
    "system-release",
    "plesk-release",
    "iredmail-release",
    "board-release",
    "ec2_version",
];

const NORMALIZED_OS_ID: &[(&str, &str)] = &[("ol", "oracle"), ("opensuse-leap", "opensuse")];
const NORMALIZED_LSB_ID: &[(&str, &str)] = &[
    ("enterpriseenterpriseas", "oracle"),
    ("enterpriseenterpriseserver", "oracle"),
    ("redhatenterpriseworkstation", "rhel"),
    ("redhatenterpriseserver", "rhel"),
    ("redhatenterprisecomputenode", "rhel"),
];
const NORMALIZED_DISTRO_ID: &[(&str, &str)] = &[("redhat", "rhel")];

static RELEASE_CONTENT_REVERSED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:[^)]*\)(.*)\()? *(?:STL )?([\d.+\-a-z]*\d) *(?:esaeler *)?(.+)")
        .expect("valid pattern")
});
static RELEASE_BASENAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\w+)[-_](release|version)$").expect("valid pattern"));
static VERSION_CODENAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\((\D+)\)|,\s*(\D+)").expect("valid pattern"));

/// The OS name shown in the header, exactly as the original computes it.
pub fn os_name() -> io::Result<String> {
    let distro = Distro::open(Path::new("/etc"), Path::new("/usr/lib"), true)?;
    let name = distro.pretty_name();
    Ok(if name.is_empty() {
        format!("{} {}", distro.id(), distro.version(false))
    } else {
        name
    })
}

/// `distro.LinuxDistribution`. The `uname -rs` source is left out: python-distro
/// discards it whenever the system name is `Linux`.
struct Distro {
    etc_dir: PathBuf,
    include_lsb: bool,
    os_release: Props,
    lsb_release: OnceCell<Props>,
    distro_release: OnceCell<Props>,
}

impl Distro {
    fn open(etc_dir: &Path, usr_lib_dir: &Path, include_lsb: bool) -> io::Result<Self> {
        let etc_file = etc_dir.join("os-release");
        let usr_lib_file = usr_lib_dir.join("os-release");
        let file = if etc_file.is_file() || !usr_lib_file.is_file() {
            etc_file
        } else {
            usr_lib_file
        };
        let os_release = if file.is_file() {
            parse_os_release(&info::read_text(file)?)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
        } else {
            Props::new()
        };
        Ok(Self {
            etc_dir: etc_dir.to_owned(),
            include_lsb,
            os_release,
            lsb_release: OnceCell::new(),
            distro_release: OnceCell::new(),
        })
    }

    fn os_release_attr(&self, key: &str) -> &str {
        self.os_release.get(key).map_or("", String::as_str)
    }

    fn lsb_release_attr(&self, key: &str) -> &str {
        self.lsb_release
            .get_or_init(|| {
                if self.include_lsb {
                    lsb_release()
                } else {
                    Props::new()
                }
            })
            .get(key)
            .map_or("", String::as_str)
    }

    fn distro_release_attr(&self, key: &str) -> &str {
        self.distro_release
            .get_or_init(|| distro_release_info(&self.etc_dir))
            .get(key)
            .map_or("", String::as_str)
    }

    fn pretty_name(&self) -> String {
        let name = first_nonempty(&[
            self.os_release_attr("pretty_name"),
            self.lsb_release_attr("description"),
        ]);
        if !name.is_empty() {
            return name.to_owned();
        }
        let name = self.distro_release_attr("name");
        let version = self.version(true);
        if version.is_empty() {
            name.to_owned()
        } else {
            format!("{name} {version}")
        }
    }

    fn id(&self) -> String {
        let normalize = |id: &str, table: &[(&str, &str)]| {
            let id = id.to_lowercase().replace(' ', "_");
            table
                .iter()
                .find(|(from, _)| *from == id)
                .map_or(id, |(_, to)| (*to).to_owned())
        };
        for (id, table) in [
            (self.os_release_attr("id"), NORMALIZED_OS_ID),
            (self.lsb_release_attr("distributor_id"), NORMALIZED_LSB_ID),
            (self.distro_release_attr("id"), NORMALIZED_DISTRO_ID),
        ] {
            if !id.is_empty() {
                return normalize(id, table);
            }
        }
        String::new()
    }

    fn version(&self, pretty: bool) -> String {
        let from_pretty_name = parse_distro_release_content(self.os_release_attr("pretty_name"));
        let from_description = parse_distro_release_content(self.lsb_release_attr("description"));
        let mut versions = vec![
            self.os_release_attr("version_id").to_owned(),
            self.lsb_release_attr("release").to_owned(),
            self.distro_release_attr("version_id").to_owned(),
            from_pretty_name
                .get("version_id")
                .cloned()
                .unwrap_or_default(),
            from_description
                .get("version_id")
                .cloned()
                .unwrap_or_default(),
        ];
        let like = self.os_release_attr("id_like");
        if self.id() == "debian" || like.split_whitespace().any(|id| id == "debian") {
            versions.push(debian_version(&self.etc_dir));
        }
        let version = versions
            .into_iter()
            .find(|v| !v.is_empty())
            .unwrap_or_default();
        let codename = self.codename();
        if pretty && !version.is_empty() && !codename.is_empty() {
            format!("{version} ({codename})")
        } else {
            version
        }
    }

    fn codename(&self) -> &str {
        // An explicitly empty os-release codename means "no codename".
        match self.os_release.get("codename") {
            Some(codename) => codename,
            None => first_nonempty(&[
                self.lsb_release_attr("codename"),
                self.distro_release_attr("codename"),
            ]),
        }
    }
}

fn first_nonempty<'a>(values: &[&'a str]) -> &'a str {
    values.iter().copied().find(|v| !v.is_empty()).unwrap_or("")
}

fn parse_os_release(content: &str) -> Result<Props, &'static str> {
    let mut props = Props::new();
    for token in shlex_split(content)? {
        if let Some((key, value)) = token.split_once('=') {
            props.insert(key.to_lowercase(), value.to_owned());
        }
    }
    if let Some(captures) = props
        .get("version")
        .and_then(|v| VERSION_CODENAME.captures(v))
    {
        let codename = captures
            .get(1)
            .or_else(|| captures.get(2))
            .map_or(String::new(), |m| m.as_str().to_owned());
        props.insert("codename".into(), codename.clone());
        props.insert("release_codename".into(), codename);
    }
    if let Some(codename) = props
        .get("version_codename")
        .or_else(|| props.get("ubuntu_codename"))
        .cloned()
    {
        props.insert("codename".into(), codename);
    }
    Ok(props)
}

/// Tokens of Python's `shlex.shlex(text, posix=True)` with `whitespace_split = True`.
fn shlex_split(text: &str) -> Result<Vec<String>, &'static str> {
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        Space,
        Word,
        Quote(char),
        // Backslash seen; remembers whether it was inside double quotes.
        Escape { in_quotes: bool },
    }
    let is_space = |c: char| matches!(c, ' ' | '\t' | '\r' | '\n');
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut quoted = false;
    let mut state = State::Space;
    let mut chars = text.chars();
    let skip_comment = |chars: &mut std::str::Chars| {
        for c in chars.by_ref() {
            if c == '\n' {
                break;
            }
        }
    };
    loop {
        let next = chars.next();
        match state {
            State::Space => match next {
                None => break,
                Some(c) if is_space(c) => {}
                Some('#') => skip_comment(&mut chars),
                Some('\\') => state = State::Escape { in_quotes: false },
                Some(c @ ('"' | '\'')) => state = State::Quote(c),
                Some(c) => {
                    token.push(c);
                    state = State::Word;
                }
            },
            State::Quote(quote) => {
                quoted = true;
                match next {
                    None => return Err("No closing quotation"),
                    Some(c) if c == quote => state = State::Word,
                    Some('\\') if quote == '"' => state = State::Escape { in_quotes: true },
                    Some(c) => token.push(c),
                }
            }
            State::Escape { in_quotes } => {
                let Some(c) = next else {
                    return Err("No escaped character");
                };
                // Inside quotes only the quote itself or a backslash is escaped.
                if in_quotes && c != '\\' && c != '"' {
                    token.push('\\');
                }
                token.push(c);
                state = if in_quotes {
                    State::Quote('"')
                } else {
                    State::Word
                };
            }
            State::Word => match next {
                None => break,
                Some(c) if is_space(c) || c == '#' => {
                    if c == '#' {
                        skip_comment(&mut chars);
                    }
                    state = State::Space;
                    if !token.is_empty() || quoted {
                        tokens.push(std::mem::take(&mut token));
                        quoted = false;
                    }
                }
                Some(c @ ('"' | '\'')) => state = State::Quote(c),
                Some('\\') => state = State::Escape { in_quotes: false },
                Some(c) => token.push(c),
            },
        }
    }
    if !token.is_empty() || quoted {
        tokens.push(token);
    }
    Ok(tokens)
}

fn lsb_release() -> Props {
    let output = Command::new("lsb_release")
        .arg("-a")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    match output {
        Ok(output) if output.status.success() => {
            parse_lsb_release(&String::from_utf8_lossy(&output.stdout))
        }
        _ => Props::new(),
    }
}

fn parse_lsb_release(content: &str) -> Props {
    content
        .lines()
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| {
            (
                key.replace(' ', "_").to_lowercase(),
                value.trim().to_owned(),
            )
        })
        .collect()
}

fn distro_release_info(etc_dir: &Path) -> Props {
    let mut basenames: Vec<String> = match fs::read_dir(etc_dir) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .filter(|name| {
                !DISTRO_RELEASE_IGNORE_BASENAMES.contains(&name.as_str())
                    && etc_dir.join(name).is_file()
            })
            .collect(),
        Err(_) => DISTRO_RELEASE_BASENAMES
            .iter()
            .map(|&n| n.to_owned())
            .collect(),
    };
    basenames.sort();
    for basename in basenames {
        let Some(captures) = RELEASE_BASENAME.captures(&basename) else {
            continue;
        };
        let Ok(content) = fs::read(etc_dir.join(&basename)) else {
            continue;
        };
        let content = info::universal_newlines(&String::from_utf8_lossy(&content));
        // Python's text-mode readline(): the first line, terminator included.
        let first_line = match content.find('\n') {
            Some(end) => &content[..=end],
            None => &content,
        };
        let mut info = parse_distro_release_content(first_line);
        let Some(name) = info.get("name") else {
            continue;
        };
        let id = if name.to_lowercase().contains("cloudlinux") {
            "cloudlinux".to_owned()
        } else {
            captures[1].to_owned()
        };
        info.insert("id".into(), id);
        return info;
    }
    Props::new()
}

fn parse_distro_release_content(line: &str) -> Props {
    let reversed: String = line.trim().chars().rev().collect();
    let unreverse = |text: &str| text.chars().rev().collect::<String>();
    let mut info = Props::new();
    if let Some(captures) = RELEASE_CONTENT_REVERSED.captures(&reversed) {
        info.insert("name".into(), unreverse(&captures[3]));
        if let Some(version) = captures.get(2).filter(|m| !m.is_empty()) {
            info.insert("version_id".into(), unreverse(version.as_str()));
        }
        if let Some(codename) = captures.get(1).filter(|m| !m.is_empty()) {
            info.insert("codename".into(), unreverse(codename.as_str()));
        }
    } else if !line.is_empty() {
        info.insert("name".into(), line.trim().to_owned());
    }
    info
}

fn debian_version(etc_dir: &Path) -> String {
    fs::read_to_string(etc_dir.join("debian_version"))
        .ok()
        .and_then(|text| text.lines().next().map(|line| line.trim_end().to_owned()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Root(PathBuf);

    impl Root {
        fn new(name: &str, files: &[(&str, &str)]) -> Self {
            let root =
                std::env::temp_dir().join(format!("linexin-distro-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            for dir in ["etc", "usr/lib"] {
                fs::create_dir_all(root.join(dir)).unwrap();
            }
            for (path, content) in files {
                fs::write(root.join(path), content).unwrap();
            }
            Self(root)
        }

        fn distro(&self) -> io::Result<Distro> {
            Distro::open(&self.0.join("etc"), &self.0.join("usr/lib"), false)
        }
    }

    impl Drop for Root {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn shlex_matches_python() {
        let tokens =
            shlex_split("A=\"x y\" # comment\nB='it''s' C=a\\ b#tail\nD=\"q\\\"\\$\\\\\" E=\"\"\n")
                .unwrap();
        assert_eq!(tokens, ["A=x y", "B=its", "C=a b", "D=q\"\\$\\", "E="]);
        assert!(shlex_split("A=\"open").is_err());
        assert!(shlex_split("A=\\").is_err());
    }

    #[test]
    fn pretty_name_comes_from_os_release() {
        let root = Root::new(
            "pretty",
            &[(
                "etc/os-release",
                "NAME=\"Linexin\"\nPRETTY_NAME=\"Linexin\"\nVERSION_ID=\"2.1\"\n",
            )],
        );
        assert_eq!(root.distro().unwrap().pretty_name(), "Linexin");
    }

    #[test]
    fn usr_lib_os_release_is_the_fallback() {
        let root = Root::new(
            "usrlib",
            &[("usr/lib/os-release", "PRETTY_NAME='Arch Linux'\n")],
        );
        assert_eq!(root.distro().unwrap().pretty_name(), "Arch Linux");
    }

    #[test]
    fn release_file_and_version_build_the_name() {
        let root = Root::new(
            "release",
            &[
                (
                    "etc/os-release",
                    "ID=centos\nVERSION_ID=7\nVERSION=\"7 (Core)\"\n",
                ),
                (
                    "etc/centos-release",
                    "CentOS Linux release 7.1.1503 (Core)\n",
                ),
            ],
        );
        let distro = root.distro().unwrap();
        assert_eq!(distro.pretty_name(), "CentOS Linux 7 (Core)");
        assert_eq!(distro.id(), "centos");
    }

    #[test]
    fn empty_arch_release_gives_a_version_only_name() {
        let root = Root::new(
            "arch",
            &[
                ("etc/os-release", "ID=arch\nVERSION_ID=rolling\n"),
                ("etc/arch-release", ""),
            ],
        );
        assert_eq!(root.distro().unwrap().pretty_name(), " rolling");
    }

    #[test]
    fn nothing_known_falls_back_to_id_and_version() {
        let root = Root::new("empty", &[("etc/os-release", "ID=\"Some OS\"\n")]);
        let distro = root.distro().unwrap();
        assert_eq!(distro.pretty_name(), "");
        assert_eq!(
            format!("{} {}", distro.id(), distro.version(false)),
            "some_os "
        );
    }

    #[test]
    fn unbalanced_quotes_are_an_error() {
        let root = Root::new("broken", &[("etc/os-release", "PRETTY_NAME=\"Linexin\n")]);
        assert!(root.distro().is_err());
    }

    #[test]
    fn release_content_parsing_matches_python() {
        let info = parse_distro_release_content("Fedora release 38 (Thirty Eight)\n");
        assert_eq!(info["name"], "Fedora");
        assert_eq!(info["version_id"], "38");
        assert_eq!(info["codename"], "Thirty Eight");
        assert_eq!(parse_distro_release_content("Linexin")["name"], "Linexin");
        assert!(parse_distro_release_content("").is_empty());
    }

    #[test]
    fn lsb_release_output_is_parsed() {
        let props = parse_lsb_release(
            "LSB Version:\tn/a\nDistributor ID:\tArch\nDescription:\tArch Linux\n",
        );
        assert_eq!(props["distributor_id"], "Arch");
        assert_eq!(props["description"], "Arch Linux");
    }
}
