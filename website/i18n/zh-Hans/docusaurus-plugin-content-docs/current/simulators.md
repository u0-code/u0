---
title: 模拟器
description: 设备标签页操控 iOS 模拟器和 Android 模拟器，供你和你的智能体使用。
---

# 模拟器

右侧栏的 **设备** 标签页（⌘T，然后按 V）列出会话所在机器上的 iOS 模拟器和 Android 模拟器。你可以：

- **启动** 一个，或将正在运行的 **关机**；
- 逐帧观看它的屏幕；
- 点击它的屏幕来轻点，以及 **向其中输入**；
- 按 **主屏幕**（Android 上还有 **返回** 和 **最近任务**）；
- 切换为 **深色** 或 **浅色**。

如果什么都没找到，标签页会说明缺少什么；**重新查找** 会再找一次。

## 需要什么 {#what-it-needs}

| 平台 | 需要 |
|---|---|
| iOS | Xcode（`xcrun simctl`）。轻点、输入、主屏幕和实时屏幕使用 [AXe](https://github.com/cameroncooke/AXe)（`brew install cameroncooke/axe/axe`）；没有它时，启动模拟器会打开 Simulator.app。 |
| Android | Android SDK 的 `emulator` 和 `adb`，可在 `PATH`、`ANDROID_HOME` 或 `ANDROID_SDK_ROOT` 中，或在 `~/Library/Android/sdk` 中找到。 |

## 给智能体用 {#for-agents}

智能体通过 [Ghost 的工具](tools-for-agents.md)使用同样的设备：`list_devices`、`device_screen`（屏幕的截图）和 `use_device`（启动、停止、轻点、输入、按键、深色或浅色），后者会先询问你。
