<div align="center">

# ⛏️ RustCraft

### A Minecraft 1.8.9 client clone written in Rust  
### 使用 Rust 编写的 Minecraft 1.8.9 客户端复刻

[![Rust](https://img.shields.io/badge/Rust-2021-orange?logo=rust)](https://www.rust-lang.org/)
[![Vulkan](https://img.shields.io/badge/Renderer-Vulkan-blue)](#)
[![Protocol](https://img.shields.io/badge/Minecraft-1.8.9%20(protocol%2047)-green)](#)
[![Build](https://img.shields.io/badge/Build-Cargo-informational)](#)

</div>

---

## 🌐 Navigation | 导航

- [🇬🇧 English](#english)
  - [Overview](#overview)
  - [Features](#features)
  - [Build & Run](#build--run)
  - [Project Structure](#project-structure)
  - [Key Dependencies](#key-dependencies)
  - [Notes](#notes)
- [🇨🇳 中文](#中文)
  - [项目简介](#项目简介)
  - [功能特性](#功能特性)
  - [构建与运行](#构建与运行)
  - [项目结构](#项目结构-1)
  - [关键依赖](#关键依赖)
  - [说明](#说明)
- [Contributing / 贡献](#contributing--贡献)
- [Disclaimer / 免责声明](#disclaimer--免责声明)

---

## English

### Overview

**RustCraft** is a Minecraft **1.8.9** client clone implemented in **Rust**.  
It can connect to real Minecraft servers using **protocol version 47**, render the world with a **Vulkan** backend, and supports **Microsoft/Xbox Live OAuth authentication**.

> The `minecraft/` directory contains decompiled Minecraft 1.8.9 Java source for reference.

---

### Features

- ✅ Minecraft 1.8.9 protocol networking (`net/`)
- ✅ Vulkan-based rendering pipeline (`render/`)
- ✅ Microsoft/Xbox authentication (`auth/`)
- ✅ World/chunk/block/entity systems
- ✅ Asset pipeline (textures/models/resource packs/skins/sounds)
- ✅ In-game UI, HUD, and menu systems
- ✅ Audio playback via `rodio`

---

### Build & Run

```bash
# Build (debug, opt-level=1)
cargo build

# Release build (LTO thin, panic=abort, stripped)
cargo build --release

# Run (launches the game)
cargo run

# Run tests
cargo test

# Lint
cargo clippy

# Format
cargo fmt -- --check
```

---

### Render Backends

Vulkan remains the default renderer. On Windows, Direct3D 12 can be selected
with the `RUSTCRAFT_RENDER_BACKEND` environment variable:

```powershell
$env:RUSTCRAFT_RENDER_BACKEND="dx12"
cargo run
```

The Direct3D 12 backend requires Windows 10 or newer and a D3D12-capable
adapter. If initialization fails, RustCraft logs the error and falls back to
Vulkan.

---

### Project Structure

```text
crates/
├── game/                # Game crate: client, world, entities, GUI, assets and gameplay policy
│   └── src/shaders/     # GLSL shaders compiled by the game build script
├── renderer/            # Renderer crate: backend-neutral Core + RHI
│   └── src/
│       ├── core/        # Render graph, queue, materials and resource management
│       └── rhi/         # Device, resources and command interfaces
└── renderer/backend/
    ├── vulkan/          # Vulkan implementation of the renderer RHI
    └── dx12/            # Direct3D 12 implementation (Windows only)
```

---

### Key Dependencies

| Category | Crate | Purpose |
|---|---|---|
| Windowing | `winit` | Window creation, events, input |
| Graphics | `ash`, `ash-window` | Vulkan bindings |
| Math | `nalgebra` | Linear algebra |
| Networking | `reqwest`, `flate2` | HTTP, compression |
| Crypto | `aes`, `rsa`, `sha1`, `sha2` | Protocol encryption |
| Audio | `rodio` | Sound playback |
| Image | `image` | Texture loading |
| Font | `fontdue` | Font rasterization |
| Serialization | `serde`, `serde_json` | JSON config |
| Concurrency | `rayon` | Parallel mesh building |
| Build script | `shaderc` | GLSL → SPIR-V compilation |

---

### Notes

- Rust edition: **2021**
- Error handling style: `std::io::Result`, `Option`, `.ok()` chaining (no `anyhow`/`thiserror`)
- Tests are inline via `#[cfg(test)]`
- `crates/game/build.rs` automatically compiles shaders in `crates/game/src/shaders`
- AMD FidelityFX SDK `v1.1.4` is pinned as `third_party/FidelityFX-SDK` for the Vulkan FSR 3.1.4 integration. Initialize it with `git submodule update --init --recursive`.
- `shaderpacks/` recognizes RustCraft Vulkan packs and detects OptiFine/Iris layouts. OptiFine/Iris program execution requires the Vulkan G-buffer compatibility render graph and is not enabled until that graph is complete.

### Linux Dependencies

On Linux the following system packages are required:

```bash
# Debian / Ubuntu
sudo apt install libshaderc-dev libasound2-dev

# Fedora
sudo dnf install shaderc alsa-lib-devel

# Arch Linux
sudo pacman -S shaderc alsa-lib
```

Clipboard support uses `wl-paste`/`wl-copy` (Wayland) or `xclip` (X11).

---

## 中文

### 项目简介

**RustCraft** 是一个使用 **Rust** 编写的 **Minecraft 1.8.9** 客户端复刻项目。  
它支持通过 **47 协议版本**连接真实 Minecraft 服务器，使用 **Vulkan** 进行世界渲染，并实现了 **Microsoft/Xbox Live OAuth 登录认证**。

> `minecraft/` 目录包含反编译的 Minecraft 1.8.9 Java 源码，仅用于对照参考。

---

### 功能特性

- ✅ Minecraft 1.8.9 协议网络实现（`net/`）
- ✅ 基于 Vulkan 的渲染管线（`render/`）
- ✅ Microsoft/Xbox 认证流程（`auth/`）
- ✅ 世界/区块/方块/实体系统
- ✅ 资源加载系统（纹理、模型、资源包、皮肤、音频）
- ✅ UI、HUD、菜单与字体渲染
- ✅ 基于 `rodio` 的音频播放

---

### 构建与运行

```bash
# 调试构建（opt-level=1）
cargo build

# 发布构建（LTO thin, panic=abort, strip）
cargo build --release

# 启动游戏
cargo run

# 运行测试
cargo test

# 代码检查
cargo clippy

# 格式检查
cargo fmt -- --check
```

### 渲染后端

默认使用 Vulkan。在 Windows 上，可以通过环境变量选择 Direct3D 12：

```powershell
$env:RUSTCRAFT_RENDER_BACKEND="dx12"
cargo run
```

Direct3D 12 后端需要 Windows 10 或更高版本，以及支持 D3D12 的显卡。
如果初始化失败，RustCraft 会记录错误并自动回退到 Vulkan。

### Linux 系统依赖

```bash
# Debian / Ubuntu
sudo apt install libshaderc-dev libasound2-dev

# Fedora
sudo dnf install shaderc alsa-lib-devel

# Arch Linux
sudo pacman -S shaderc alsa-lib
```

剪贴板功能需要 `wl-paste`/`wl-copy`（Wayland）或 `xclip`（X11）。

---

### 项目结构

```text
crates/
├── game/                # game crate：客户端、世界、实体、GUI、资源和游戏渲染策略
│   └── src/shaders/     # 由 game build script 编译的 GLSL 着色器
├── renderer/            # renderer crate：与后端无关的 Core + RHI
│   └── src/
│       ├── core/        # Render Graph、队列、材质和资源管理
│       └── rhi/         # Device、资源和 Command 接口
└── renderer/backend/
    ├── vulkan/          # renderer RHI 的 Vulkan 实现
    └── dx12/            # Direct3D 12 实现（仅 Windows）
```

---

### 关键依赖

| 分类 | Crate | 用途 |
|---|---|---|
| 窗口与输入 | `winit` | 窗口创建、事件、输入 |
| 图形渲染 | `ash`, `ash-window` | Vulkan 绑定 |
| 数学 | `nalgebra` | 线性代数 |
| 网络 | `reqwest`, `flate2` | HTTP 与压缩 |
| 加密 | `aes`, `rsa`, `sha1`, `sha2` | 协议加密 |
| 音频 | `rodio` | 声音播放 |
| 图像 | `image` | 纹理加载 |
| 字体 | `fontdue` | 字体光栅化 |
| 序列化 | `serde`, `serde_json` | JSON 配置 |
| 并行 | `rayon` | 并行网格构建 |
| 构建脚本 | `shaderc` | GLSL → SPIR-V 编译 |

---

### 说明

- Rust 版本：**2021**
- 错误处理风格：`std::io::Result`、`Option`、`.ok()` 链式调用（未使用 `anyhow/thiserror`）
- 测试以源码内联 `#[cfg(test)]` 形式组织
- `crates/game/build.rs` 会自动编译 `crates/game/src/shaders` 下的着色器
- AMD FidelityFX SDK `v1.1.4` 固定在 `third_party/FidelityFX-SDK`，用于 Vulkan FSR 3.1.4 集成。请通过 `git submodule update --init --recursive` 初始化。
- `shaderpacks/` 可识别 RustCraft Vulkan 包并检测 OptiFine/Iris 布局。OptiFine/Iris 程序执行依赖 Vulkan G-buffer 兼容渲染图，在该渲染图完成前不会错误地启用。

---

## Contributing / 贡献

PRs and issues are welcome.  
欢迎提交 Issue 与 PR，一起完善 RustCraft。

---

## Disclaimer / 免责声明

This project is an independent reimplementation for educational and technical purposes.  
Minecraft is a trademark of Mojang Studios / Microsoft.

本项目为独立技术复刻与学习用途。  
Minecraft 商标归 Mojang Studios / Microsoft 所有.
