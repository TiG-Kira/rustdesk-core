# 上游来源与改动清单

## 来源

- 仓库：`rustdesk/rustdesk`
- Tag：`1.5.1`
- Commit：`git ls-remote https://github.com/rustdesk/rustdesk.git refs/tags/1.5.1`（fork 时记录）
- 许可：AGPL-3.0，许可全文 `LICENCE` 原样保留
- `libs/hbb_common` 上游为 submodule，fork 时固定在 `fae47e024dd83cef6aaecde0aa7d11731df11649`，此处 vendored 为普通目录

## 已删除（不参与 Android 控制端构建）

```
flutter/            # Dart UI，整目录删除（含 pubspec / lib / macos / android / ...）
src/ui/             # Sciter 桌面 UI（lib.rs:26-31 已在 android/flutter feature 下 cfg 排除）
appimage/ flatpak/ fastlane/ examples/ docs/
.github/            # 上游 CI，替换为 build-core.yml
Dockerfile entrypoint.sh build.py
AGENTS.md CLAUDE.md GEMINI.md
.gitmodules         # hbb_common 已 vendored，避免 checkout 覆盖
```

## 保留（除 `src/lib.rs` 一行删除外，其余原样未改动）

```
src/                # 含 lib.rs / flutter.rs / flutter_ffi.rs / hbbs_http/ 等核心
libs/               # 全部 9 个 crate（Cargo.toml workspace members 引用了全部，不可删）
res/  .cargo/
Cargo.toml  Cargo.lock  build.rs  vcpkg.json
LICENCE
```

## 移动（内容未改）

```
flutter/ndk_arm.sh            -> scripts/ndk_arm.sh
flutter/ndk_arm64.sh          -> scripts/ndk_arm64.sh
flutter/ndk_x64.sh            -> scripts/ndk_x64.sh
flutter/ndk_x86.sh            -> scripts/ndk_x86.sh
flutter/build_android_deps.sh -> scripts/build_android_deps.sh
```

`build_android_deps.sh` 用 `SCRIPTDIR/../vcpkg.json` 定位，从 `flutter/` 移到 `scripts/` 后仍指向仓库根，无需改动。

## 新增

```
README.md            # 本仓库说明
UPSTREAM.md          # 本文件
NOTICE               # 版权与修改声明
.github/workflows/build-core.yml
```

## 修改（唯一改动的上游文件）

```
src/lib.rs   删除 `#[cfg(...)] mod bridge_generated;`
```

`bridge_generated.rs` 是 FRB codegen 生成的 Dart 侧胶水，上游 `lib.rs:38` 声明它，
但全仓库除该行外无任何引用，也不参与 Rust 逻辑。本仓库自研 JNI UI、不需要 Dart，
故一并删掉 codegen 步骤、Flutter SDK 依赖与 `flutter/` 占位目录。
`StreamSink` / `SyncReturn` 类型仍由 `flutter_rust_bridge` crate 提供，`flutter` feature 保留。

**连带修改（run `42281757405` 实测暴露）**：`EventToUI` 是自定义 enum，其
`IntoIntoDart` 实现正是由 codegen 生成的。删掉生成物后，`flutter.rs` 里 6 处
`stream.add(EventToUI::...)` 失去 trait bound 而无法编译。Android target 下事件出口本就
改走 JNI 旁路，故这 6 处 `.add()` 用 `#[cfg(not(target_os = "android"))]` 消去；
类型定义与 FRB 导出签名均不动，非 Android 平台行为完全不变。
`StreamSink<String>` 的 `.add(...)` 不受影响（String 的实现由 FRB crate 自带）。

## 后续计划改动（本地已写好，待 P1 基线 CI 通过后提交验证）

采用 **JNI 旁路**方案，**不改任何 FRB 类型与导出签名**（早期设想的 `UiSink` 枚举会改动
`session_start` / `start_global_event_stream` 的参数类型，触发 FRB codegen 解析，冲突面大，已弃用）：

1. `src/android_jni.rs`（新增，`#[cfg(target_os="android")]`）：把 `flutter_ffi.rs` 的建连/认证/
   帧/输入/账号登录函数包成 `Java_com_termux_app_rustdesk_RustDeskCore_*` JNI 导出。
   **只做透传**，不含业务逻辑，不发起 HTTP。
2. `src/flutter.rs`：在既有事件出口旁**额外转发一份给 JNI**（`push_event_` 里 `out` 已是 JSON、
   `Rgba` / `Cursor` / `Texture` 推送点、`push_global_event`），FRB 的 `stream.add(...)` 原样保留。
   全部调用点用 `#[cfg(target_os="android")]` 包住，非 Android 平台零影响。
3. `src/lib.rs`：注册 `android_jni` 模块。

**JNI_OnLoad 不能重复定义**：`libs/scrap/src/android/ffi.rs:486` 已导出 `JNI_OnLoad` 并持有
私有 `JVM` 静态。桥接层在 `nativeInit(ctx, callback)` 里用 `env.get_java_vm()` 自己缓存一份
`JavaVM` + 回调对象的 `GlobalRef`；网络线程推事件时 `attach_current_thread()`，会话关闭时释放引用。
`jni = "0.21"` 已在 `Cargo.toml` 的 `[target.'cfg(target_os = "android")'.dependencies]` 中，无需加依赖。

这些是与上游逻辑产生分歧的地方，升级 tag 时优先处理冲突。

## 不变的身份代码（不要改）

```
src/hbbs_http/account.rs        # OIDC 登录请求体 {op,id,uuid,deviceInfo,apiDomain}
src/ui_interface.rs:1379        # get_login_device_info() -> {os,type:"client",name}
src/flutter_ffi.rs:2480         # main_account_auth() 内部自取 get_id()/get_uuid()
src/flutter_ffi.rs:273          # session_login()
src/flutter_ffi.rs:285          # session_send2fa()
```

改这些文件里任何身份字段，都会破坏「只有 RustDesk 登录以 RustDesk 身份发起」的约束。
