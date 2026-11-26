---
title: Simulators and emulators
description: The Devices tab drives iOS simulators and Android emulators, for you and your agents.
---

# Simulators and emulators

The right column's **Devices** tab (⌘T, then V) lists the iOS simulators
and Android emulators of the machine the session runs on. You can:

- **Start** one, or **Power off** a running one;
- watch its screen, frame after frame;
- click its screen to tap it, hold the click half a second for a long
  press, drag across it to swipe, and scroll it with the wheel or
  trackpad;
- **Type into it**;
- press **Home** (and **Back** and **Recents** on Android);
- **Rotate left** or **Rotate right**;
- switch it **Dark** or **Light**.

If nothing is found, the tab says what is missing; **Look again** looks
again. Where AXe is missing for iOS, the tab offers **Install AXe**, which
runs `brew install cameroncooke/axe/axe` (AXe's own way to install it)
only once you press it; Ghost says when it is done, or why not (for
instance, when Homebrew is not installed).

## What it needs

| Platform | Needs |
|---|---|
| iOS | Xcode (`xcrun simctl`). Touch, gestures, rotation, typing, Home and the live screen use [AXe](https://github.com/cameroncooke/AXe) (`brew install cameroncooke/axe/axe`); without it, starting a simulator opens Simulator.app. |
| Android | The Android SDK's `emulator` and `adb`, found on the `PATH`, in `ANDROID_HOME` or `ANDROID_SDK_ROOT`, or in `~/Library/Android/sdk`. |

## For agents

Agents use the same devices through [Ghost's tools](tools-for-agents.md):
`list_devices`, `device_screen` (a picture of the screen) and
`use_device` (start, stop, tap, long `press`, `swipe` from one point to
another, `scroll` toward a side, `rotate`, type, keys, dark or light),
which asks you first. Points are in the pixels of the picture
`device_screen` gives.
