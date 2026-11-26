---
title: Simulators and emulators
description: The Devices tab drives iOS simulators and Android emulators, for you and your agents.
---

# Simulators and emulators

The right column's **Devices** tab (⌘T, then V) lists the iOS simulators
and Android emulators of the machine the session runs on. You can:

- **Start** one, or **Power off** a running one;
- watch its screen, frame after frame;
- click its screen to tap it, and **Type into it**;
- press **Home** (and **Back** and **Recents** on Android);
- switch it **Dark** or **Light**.

If nothing is found, the tab says what is missing; **Look again** looks
again.

## What it needs

| Platform | Needs |
|---|---|
| iOS | Xcode (`xcrun simctl`). Touch, typing, Home and the live screen use [AXe](https://github.com/cameroncooke/AXe) (`brew install cameroncooke/axe/axe`); without it, starting a simulator opens Simulator.app. |
| Android | The Android SDK's `emulator` and `adb`, found on the `PATH`, in `ANDROID_HOME` or `ANDROID_SDK_ROOT`, or in `~/Library/Android/sdk`. |

## For agents

Agents use the same devices through [Ghost's tools](tools-for-agents.md):
`list_devices`, `device_screen` (a picture of the screen) and
`use_device` (start, stop, tap, type, keys, dark or light), which asks
you first.
