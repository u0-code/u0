//! iOS simulators and Android emulators on the machine the server runs on,
//! as t3code's Devices panel has them: listed, started and stopped, their
//! screen read frame after frame, touched and typed into. Driven by Xcode's
//! `xcrun simctl` and the Android SDK's `adb` and `emulator`, where they are
//! installed. `simctl` takes no touch: an iOS simulator is touched, and its
//! screen streamed, through [AXe](https://github.com/cameroncooke/AXe)
//! where it is installed (it sends the simulator's own touch events, with
//! no test runner built), else in Simulator itself.

use std::{
    collections::HashMap,
    io::Read as _,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Condvar, LazyLock, Mutex},
    time::{Duration, Instant},
};

use rmcp::schemars;
use serde::{Deserialize, Serialize};

/// A simulator or an emulator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    /// The simulator's UDID; the emulator's AVD name, or a running
    /// device's adb serial.
    pub id: String,
    pub name: String,
    pub android: bool,
    /// adb's serial for an Android one running.
    pub serial: Option<String>,
    pub running: bool,
}

/// What can be done to a device; as an agent names it too.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "act", rename_all = "snake_case")]
pub enum Act {
    Start,
    Stop,
    /// Press a key of it.
    Key {
        key: Key,
    },
    /// Touch this point of its screen, in the pixels of its picture.
    Tap {
        x: u32,
        y: u32,
    },
    Type {
        text: String,
    },
    /// Dark appearance, or light.
    Dark {
        dark: bool,
    },
}

/// Home, and Android's Back and Recents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Key {
    Home,
    Back,
    Recents,
}

/// What a tool printed, or what it said went wrong.
fn run(program: &PathBuf, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{} could not be started: {e}", program.display()))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}

fn text(program: &PathBuf, args: &[&str]) -> Result<String, String> {
    run(program, args).map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// `AXe`, which touches and streams an iOS simulator.
fn axe() -> Option<PathBuf> {
    cfg!(target_os = "macos")
        .then(|| which::which("axe").ok())
        .flatten()
}

fn xcrun() -> Option<PathBuf> {
    cfg!(target_os = "macos")
        .then(|| which::which("xcrun").ok())
        .flatten()
}

/// One of the Android SDK's tools: on the PATH, or under `ANDROID_HOME`
/// (or `ANDROID_SDK_ROOT`, or where Android Studio puts the SDK).
fn sdk(dir: &str, tool: &str) -> Option<PathBuf> {
    which::which(tool).ok().or_else(|| {
        let home = std::env::var_os("ANDROID_HOME")
            .or_else(|| std::env::var_os("ANDROID_SDK_ROOT"))
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Android/sdk"))
            })?;
        Some(home.join(dir).join(tool)).filter(|p| p.is_file())
    })
}

/// The devices here, and what is missing to have any; the iOS ones first.
pub fn list() -> (Vec<Device>, Vec<String>) {
    let (mut found, mut missing) = (Vec::new(), Vec::new());
    match xcrun().map(|x| text(&x, &["simctl", "list", "devices", "available", "--json"])) {
        Some(Ok(json)) => found.extend(simulators(&json)),
        Some(Err(why)) => missing.push(format!("Xcode's simulators: {why}")),
        None => missing.push("Xcode, for iOS simulators".into()),
    }
    match (sdk("platform-tools", "adb"), sdk("emulator", "emulator")) {
        (Some(adb), emulator) => {
            let avds = (emulator.as_ref())
                .and_then(|e| text(e, &["-list-avds"]).ok())
                .unwrap_or_default();
            let running = text(&adb, &["devices"]).unwrap_or_default();
            let name_of = |serial: &str| {
                text(&adb, &["-s", serial, "emu", "avd", "name"])
                    .ok()
                    .and_then(|t| t.lines().next().map(|l| l.trim().to_owned()))
            };
            found.extend(androids(&avds, &running, name_of));
        }
        (None, _) => missing.push("the Android SDK's platform tools, for emulators".into()),
    }
    (found, missing)
}

