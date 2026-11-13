//! Privacy mode covers everything cpatop prints outside the UI: argument
//! errors, credential and config errors, hints and subcommand output must
//! not show any part of an e-mail address, IP address or key.

use std::path::{Path, PathBuf};
use std::process::Command;

/// A scratch directory removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!("cpatop-cli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Scratch(p)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Run cpatop with throwaway config/state dirs; returns (exit code,
/// stdout + stderr).
fn run(home: &Path, args: &[&str]) -> (i32, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_cpatop"))
        .args(args)
        .env("XDG_CONFIG_HOME", home.join("cfg"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("HOME", home.join("home"))
        .env("CPATOP_NO_UPDATE_CHECK", "1")
        .env_remove("CPATOP_BASE_URL")
        .env_remove("CPATOP_PASSWORD")
        .env_remove("CPATOP_API_KEY")
        .env_remove("NO_COLOR")
        .stdin(std::process::Stdio::null())
        .output()
        .expect("run cpatop");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    (o.status.code().unwrap_or(-1), text)
}

/// Fragments of the identifiers used below; none may be printed.
const SECRETS: &[&str] = &[
    "alice",
    "bob",
    "carol",
    "dave",
    "erin",
    "frank",
    "example.org",
    "x.io",
    "10.1.2.3",
    "10.9.8.7",
    "2001:db8",
    "abcdefghijklmnopqrst",
    "Zx9Qw8Er7Ty6Ui5Op4As",
    "sk-proj",
];

fn assert_clean(args: &[&str], out: &str) {
    for s in SECRETS {
        assert!(
            !out.to_lowercase().contains(&s.to_lowercase()),
            "{args:?} printed {s:?}:\n{out}"
        );
    }
}

#[test]
fn privacy_scrubs_credential_and_config_errors() {
    let home = Scratch::new("errors");
    let dir = home.0.join("frank@example.org");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("empty"), "").unwrap();
    let empty = dir.join("empty");
    let empty = empty.to_str().unwrap();
    let cases: Vec<Vec<&str>> = vec![
        vec!["--privacy", "--password-file", "/tmp/alice@example.org"],
        vec![
            "--privacy",
            "--password-file",
            "/tmp/alice@example.org/missing",
        ],
        vec!["--privacy", "--api-key-file", "/tmp/10.1.2.3/key"],
        vec!["--privacy", "--api-key-file", "/srv/[2001:db8::1]/key"],
        vec!["--privacy", "--api-key-file", "/k/abcdefghijklmnopqrst"],
        vec!["--privacy", "--api-key-file", "/k/Zx9Qw8Er7Ty6Ui5Op4As/k"],
        vec!["--privacy", "--api-key-file", "/k/sk-proj-0123456789abcdef"],
        vec!["--privacy", "--api-key-file", empty],
        vec!["--privacy", "--config", "/tmp/bob@example.org.toml"],
        vec!["--privacy", "--config", "/tmp/dave+tag@x.io/config.toml"],
        vec!["--privacy", "--base-url", "ftp://10.9.8.7/x"],
        vec!["--privacy", "--base-url", "http://carol@example.org:1"],
        // Flags after a subcommand.
        vec!["logout", "--privacy", "--config", "/tmp/erin@x.io/c.toml"],
    ];
    for args in &cases {
        let (code, out) = run(&home.0, args);
        assert_ne!(code, 0, "{args:?} should fail:\n{out}");
        assert!(!out.trim().is_empty(), "{args:?} printed nothing");
        assert_clean(args, &out);
    }
    // Without privacy the path is shown, so the user can fix it.
    let (_, out) = run(&home.0, &["--password-file", "/tmp/alice@example.org"]);
    assert!(out.contains("/tmp/alice@example.org"), "{out}");
}

#[test]
fn privacy_scrubs_argument_errors() {
    let home = Scratch::new("clap");
    let cases: &[&[&str]] = &[
        &["--privacy", "--range", "bob@example.org"],
        &["--privacy", "--range", "abcdefghijklmnopqrst"],
        &["--privacy", "--theme", "x", "--bogus=carol@10.1.2.3"],
        &["--bogus=dave@x.io", "--privacy"],
        &["--privacy", "--refresh", "10.9.8.7"],
        &["--privacy", "frank@example.org"],
        &["--privacy", "--auth", "Zx9Qw8Er7Ty6Ui5Op4As"],
    ];
    for args in cases {
        let (code, out) = run(&home.0, args);
        assert_eq!(code, 2, "{args:?}:\n{out}");
        assert!(out.contains("error"), "{args:?}:\n{out}");
        assert_clean(args, &out);
    }
}

#[test]
fn privacy_from_the_config_file_scrubs_too() {
    let home = Scratch::new("cfg");
    let cfg = home.0.join("cfg/cpatop");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(cfg.join("config.toml"), "privacy = true\n").unwrap();
    for args in [
        &["--password-file", "/tmp/alice@example.org"][..],
        &["--range", "bob@example.org"][..],
    ] {
        let (code, out) = run(&home.0, args);
        assert_eq!(code, 2, "{args:?}:\n{out}");
        assert_clean(args, &out);
    }
}

/// Quoted local parts with escaped quotes and blanks (`"alice \"bob"@…`)
/// echoed back by argument errors.
#[test]
fn privacy_scrubs_escaped_quoted_emails_in_argument_errors() {
    let home = Scratch::new("quoted");
    for v in [
        r#""alice \"bob"@example.org"#,
        r#""alice \"bob\" smith"@example.org"#,
        r#"x "alice, bob"@example.org"#,
    ] {
        for args in [
            vec!["--privacy", "--range", v],
            vec!["--privacy", "--base-url", v],
            vec!["--privacy", "--theme", "x", v],
        ] {
            let (code, out) = run(&home.0, &args);
            assert_ne!(code, 0, "{args:?}:\n{out}");
            assert_clean(&args, &out);
            assert!(!out.contains("smith"), "{args:?}:\n{out}");
        }
    }
}

/// `cpatop --privacy paths` keeps the useful part of each path.
#[test]
fn privacy_paths_mask_only_identifying_segments() {
    let home = Scratch::new("paths");
    let o = Command::new(env!("CARGO_BIN_EXE_cpatop"))
        .args(["--privacy", "paths"])
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_STATE_HOME")
        .env("HOME", home.0.join("tmp.Zx9Qw8Er7T"))
        .env("CPATOP_NO_UPDATE_CHECK", "1")
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(!out.contains("‹key›"), "{out}");
    assert!(!out.contains("Zx9Qw8Er7T"), "{out}");
    assert!(out.contains("~/.config/cpatop/config.toml"), "{out}");
    assert!(out.contains("~/.local/state/cpatop/session.json"), "{out}");
}

/// A credential file error names the option, and its path only when
/// privacy is off: no nested `(file (‹path›))`.
#[test]
fn credential_file_errors_name_the_option() {
    let home = Scratch::new("credfile");
    let (_, out) = run(
        &home.0,
        &["--privacy", "--password-file", "/tmp/nope/admin.pw"],
    );
    assert!(
        out.contains("cannot read --password-file: no such file"),
        "{out}"
    );
    assert!(!out.contains("(‹path›)") && !out.contains("nope"), "{out}");
    let (_, out) = run(&home.0, &["--password-file", "/tmp/nope/admin.pw"]);
    assert!(
        out.contains("cannot read --password-file /tmp/nope/admin.pw: no such file"),
        "{out}"
    );
    let (_, out) = run(&home.0, &["--api-key-file", "/tmp/nope/k"]);
    assert!(
        out.contains("cannot read --api-key-file /tmp/nope/k"),
        "{out}"
    );
}
