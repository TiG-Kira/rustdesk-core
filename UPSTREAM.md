# 上游来源与改动清单

## 来源

- 仓库：`rustdesk/rustdesk`
- Tag：`1.5.1`
- Commit：`git ls-remote https://github.com/rustdesk/rustdesk.git refs/tags/1.5.1`（fork 时记录）
- 许可：AGPL-3.0，许可全文 `LICENCE` 原样保留
- `libs/hbb_common` 上游为 submodule，fork 时固定在 `fae47e024dd83cef6aaecde0aa7d11731df11649`，此处 vendored 为普通目录

## 已删除（不参与 Android 控制端构建）

```
flutter/            # Dart UI；仅保留下列占位
  ├─ lib/.gitkeep           # FRB codegen --dart-output 目标
  ├─ macos/Runner/.gitkeep  # FRB codegen --c-output 目标
  └─ pubspec.yaml           # 占位，非上游原文件
src/ui/             # Sciter 桌面 UI（lib.rs:26-31 已在 android/flutter feature 下 cfg 排除）
appimage/ flatpak/ fastlane/ examples/ docs/
.github/            # 上游 CI，替换为 build-core.yml
Dockerfile entrypoint.sh build.py
AGENTS.md CLAUDE.md GEMINI.md
.gitmodules         # hbb_common 已 vendored，避免 checkout 覆盖
```

## 保留（原样，未改动）

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
flutter/{lib,macos/Runner}/.gitkeep + flutter/pubspec.yaml   # codegen 输出占位
```

## 后续计划改动（尚未实施，P1 基线先验证纯裁剪可编）

1. `src/android_jni.rs`（新增）：把 `flutter_ffi.rs` 的建连/认证/帧/输入/账号登录函数包成 `Java_com_termux_app_rustdesk_RustDeskCore_*` JNI 导出。**只做透传**，不含业务逻辑，不发起 HTTP。
2. `src/flutter.rs`：`StreamSink<EventToUI>`（FRB 绑 Dart）改为 `UiSink` 枚举（`Frb`｜`Jni(GlobalRef)`），`push_event_` 等约 10 处 `stream.add(...)` 改为 `sink.emit(...)`；`JNI_OnLoad` 缓存 JavaVM，网络线程 `AttachCurrentThread` 后回调 Kotlin，退出时释放 `GlobalRef`。
3. `Cargo.toml`：注册 `android_jni` 模块（并加 `#[cfg(target_os="android")]` gate）。

这三处是与上游逻辑产生分歧的地方，升级 tag 时优先处理冲突。

## 不变的身份代码（不要改）

```
src/hbbs_http/account.rs        # OIDC 登录请求体 {op,id,uuid,deviceInfo,apiDomain}
src/ui_interface.rs:1379        # get_login_device_info() -> {os,type:"client",name}
src/flutter_ffi.rs:2480         # main_account_auth() 内部自取 get_id()/get_uuid()
src/flutter_ffi.rs:273          # session_login()
src/flutter_ffi.rs:285          # session_send2fa()
```

改这些文件里任何身份字段，都会破坏「只有 RustDesk 登录以 RustDesk 身份发起」的约束。