/// The devices here, and what is missing, as the server says them.
pub fn listed() -> crate::proto::Event {
    let (found, missing) = list();
    crate::proto::Event::Devices { found, missing }
}

/// The simulators `simctl list devices --json` names.
fn simulators(json: &str) -> Vec<Device> {
    #[derive(Deserialize)]
    struct Listed {
        devices: std::collections::BTreeMap<String, Vec<Sim>>,
    }
    #[derive(Deserialize)]
    struct Sim {
        udid: String,
        name: String,
        state: String,
    }
    let Ok(listed) = serde_json::from_str::<Listed>(json) else {
        return Vec::new();
    };
    let mut found: Vec<Device> = (listed.devices.into_iter())
        .filter(|(runtime, _)| runtime.contains("iOS"))
        .flat_map(|(_, sims)| sims)
        .map(|s| Device {
            id: s.udid,
            name: s.name,
            android: false,
            serial: None,
            running: s.state == "Booted",
        })
        .collect();
    found.sort_by(|a, b| b.running.cmp(&a.running).then(a.name.cmp(&b.name)));
    found
}

/// The emulators `emulator -list-avds` names, each with the serial `adb
/// devices` lists it under while it runs (`name_of` asks a running one its
/// name); a device adb lists that is no emulator of these, by its serial.
fn androids(avds: &str, running: &str, name_of: impl Fn(&str) -> Option<String>) -> Vec<Device> {
    let serials: Vec<&str> = (running.lines().skip(1))
        .filter_map(|l| l.split_once('\t'))
        .filter(|(_, state)| state.trim() == "device")
        .map(|(serial, _)| serial)
        .collect();
    let mut found: Vec<Device> = (avds.lines().map(str::trim).filter(|a| !a.is_empty()))
        .map(|avd| Device {
            id: avd.to_owned(),
            name: avd.replace('_', " "),
            android: true,
            serial: None,
            running: false,
        })
        .collect();
    for serial in serials {
        let name = serial
            .starts_with("emulator-")
            .then(|| name_of(serial))
            .flatten();
        match found.iter_mut().find(|d| Some(&d.id) == name.as_ref()) {
            Some(d) => (d.serial, d.running) = (Some(serial.to_owned()), true),
            None => found.push(Device {
                id: serial.to_owned(),
                name: serial.to_owned(),
                android: true,
                serial: Some(serial.to_owned()),
                running: true,
            }),
        }
    }
    found
}

/// Do `act` to `device`.
pub fn act(device: &Device, act: &Act) -> Result<(), String> {
    if !device.android {
        let x = xcrun().ok_or("Xcode is not installed")?;
        let id = device.id.as_str();
        return match act {
            Act::Start => run(&x, &["simctl", "boot", id]).map(|_| {
                // Without AXe it is touched in its window: Simulator, of
                // the Xcode simctl is.
                let xcode = std::env::var_os("DEVELOPER_DIR")
                    .map(PathBuf::from)
                    .or_else(|| {
                        text(&PathBuf::from("/usr/bin/xcode-select"), &["-p"])
                            .ok()
                            .map(|p| PathBuf::from(p.trim()))
                    });
                if axe().is_none()
                    && let Some(app) = xcode.map(|d| d.join("Applications/Simulator.app"))
                {
                    let _ = run(&PathBuf::from("/usr/bin/open"), &[&app.to_string_lossy()]);
                }
                Vec::new()
            }),
            Act::Stop => run(&x, &["simctl", "shutdown", id]),
            Act::Dark { dark } => run(
                &x,
                &[
                    "simctl",
                    "ui",
                    id,
                    "appearance",
                    if *dark { "dark" } else { "light" },
                ],
            ),
            Act::Key {
                key: Key::Back | Key::Recents,
            } => Err("An iOS simulator has no such key.".into()),
            Act::Key { .. } | Act::Tap { .. } | Act::Type { .. } => {
                let axe = axe().ok_or(
                    "An iOS simulator is touched in Ghost with AXe (brew install cameroncooke/axe/axe), else in Simulator.",
                )?;
                touch(&axe, id, act)
            }
        }
        .map(drop);
    }
    if *act == Act::Start {
        let emulator =
            sdk("emulator", "emulator").ok_or("the Android emulator is not installed")?;
        // It runs on after this returns, as Android Studio leaves it.
        return Command::new(emulator)
            .args(["-avd", &device.id])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(drop)
            .map_err(|e| e.to_string());
    }
    let adb = sdk("platform-tools", "adb").ok_or("adb is not installed")?;
    let serial = device.serial.as_deref().ok_or("it is not running")?;
    let shell = |args: &[&str]| {
        let mut all = vec!["-s", serial, "shell"];
        all.extend_from_slice(args);
        run(&adb, &all).map(drop)
    };
    match act {
        Act::Start => unreachable!("started above"),
        Act::Stop => run(&adb, &["-s", serial, "emu", "kill"]).map(drop),
        Act::Key { key } => shell(&[
            "input",
            "keyevent",
            match key {
                Key::Home => "KEYCODE_HOME",
                Key::Back => "KEYCODE_BACK",
                Key::Recents => "KEYCODE_APP_SWITCH",
            },
        ]),
        Act::Tap { x, y } => shell(&["input", "tap", &x.to_string(), &y.to_string()]),
        Act::Type { text } => shell(&["input", "text", &typed(text)]),
        Act::Dark { dark } => shell(&["cmd", "uimode", "night", if *dark { "yes" } else { "no" }]),
    }
}

