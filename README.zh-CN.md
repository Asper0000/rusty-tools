<p align="center">
  <img src="https://raw.githubusercontent.com/Asper0000/rusty-tools/bced465d965c713ff4f68138787eae40798be40a/src/public/icon-animated.svg" width="160" height="160" alt="RustyTools 标志">
</p>

<h1 align="center">RustyTools</h1>

<p align="center">
  <strong>解锁几乎全部 Steam 游戏，以及它们的全部 DLC。</strong><br>
  一个免费、开源的 Steam 解锁工具。它是 <a href="https://github.com/madoiscool/BetterSteamTools">BetterSteamTools</a> 的分支，使用 Rust 从零重写。
</p>

<p align="center">
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/Rust-1.75%2B-orange?logo=rust&logoColor=white" alt="Rust"></a>
  <a href="#"><img src="https://img.shields.io/badge/Platform-Windows%2010%2F11%20x64-0078D6?logo=windows&logoColor=white" alt="Windows"></a>
</p>

<p align="center">
  <a href="README.md">English</a> · <strong>简体中文</strong>
</p>

---

## 目录

- [什么是 RustyTools？](#什么是-rustytools)
- [功能](#功能)
- [工作原理](#工作原理)
- [不支持的情况：Denuvo](#不支持的情况denuvo)
- [安装](#安装)
- [配置](#配置)
- [命令行工具](#命令行工具)
- [技术细节](#技术细节)
- [教育用途声明](#教育用途声明)
- [许可证](#许可证)
- [致谢](#致谢)

---

## 什么是 RustyTools？

**RustyTools** 是一个轻量的 Steam 客户端扩展，能让 Steam 认为你的账号拥有你指定的游戏和 DLC。只要在一个小的 Lua 脚本里加上 appid，重启 Steam，游戏就会出现在你的库中，可以直接安装和游玩。

在底层，RustyTools 在 Steam 启动时载入客户端进程，悄悄地按照你的意愿回答 Steam 自己的"我拥有这个游戏吗？"的问题。Steam 完全察觉不到区别，游戏可以正常安装和启动。

它是 C++ 项目 **[BetterSteamTools](https://github.com/madoiscool/BetterSteamTools)** 的分支，使用 Rust 完全重写，以换取更好的内存安全、更干净的并发模型和现代化的工具链。原项目的所有功能 RustyTools 都有，另外还增加了一些额外的功能。

- **免费且开源。** 无账号、无 token、无付费等级。
- **无需安装。** 把三个 DLL 放到 `steam.exe` 旁边，重启 Steam 即可。
- **可逆。** 删除 DLL，Steam 立刻恢复原状。
- **配置文件是纯文本。** TOML 用于设置，Lua 用于游戏列表。

---

## 功能

- 🔓 **解锁任意 Steam 游戏**：全部商店页面、全部区域、全部平台
- 🎁 **自动解锁全部 DLC**：包括豪华版、季票、装饰包
- **成就与统计**：与成就系统完全兼容
- **自动 depot 解密**：无需手动输入密钥
- **Manifest 预缓存**：游戏下载立刻开始
- **`bst://` 链接处理**：浏览器一键兑换
- **配置热重载**：无需重启 Steam 即可增删游戏
- **Lua 脚本支持**：灵活的单游戏调整与覆写
- 🛡️ **可安全移除**：不会对 Steam 安装产生任何永久修改

---

## 工作原理

RustyTools 在 Steam 启动的那一刻载入 `steam.exe`。随后，它在 Steam 自身的代码里钩住少量函数，并按照你希望的方式回答它们：

| Steam 询问…… | RustyTools 回答…… |
|---|---|
| "该用户是否拥有 app 730？" | 是 |
| "他能安装哪些 depot？" | 全部 |
| "这个 depot 的解密密钥是什么？" | 给你 |
| "这个游戏应该出现在库中吗？" | 当然 |

Steam 客户端非常直白：如果你告诉它你拥有一个游戏，它就会表现得完全就像你拥有一样。安装按钮会出现，下载会开始，游戏会启动，反作弊握手会通过，因为在 Steam 看来，这一切都不是谎言。

**没有从 Valve 服务器下载任何非公开内容。** RustyTools 从不接触你的支付信息、账号密码或好友列表。它只改变 Steam *认为的* 游戏所有权，而且只针对你配置里列出的游戏。

---

## 不支持的情况：Denuvo

一些较新的 3A 大作使用 **Denuvo** 保护，它要求每次启动时都有一张 **新鲜、经过密码学签名的授权票据**（"e-ticket"）。Steam 本身不签发这种票据。它们来自一个背后有真实拥有账号的服务器，而且会过期。

要让 Denuvo 加密的游戏能启动，工具必须运行 **一个拥有该游戏、保持 24/7 在线、并在你每次按 Play 时签发新 e-ticket 的服务器**。这意味着：

- 每个受保护的游戏都需要一个 **真实付费账号**
- **24/7 运行的虚拟机** 让这些账号保持登录
- 带宽和运维成本，随用户数量线性增长

我们负担不起这样的运维，也不愿意这么做，因为这正是会让 RustyTools 无法持续、法律上也会变得一团糟的事情。**所以 Denuvo 加密的游戏不受支持。** 如果某个游戏受 Denuvo 保护，它可以正常安装和更新，但无法通过 DRM 检查启动。

**没有** Denuvo 保护的游戏，也就是 Steam 目录中绝大部分（包括绝大多数独立游戏、较老的 3A 游戏和中型预算的作品），完全正常。

> **提示：** 在尝试之前，先看看游戏商店页面或 [PCGamingWiki](https://www.pcgamingwiki.com/)。如果它写着 "Denuvo Anti-Tamper"，就无法启动。

---

## 安装

### 系统要求

- **Windows 10 或 11**（64 位）
- **Steam** 已安装并至少登录过一次
- 没有其他修改 Steam 的工具在运行

### 步骤

1. 从 [Releases 页面](../../releases) **下载**最新版本。
2. **完全退出 Steam**（托盘图标 -> *退出*，不是关闭窗口）。
3. 将**三个文件**复制到你的 Steam 安装目录（通常是 `C:\Program Files (x86)\Steam\`）：
   - `RustyTools.dll`
   - `dwmapi.dll`
   - `xinput1_4.dll`
4. **启动 Steam。**
5. 把游戏加入 `rustytools.toml`，见下方[配置](#配置)。
6. **重启一次 Steam**，游戏就会出现在库中。

### 卸载

从 Steam 目录删除三个 DLL 并重启 Steam。一切恢复正常：没有注册表残留，没有遗留文件，没有痕迹。

---

## 配置

所有游戏选择都放在 **`rustytools.toml`**（与 `steam.exe` 同目录）和 **`*.lua` 脚本**中（位于 `Steam\config\stplug-in\`）。

### `rustytools.toml`

配置 RustyTools 本身的 TOML 文件：日志、manifest 提供方、可选的 Cloud 重定向等等。它是可选的；缺少时会使用合理的默认值。带内联注释的完整示例位于 [`config/rustytools.example.toml`](config/rustytools.example.toml)。

### Lua 脚本：你的游戏列表

这里告诉 RustyTools 要解锁哪些游戏。创建 `Steam\config\stplug-in\my-games.lua`：

```lua
-- 解锁 Portal 2（appid 620）
addappid(620)

-- 解锁全部 DLC
addappid(620, 0, "DLC_DEPOT_ID_HERE")

-- 解锁 Half-Life: Alyx
addappid(546560)

-- 强制指定某个 depot 的解密密钥
addappid(546561, 0, "0123456789abcdef0123456789abcdef")
```

更改**在 Steam 运行时**就会被读取。无需重启。添加一个游戏，它会在几秒内出现在库中。

### 如何查找 appid？

- **游戏商店页面 URL：** `store.steampowered.com/app/**620**/Portal_2/`
- **SteamDB：** 搜索游戏名，appid 在页面最上面
- **社区列表：** RustyTools 讨论区里有很多置顶的

### 移除游戏

从 Lua 脚本中删除 `addappid(...)` 那一行并保存。Steam 会在几秒内从库中隐藏该游戏。跨重启、缓存清理、全新登录都会保持隐藏。

---

## 命令行工具

RustyTools 附带 `ost-cli.exe`，一个小的开发者工具，用于在 Steam 之外测试配置和运行便捷功能。

```text
ost-cli version                      打印库版本
ost-cli init [--config PATH]         加载并打印 TOML 配置
ost-cli parse-lua PATH               解析 Lua 脚本并打印 depot 集合
ost-cli parse-toml PATH              打印实际生效的 TOML 设置
ost-cli sha256 PATH                  文件的 SHA-256（用于 pattern 缓存键）
ost-cli fnv1a STRING                 FNV-1a 哈希（内部用于签名）
ost-cli handle-uri URL               模拟一次 bst:// 链接处理，无需浏览器
ost-cli register-uri PATH            一次性注册 bst:// URL scheme
```

---

## 技术细节

*如果你只想玩游戏，可以跳过这一节。*

### 架构

RustyTools 是一个原生 Windows DLL，通过 **加载器劫持** 载入 `steam.exe`。两个代理 DLL（`dwmapi.dll` 和 `xinput1_4.dll`）放在 `steam.exe` 旁边，并被优先加载到系统副本（Windows 在 `System32` 之前搜索应用程序目录）。每个代理 DLL 将其整个导出表转发到真正的系统 DLL，并且仅在宿主进程是 `steam.exe` 时载入 `RustyTools.dll`。

加载后，`RustyTools.dll` 会派生一个初始化线程，它会：

1. 通过 **字节模式扫描** 解析 Steam 的内部函数（签名按 Steam 构建版本拉取；本地有缓存的回退签名）
2. 解析你的 TOML 配置和 Lua 脚本
3. 在约十几个 Steam 函数上安装 **Detours 钩子**
4. 启动文件监视器，实现配置热重载
5. 进入空闲模式

钩子层覆盖：

| 层 | 作用 |
|---|---|
| **IPC** | 拦截客户端与 `steamclient64.dll` 之间的 Steam IPC 消息 |
| **NetPacket** | 重写 WebSocket protobuf 数据包（licenses、stats、presence、manifests） |
| **Package** | 注入一个虚假的 license 包，授予你对配置中 appid 的所有权 |
| **Manifest** | 预先缓存和预先拉取 depot manifest，让游戏立刻开始下载 |
| **Decryption** | 为 depot 提供 AES 密钥；密钥来自你的 Lua 配置或 Steam 自己的存储 |
| **SteamUI** | 在完整库重建中保持被移除的游戏隐藏 |
| **Misc** | 处理 `-onlinefix` 启动参数和必要的 appid 伪装 |

### 构建

要求：

- Windows 10/11 x64
- Visual Studio 2022（MSVC x64 工具链）
- Rust 1.75+
- [Microsoft Detours](https://github.com/microsoft/Detours) 静态库，vendored 到 `third_party/Detours/`

在 x64 Native Tools 命令提示符中执行：

```bat
git clone https://github.com/microsoft/Detours.git third_party\Detours
cd third_party\Detours\src
set DETOURS_TARGET_PROCESSOR=X64
nmake
cd ..\..\..
cargo build --workspace --release
```

输出位于 `target\release\`：

- `RustyTools.dll`：被注入的钩子 DLL
- `dwmapi.dll`、`xinput1_4.dll`：加载器劫持代理
- `ost-cli.exe`：开发者 CLI

手动验证清单见 [`docs/TESTING.md`](docs/TESTING.md)；已知分歧（很短）见 [`docs/STUBS.md`](docs/STUBS.md)。

### Pattern 如何拉取

Steam 频繁更新客户端。钩子目标通过 **签名**（字节模式）解析，而不是固定偏移，并且每个 Steam 构建版本都会重新生成 pattern 文件。如果 pattern 文件缺失或过期，RustyTools 会回退到本地缓存的签名集；如果某个函数根本找不到，就只有那个具体的钩子被禁用。Steam 仍然正常启动。

### 数据布局

| 文件 / 文件夹 | 用途 |
|---|---|
| `rustytools.toml` | 主配置（与 `steam.exe` 同目录） |
| `config\stplug-in\*.lua` | 单游戏解锁列表 |
| `rustytools\logs\*.log` | 每个模块一个日志（`main`、`ipc`、`netpacket`、……） |
| `rustytools\pattern\` | 按 DLL SHA-256 缓存的签名文件 |
| `rustytools\ipc\` | 按 DLL SHA-256 缓存的 IPC 方法描述 |

---

## 教育用途声明

**RustyTools 仅供教育研究目的提供。**

本项目存在的意义是研究 Windows 进程注入、函数钩取、Steam IPC 协议、depot manifest 以及 DRM 系统在系统层面是如何运作的。它不是产品，也不作为产品分发。

**使用 RustyTools 与 Steam 交互可能违反 [Steam 用户协议](https://store.steampowered.com/subscriber_agreement/)，可能导致你的账号被限制或终止。** 使用 RustyTools 获取你未曾付费的内容，可能在你所在的司法管辖区违反版权法。

RustyTools 的作者与贡献者：

- **不赞成** 使用本软件规避你本应付费的付费内容
- **不提供支持** 用于任何违反游戏 EULA、Steam 服务条款或任何适用法律的用途
- **不承担任何责任**，包括账号封禁、法律诉讼、存档损坏、数据丢失，或任何其他因使用本软件而产生的后果

**使用 RustyTools 的任何风险完全由你自己承担，完全在你一个人身上。**

如果你不是为了研究代码本身，这个项目不适合你。

---

## 许可证

RustyTools 采用 **GNU 通用公共许可证 v3.0**（GPLv3）授权。

本项目是 **[BetterSteamTools](https://github.com/madoiscool/BetterSteamTools)** 的分支，使用 Rust 重写并做了大幅修改。它包含来自原项目的代码和设计成果。原作者的著作权在许可条款下被承认和保留；这里的合并作品以 GPLv3 分发，以符合这些条款。

完整许可证文本位于 [`LICENSE`](LICENSE)。使用、修改或再分发本项目，即表示你同意 GPLv3 的条款：

- 你可以 **使用** RustyTools 用于任何目的
- 你可以 **研究并修改** 源码
- 你可以 **再分发** 修改版，**但必须同样以 GPLv3 授权**，**且必须提供源码**
- 你 **不得** 添加任何限制他人 GPLv3 自由的条件

---

## 致谢

- **[BetterSteamTools](https://github.com/madoiscool/BetterSteamTools)**：本项目的 C++ 原项目
- **[OpenSteamTool](https://github.com/madoiscool/OpenSteamTool)**：整个生态的精神先驱
- **[Microsoft Detours](https://github.com/microsoft/Detours)**：一切所依赖的函数钩取库
- **[mlua](https://github.com/mlua-rs/mlua)** 与 **[prost](https://github.com/tokio-rs/prost)**：Rust 的 Lua 与 protobuf 库
- **Steam 模组社区**：多年的逆向工程让这一切成为可能

---

<p align="center">
  <em>如果 RustyTools 让你的日子好过一点，考虑给仓库点个 star ⭐</em>
</p>