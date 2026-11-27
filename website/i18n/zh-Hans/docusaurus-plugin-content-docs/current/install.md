---
title: 安装
description: 在 Mac 上用 Homebrew 安装 u0，或在 Linux 上安装 u0-server。
---

# 安装

u0.app 运行在搭载 Apple 芯片、macOS 13（Ventura）或更高版本的 Mac 上。
`u0-server` 是不带窗口的同一个服务器，也可以在 Linux（x86_64）上运行。
构建发布在仓库的 [GitHub releases](https://github.com/u0-code/u0/releases) 上。

## 在 Mac 上，使用 Homebrew {#on-a-mac-with-homebrew}

cask 放在 u0 自己的仓库中，所以先把它添加为 tap：

```bash
brew tap u0-code/u0 https://github.com/u0-code/u0
```

然后安装两个渠道之一。两者互相冲突，所以只装其中一个。

| Cask | 安装的内容 |
|---|---|
| `u0` | 最新发布版。`brew upgrade` 会让它保持最新。 |
| `u0@nightly` | `main` 的最新构建。每次推送都会替换它。 |

```bash
# The latest release
brew install --cask u0-code/u0/u0

# Or the nightly build
brew install --cask u0-code/u0/u0@nightly
brew reinstall --cask u0-code/u0/u0@nightly   # fetch the newest build
```

u0 使用临时签名（ad hoc），没有经过公证，因此 Gatekeeper 会拒绝打开它。
cask 在安装 u0.app 时会移除隔离属性（`xattr -dr com.apple.quarantine`）。
安装这个 cask，就是你选择打开它。

`brew uninstall --zap` 还会删除 u0 的主目录 `~/.u0`。

## 更新 {#updates}

u0 在启动时以及每四小时检查一次是否有更新的构建，除非你关闭它
（[设置](reference/settings.md)中的 `check_updates`）。
**设置 › 关于** 显示你正在运行的版本，并让你选择 u0 更新到哪个渠道：
**稳定版**（`u0` cask 安装的发布版）或 **每夜版**（`u0@nightly` 安装的内容），
并提供 **立即检查**。找到更新的构建时，u0 会告诉你；在你按下 **安装** 之前不会安装任何东西。
之后 u0 会下载它、检查其代码签名、用它替换 u0.app（若失败则放回旧版本），并提供 **重新启动**。

## 在 Linux 上：u0-server {#on-linux-u0-server}

每个发布版以及每夜构建，都在 Mac 应用旁附带
`u0-server-linux-x86_64.tar.gz`。它只包含一个程序 `u0-server`：
不带窗口的 u0 服务器。它运行智能体、保存线程，并把它们提供给与之配对的 Mac 上的 u0 窗口。

```bash
curl -LO https://github.com/u0-code/u0/releases/download/nightly/u0-server-linux-x86_64.tar.gz
tar -xzf u0-server-linux-x86_64.tar.gz
./u0-server
```

`u0-server` 启动时会打印它监听的地址和一个配对链接（附带二维码），该链接只能用一次，
有效期十分钟。Mac 如何与它配对，见[远程控制](remote.md)；它的命令见
[CLI](reference/cli.md)。请用你自己的进程管理器（systemd 或其他）运行它；
`u0 service` 只适用于 macOS 的 launchd。

## 从源代码构建 {#from-source}

安装好 [Rust](https://rustup.rs) 后，在
[u0-code/u0](https://github.com/u0-code/u0) 的检出目录中：

```bash
cargo run --release        # run the app
./bundle.sh                # build target/release/u0.app
```

手动构建报告的渠道是 `dev`，而 `dev` 构建无论级别如何都不会发送
[遥测](first-run.md#telemetry)。

macOS 会在应用读取 `~/Desktop` 或 `~/Documents` 下的文件夹之前先询问，
并按应用的签名记住你的回答。临时签名的构建在每次重新构建后都会被再问一次。
要避免这种情况，在“钥匙串访问”中创建一个名为 `u0 Dev` 的自签名代码签名证书
（证书助理 › 创建证书…，自签名根证书，代码签名）。`bundle.sh` 会找到它并用它签名；
`U0_SIGN_IDENTITY` 可以指定另一个签名身份。

## u0 把文件放在哪里 {#where-u0-keeps-its-files}

u0 保存的一切都在它的主目录 `~/.u0` 中，或环境变量 `U0_HOME` 指定的文件夹中。
同一台机器上的两个 u0 各自需要自己的 `U0_HOME`。每个文件保存了什么，见
[隐私与安全](privacy-security.md)。