/// Touch, type into or press Home on the iOS simulator `udid` with `AXe`,
/// which takes points: a tap in pixels is divided by its screen's scale.
fn touch(axe: &PathBuf, udid: &str, act: &Act) -> Result<Vec<u8>, String> {
    match act {
        Act::Tap { x, y } => {
            let said = text(axe, &["--output", "json", "displays", "--udid", udid])?;
            let scale = scale(&said).ok_or("AXe did not say its screen's scale")?;
            let (x, y) = ((x / scale).to_string(), (y / scale).to_string());
            run(axe, &["tap", "-x", &x, "-y", &y, "--udid", udid])
        }
        Act::Type { text } => run(axe, &["type", text, "--udid", udid]),
        _ => run(axe, &["button", "home", "--udid", udid]),
    }
}

/// The scale of the active screen, of what `axe --output json displays`
/// says.
fn scale(said: &str) -> Option<u32> {
    let said: serde_json::Value = serde_json::from_str(said).ok()?;
    let scale = said.pointer("/data/result/activeScreen/scale")?.as_u64()?;
    u32::try_from(scale).ok().filter(|s| *s > 0)
}

/// `words` as one word of the emulator's shell, which reads what `adb shell`
/// is given: quoted, so `;` or `$(…)` are typed rather than run, and with
/// spaces as `%s`, which `input text` takes for a space.
fn typed(words: &str) -> String {
    shell_words::quote(&words.replace(' ', "%s")).into_owned()
}

/// What `device` shows now, as a PNG or a JPEG: where it is streamed, a
/// frame not handed out before.
pub fn screen(device: &Device) -> Result<Vec<u8>, String> {
    if let Some(axe) = axe().filter(|_| !device.android) {
        streamed(&axe, &device.id)
    } else if device.android {
        let adb = sdk("platform-tools", "adb").ok_or("adb is not installed")?;
        let serial = device.serial.as_deref().ok_or("it is not running")?;
        run(&adb, &["-s", serial, "exec-out", "screencap", "-p"])
    } else {
        let x = xcrun().ok_or("Xcode is not installed")?;
        // simctl writes it only to a file.
        let file = tempfile::Builder::new()
            .suffix(".png")
            .tempfile()
            .map_err(|e| e.to_string())?;
        let at = file.path().to_string_lossy().into_owned();
        let shot = |display: Option<&str>| {
            let mut args = vec!["simctl", "io", &device.id, "screenshot", "--type=png"];
            args.extend(display);
            args.push(&at);
            run(&x, &args)
        };
        match shot(None) {
            // A device of two screens (a foldable) is asked for its first.
            Err(why) if why.contains("--display") => shot(primary(&why).as_deref())?,
            done => done?,
        };
        std::fs::read(file.path()).map_err(|e| e.to_string())
    }
}

