use super::*;
use std::io::Write;
use std::process::{Command, Stdio};

/// Runs `python3 -B <args>` with `input` on stdin; `None` when python3 is
/// missing (the caller skips its test).
pub(crate) fn python(args: &[&str], input: &str) -> Option<String> {
    let mut child = match Command::new("python3")
        .arg("-B")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            eprintln!("SKIPPED: python3 unavailable ({error})");
            return None;
        }
    };
    let mut stdin = child.stdin.take().unwrap();
    let input = input.to_owned();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap().unwrap();
    assert!(output.status.success(), "python3 {args:?} failed");
    Some(String::from_utf8(output.stdout).unwrap())
}

#[test]
fn localization_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Localization>();
}

#[test]
fn python_space_matches_str_isspace() {
    let script = "print(' '.join(str(c) for c in range(0x110000) if chr(c).isspace()))";
    let Some(output) = python(&["-c", script], "") else {
        return;
    };
    let expected: Vec<u32> = output
        .split_whitespace()
        .map(|code| code.parse().unwrap())
        .collect();
    let actual: Vec<u32> = (0..0x110000)
        .filter(|&code| char::from_u32(code).is_some_and(is_python_space))
        .collect();
    assert_eq!(actual, expected);
}
