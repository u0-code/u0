//! Where programs are found, and how they reach the network. Opened from
//! the Finder or the Dock, an app gets the system's shortest PATH
//! (`/usr/bin:/bin:/usr/sbin:/sbin`) and none of the proxy your shell sets
//! up, so `npx` in `/opt/homebrew/bin` would not be found, neither by the
//! desk asking whether an agent can run nor by the agent it starts, which
//! inherits the desk's environment, and the CLIs an agent calls on
//! (`claude`, `codex`) would go out without your proxy. So the desk takes
//! the PATH your login shell would have, keeps what it was given, and adds
//! the places such tools are usually installed; and it takes the shell's
//! proxy variables where it was given none, once, as it starts.

use std::{
    ffi::OsString,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// The variables that say which proxy a program goes out by, as `curl`,
/// Node and the agents' CLIs read them.
const PROXY: [&str; 8] = [
    "HTTPS_PROXY",
    "https_proxy",
    "HTTP_PROXY",
    "http_proxy",
    "ALL_PROXY",
    "all_proxy",
    "NO_PROXY",
    "no_proxy",
];

/// Put the login shell's PATH, and the usual places, after what is there;
/// take its proxy variables where none was given.
pub fn widen() {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    let shell = login_env().unwrap_or_default();
    let path = shell
        .iter()
        .find(|(k, _)| k == "PATH")
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
    let usual = usual(home.as_deref());
    for dir in std::env::split_paths(&path).chain(usual) {
        if !dir.as_os_str().is_empty() && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    let given = PROXY.iter().any(|k| std::env::var_os(k).is_some());
    // SAFETY: called first thing in `main`, before any other thread
    // exists to read the environment.
    unsafe {
        if let Ok(joined) = std::env::join_paths(dirs) {
            std::env::set_var("PATH", joined);
        }
        if !given {
            for (k, v) in proxies(&shell) {
                std::env::set_var(k, v);
            }
        }
    }
}

/// The proxy variables among a shell's.
fn proxies(env: &[(String, OsString)]) -> impl Iterator<Item = &(String, OsString)> {
    env.iter()
        .filter(|(k, v)| PROXY.contains(&k.as_str()) && !v.is_empty())
}

/// Where command-line tools are usually installed, besides the system's.
fn usual(home: Option<&std::path::Path>) -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ];
    if let Some(home) = home {
        for d in [
            ".local/bin",
            ".cargo/bin",
            ".bun/bin",
            ".npm-global/bin",
            ".volta/bin",
        ] {
            dirs.push(home.join(d));
        }
    }
    dirs
}

/// The environment a login shell sets up, asked of `$SHELL` with a
/// deadline: a shell whose profile waits on something must not hold the
/// desk up.
fn login_env() -> Option<Vec<(String, OsString)>> {
    const MARK: &str = "__u0_env__";
    let shell = std::env::var_os("SHELL").unwrap_or_else(|| "/bin/zsh".into());
    let mut child = Command::new(shell)
        .args([
            "-l",
            "-c",
            &format!("printf '{MARK}'; env -0; printf '{MARK}'"),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut out = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take()?, &mut out).ok()?;
    Some(variables(between(&out, MARK)?))
}

/// `env -0`'s output read as its variables.
fn variables(env: &str) -> Vec<(String, OsString)> {
    env.split('\0')
        .filter_map(|line| line.split_once('='))
        .map(|(k, v)| (k.to_owned(), OsString::from(v)))
        .collect()
}

/// What is between the first two `mark`s in `text`: a profile may print
/// things of its own around it.
fn between<'a>(text: &'a str, mark: &str) -> Option<&'a str> {
    let rest = &text[text.find(mark)? + mark.len()..];
    Some(&rest[..rest.find(mark)?])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_a_profile_prints_around_the_path_is_left_out() {
        let out = "welcome!\n__m__/a:/b__m__\nbye";
        assert_eq!(between(out, "__m__"), Some("/a:/b"));
        assert_eq!(between("no mark here", "__m__"), None);
    }

    #[test]
    fn a_login_shell_s_proxy_is_read_from_its_variables() {
        let env = variables("PATH=/a:/b\0https_proxy=http://127.0.0.1:7897\0NO_PROXY=\0HOME=/x\0");
        let found: Vec<_> = proxies(&env)
            .map(|(k, v)| (k.as_str(), v.clone()))
            .collect();
        assert_eq!(
            found,
            [("https_proxy", OsString::from("http://127.0.0.1:7897"))]
        );
        assert!(env.iter().any(|(k, v)| k == "PATH" && v == "/a:/b"));
    }

    #[test]
    fn the_usual_places_include_homebrew_and_your_own_bin() {
        let dirs = usual(Some(std::path::Path::new("/Users/you")));
        assert!(dirs.contains(&PathBuf::from("/opt/homebrew/bin")));
        assert!(dirs.contains(&PathBuf::from("/Users/you/.local/bin")));
    }
}