/// A simulator's screen as `AXe` streams it: the newest frame, numbered;
/// the number last handed out; when one was last asked for; why it ended.
#[derive(Default)]
struct Frames {
    jpeg: Vec<u8>,
    n: u64,
    given: u64,
    asked: Option<Instant>,
    ended: Option<String>,
}

type Stream = Arc<(Mutex<Frames>, Condvar)>;

/// The simulators streamed, by UDID.
static STREAMS: LazyLock<Mutex<HashMap<String, Stream>>> = LazyLock::new(Mutex::default);

/// How long a stream nobody asks of runs on.
const UNWATCHED: Duration = Duration::from_secs(10);

/// The newest frame of the simulator `udid` not handed out yet, waiting up
/// to a second for one; its stream started as it is first asked for.
fn streamed(axe: &Path, udid: &str) -> Result<Vec<u8>, String> {
    let stream = (STREAMS.lock().expect("streams"))
        .entry(udid.to_owned())
        .or_insert_with(|| stream(axe, udid))
        .clone();
    let (frames, came) = &*stream;
    let mut f = frames.lock().expect("frames");
    f.asked = Some(Instant::now());
    f = (came.wait_timeout_while(f, Duration::from_secs(1), |f| {
        f.n == f.given && f.ended.is_none()
    }))
    .expect("frames")
    .0;
    if let Some(why) = &f.ended {
        return Err(why.clone());
    }
    f.given = f.n;
    (f.n > 0)
        .then(|| f.jpeg.clone())
        .ok_or_else(|| "its screen has not come yet".into())
}

