# rustdesk-core

RustDesk Rust 核心的裁剪 fork，供 [Termux Ultra](https://github.com/TiG-Kira/Termux-Ultra) 内嵌使用。

- **上游**：`rustdesk/rustdesk` tag `1.5.1`
- **许可**：AGPL-3.0（见 `LICENCE`）
- **范围**：**只做控制端**（连接远端、渲染画面、发送输入、账号登录）；不含被控端、不含 RustDesk 主页。
- **UI**：不在本仓库。宿主用 Kotlin + Compose 自研会话页，通过 JNI 桥接调用本仓库产出的 `librustdesk.so`。

## 与上游的差异

| 项 | 处理 |
|---|---|
| `flutter/`（Dart UI） | 整目录删除，不留占位 |
| `src/ui/`（Sciter 桌面 UI） | 删除（`lib.rs` 中该模块在 android/flutter feature 下已被 cfg 排除） |
| 桌面打包材料（appimage/flatpak/fastlane/docs/...） | 删除 |
| `libs/hbb_common` | 上游为 git submodule，此处 vendored 为普通目录，仓库自包含 |
| 构建脚本 | `flutter/ndk_*.sh`、`flutter/build_android_deps.sh` 提取到 `scripts/` |
| `src/`、`libs/`、`res/`、`Cargo.*`、`build.rs`、`vcpkg.json` | 原样保留 |

详见 `UPSTREAM.md`。

## 构建

由 `.github/workflows/build-core.yml` 的 `workflow_dispatch` 触发，产出：

```
librustdesk.so
libc++_shared.so
```

按 ABI（`arm64-v8a` / `armeabi-v7a` / `x86_64`）分别出包，产物上传为 Actions artifact，供宿主落到 `app/src/main/jniLibs/<abi>/`。

构建工具链版本必须与上游 `1.5.1` 保持一致（Rust 1.75 / NDK r28c / cargo-ndk 3.1.2 / vcpkg 9e593bb），改版本号前先核对上游 `.github/workflows/flutter-build.yml`。

`flutter` feature 仍启用（`StreamSink` / `SyncReturn` 类型来自 `flutter_rust_bridge` crate），但 FRB 的 **codegen 生成物 `bridge_generated.rs` 不参与编译**——它只是 Dart 侧胶水，`lib.rs` 里已移除该模块声明，因此无需 Flutter SDK、`pub get` 或 codegen 步骤。

## 身份边界（硬约束）

**只有 RustDesk 账号/OIDC 登录与会话级密码/2FA 认证允许以 RustDesk 客户端身份发起。**

- 这两条链路的 `id` / `uuid` / `deviceInfo` / `apiDomain` 全部由核心自己生成与填充，JNI 桥只透传，不重写。
- 桥接层不得自行发起 HTTP 登录请求、不得缓存 token、不得伪装包名或改写授权 URL。
- 宿主（Termux Ultra）自有账号体系、AI 服务及其它集成工具的认证与本链路完全隔离，不共用存储、token 或回调。

CI 中有静态检查拦截桥接层出现自建 HTTP 调用。

## 对齐上游新版本

```bash
git remote add upstream https://github.com/rustdesk/rustdesk.git
git fetch upstream --tags
git merge <new-tag>          # 或变基
# 处理 UPSTREAM.md 列出的改动点冲突
# 重跑 build-core.yml，并回归会话连接与登录实测
```

## 许可与署名

本仓库是 RustDesk 的衍生作品，依 AGPL-3.0 发布。上游许可全文见 `LICENCE`，版权与修改声明见 `NOTICE`。
宿主应用须在关于页标注 RustDesk 版本与上游版权说明，并依 AGPL-3.0 提供本仓库对应源码。
