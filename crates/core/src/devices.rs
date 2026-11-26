//! iOS simulators and Android emulators on the machine the server runs on,
//! as t3code's Devices panel has them: listed, started and stopped, their
//! screen read as a picture, and an Android one touched and typed into.
//! Driven by Xcode's `xcrun simctl` and the Android SDK's `adb` and
//! `emulator`, where they are installed; `simctl` takes no touch, so an
//! iOS simulator is touched in Simulator itself.

use std::{
    path::PathBuf,
    process::{Command, Stdio},
};

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

/// What can be done to a device.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Act {
    Start,
    Stop,
    /// Android's Home, Back and Recents keys.
    Key(AndroidKey),
    /// A touch at this point of its screen, in its pixels.
    Tap(u32, u32),
    Type(String),
    /// Dark appearance, or light.
    Dark(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AndroidKey {
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
                // Its window, where it is touched: Simulator, of the Xcode
                // simctl is.
                let xcode = std::env::var_os("DEVELOPER_DIR")
                    .map(PathBuf::from)
                    .or_else(|| {
                        text(&PathBuf::from("/usr/bin/xcode-select"), &["-p"])
                            .ok()
                            .map(|p| PathBuf::from(p.trim()))
                    });
                if let Some(app) = xcode.map(|d| d.join("Applications/Simulator.app")) {
                    let _ = run(&PathBuf::from("/usr/bin/open"), &[&app.to_string_lossy()]);
                }
                Vec::new()
            }),
            Act::Stop => run(&x, &["simctl", "shutdown", id]),
            Act::Dark(dark) => run(
                &x,
                &[
                    "simctl",
                    "ui",
                    id,
                    "appearance",
                    if *dark { "dark" } else { "light" },
                ],
            ),
            Act::Key(_) | Act::Tap(..) | Act::Type(_) => {
                Err("An iOS simulator is touched in Simulator itself.".into())
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
        Act::Key(key) => shell(&[
            "input",
            "keyevent",
            match key {
                AndroidKey::Home => "KEYCODE_HOME",
                AndroidKey::Back => "KEYCODE_BACK",
                AndroidKey::Recents => "KEYCODE_APP_SWITCH",
            },
        ]),
        Act::Tap(x, y) => shell(&["input", "tap", &x.to_string(), &y.to_string()]),
        // `input text` takes no spaces as they are.
        Act::Type(words) => shell(&["input", "text", &words.replace(' ', "%s")]),
        Act::Dark(dark) => shell(&["cmd", "uimode", "night", if *dark { "yes" } else { "no" }]),
    }
}

/// What `device` shows now, as a PNG.
pub fn screen(device: &Device) -> Result<Vec<u8>, String> {
    if device.android {
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