/// `axe stream-video` of `udid`, each frame kept as it comes, until nobody
/// asks for one.
fn stream(axe: &Path, udid: &str) -> Stream {
    let stream = Stream::default();
    let child = Command::new(axe)
        .args([
            "stream-video",
            "--udid",
            udid,
            "--fps",
            "30",
            "--format",
            "raw",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let (kept, udid) = (stream.clone(), udid.to_owned());
    std::thread::spawn(move || {
        let (frames, came) = &*kept;
        let why = match child {
            Ok(mut child) => {
                let why = frames_of(child.stdout.take().expect("piped"), frames, came);
                let _ = child.kill();
                let _ = child.wait();
                why
            }
            Err(e) => format!("AXe could not be started: {e}"),
        };
        let mut streams = STREAMS.lock().expect("streams");
        if streams.get(&udid).is_some_and(|s| Arc::ptr_eq(s, &kept)) {
            streams.remove(&udid);
        }
        frames.lock().expect("frames").ended = Some(why);
        came.notify_all();
    });
    stream
}

/// Each frame read from `out` (four bytes of its length, then a JPEG) into
/// `frames`, until it ends or nobody has asked for one in a while; why it
/// stopped.
fn frames_of(mut out: impl std::io::Read, frames: &Mutex<Frames>, came: &Condvar) -> String {
    loop {
        let mut len = [0; 4];
        if let Err(e) = out.read_exact(&mut len) {
            return format!("its stream ended: {e}");
        }
        let mut jpeg = Vec::new();
        let len = u64::from(u32::from_be_bytes(len));
        if let Err(e) = (&mut out).take(len).read_to_end(&mut jpeg) {
            return format!("its stream ended: {e}");
        }
        let mut f = frames.lock().expect("frames");
        if f.asked.is_some_and(|at| at.elapsed() > UNWATCHED) {
            return "nobody watched it".into();
        }
        (f.jpeg, f.n) = (jpeg, f.n + 1);
        came.notify_all();
    }
}

/// The `--display=` of the primary screen, of those simctl lists when a
/// device has more than one.
fn primary(said: &str) -> Option<String> {
    said.lines()
        .find(|l| l.contains("(primary)"))
        .and_then(|l| l.split_whitespace().next())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_typed_on_an_emulator_is_one_word_its_shell_never_runs() {
        for words in ["a;reboot", "don't", "$(rm -rf /)", "`id`", "two words"] {
            let word = typed(words);
            assert_eq!(
                shell_words::split(&word).unwrap(),
                [words.replace(' ', "%s")],
                "{words}"
            );
        }
    }

    #[test]
    fn a_tap_on_an_ios_simulator_is_scaled_by_what_axe_says_of_its_screen() {
        let said = r#"{"command":"displays","data":{"result":{"udid":"U","name":"iPhone",
            "activeScreen":{"id":1,"width":402,"height":874,"scale":3}}},"didError":false}"#;
        assert_eq!(scale(said), Some(3));
        assert_eq!(scale(r#"{"didError":true}"#), None);
    }

    #[test]
    fn a_stream_keeps_its_newest_frame_and_ends_when_nobody_watches() {
        let mut bytes = Vec::new();
        for jpeg in [&b"one"[..], b"two!"] {
            bytes.extend(u32::try_from(jpeg.len()).unwrap().to_be_bytes());
            bytes.extend(jpeg);
        }
        let (frames, came) = (Mutex::new(Frames::default()), Condvar::new());
        let why = frames_of(&bytes[..], &frames, &came);
        assert!(why.contains("ended"), "{why}");
        let f = frames.lock().unwrap();
        assert_eq!((f.n, f.jpeg.as_slice()), (2, &b"two!"[..]));
        drop(f);
        frames.lock().unwrap().asked = Instant::now().checked_sub(UNWATCHED * 2);
        assert_eq!(frames_of(&bytes[..], &frames, &came), "nobody watched it");
    }

    #[test]
    fn an_act_reads_as_an_agent_writes_it() {
        let act: Act = serde_json::from_str(r#"{"act":"tap","x":10,"y":20}"#).unwrap();
        assert_eq!(act, Act::Tap { x: 10, y: 20 });
        let act: Act = serde_json::from_str(r#"{"act":"key","key":"home"}"#).unwrap();
        assert_eq!(act, Act::Key { key: Key::Home });
    }

    #[test]
    fn simctl_s_list_gives_its_ios_simulators_the_booted_first() {
        let json = r#"{"devices": {
            "com.apple.CoreSimulator.SimRuntime.iOS-27-2": [
                {"udid": "A", "name": "iPhone Duo", "state": "Shutdown", "isAvailable": true},
                {"udid": "B", "name": "iPad", "state": "Booted", "isAvailable": true}],
            "com.apple.CoreSimulator.SimRuntime.watchOS-12-0": [
                {"udid": "C", "name": "Watch", "state": "Shutdown"}]}}"#;
        let found = simulators(json);
        assert_eq!(
            found
                .iter()
                .map(|d| (d.id.as_str(), d.running))
                .collect::<Vec<_>>(),
            [("B", true), ("A", false)]
        );
    }

    #[test]
    fn a_foldable_is_read_on_its_primary_screen() {
        let said = "Cannot choose between 2 displays. Specify one with --display:\n    --display=3  LCD-1 (primary-1) 2007x2853\n    --display=1  LCD (primary) 1398x2034\n";
        assert_eq!(primary(said).as_deref(), Some("--display=1"));
    }

    #[test]
    fn an_emulator_running_takes_its_serial_and_a_phone_is_listed_by_its_own() {
        let running = "List of devices attached\nemulator-5554\tdevice\nR58M\tdevice\nX\toffline\n";
        let found = androids("Pixel_9\nTablet\n", running, |s| {
            (s == "emulator-5554").then(|| "Pixel_9".to_owned())
        });
        assert_eq!(found.len(), 3);
        assert_eq!(
            (
                found[0].name.as_str(),
                found[0].serial.as_deref(),
                found[0].running
            ),
            ("Pixel 9", Some("emulator-5554"), true)
        );
        assert!(!found[1].running);
        assert_eq!(found[2].id, "R58M");
    }
}
