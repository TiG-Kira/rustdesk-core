//! Termux Ultra 自研 UI 的 JNI 桥接层。
//!
//! 设计约束（见 README.md「身份边界」）：
//!   * 本文件**只做透传**：把 `flutter_ffi.rs` / `flutter.rs` 的既有函数包成 JNI 导出，
//!     不含业务逻辑、不自行发起 HTTP、不生成或改写任何客户端身份字段。
//!   * RustDesk 账号/OIDC 登录与会话密码/2FA 的 `id` / `uuid` / `deviceInfo` 等
//!     全部由核心自己生成（见 `main_account_auth` / `session_login`），UI 不接触。
//!   * 宿主（Termux Ultra）自有账号体系与其它集成工具的认证不经过本桥。
//!
//! 事件出口采用「旁路」而非替换 FRB 类型：`flutter.rs` 在既有 `stream.add(...)` 旁
//! 额外调用 `emit_event(...)`，FRB 的导出签名与 Dart 侧胶水均不改动。
//!
//! `JNI_OnLoad` **不能在此定义**：`libs/scrap/src/android/ffi.rs` 已导出同名符号。
//! 本桥在 `nativeInit` 中用 `env.get_java_vm()` 自行缓存 VM 与回调对象的全局引用。

#![cfg(target_os = "android")]

use std::str::FromStr;
use std::sync::RwLock;

use hbb_common::{config, log};
use jni::objects::{GlobalRef, JClass, JObject, JString, JValue};
use jni::sys::{jboolean, jint, jlong, jstring};
use jni::{JNIEnv, JavaVM};

use crate::flutter_ffi::{self, SessionID};

/// Kotlin 侧 `RustDeskCore.onEvent(sessionId, kind, payload)` 的回调对象。
/// 由 `nativeInit` 注册，`nativeShutdown` 释放。
static EVENT_SINK: RwLock<Option<EventSink>> = RwLock::new(None);

struct EventSink {
    vm: JavaVM,
    callback: GlobalRef,
}

impl EventSink {
    /// 在任意 Rust 线程把事件 JSON 交给 Kotlin。网络线程需临时附着 JVM。
    fn emit(&self, session_id: &str, kind: &str, payload: &str) {
        let mut env = match self.vm.attach_current_thread() {
            Ok(env) => env,
            Err(err) => {
                log::error!("Failed to attach JVM for event: {:?}", err);
                return;
            }
        };
        let sid = match env.new_string(session_id) {
            Ok(s) => s,
            Err(err) => {
                log::error!("Failed to alloc session id string: {:?}", err);
                return;
            }
        };
        let k = match env.new_string(kind) {
            Ok(s) => s,
            Err(err) => {
                log::error!("Failed to alloc kind string: {:?}", err);
                return;
            }
        };
        let p = match env.new_string(payload) {
            Ok(s) => s,
            Err(err) => {
                log::error!("Failed to alloc payload string: {:?}", err);
                return;
            }
        };
        // 先绑定为具名变量，避免临时 JObject 在 call_method 前被销毁（悬垂引用）。
        let sid_obj = JObject::from(sid);
        let kind_obj = JObject::from(k);
        let payload_obj = JObject::from(p);
        let args = [
            JValue::Object(&sid_obj),
            JValue::Object(&kind_obj),
            JValue::Object(&payload_obj),
        ];
        if let Err(err) = env.call_method(
            self.callback.as_obj(),
            "onEvent",
            "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V",
            &args,
        ) {
            log::error!("Failed to call onEvent: {:?}", err);
        }
    }
}

/// 供 `flutter.rs` 判断帧是否有人消费：JNI sink 已注册时须把 `is_sent` 置 true，
/// 否则 `on_rgba_soft_render` 会立刻把 `rgba_data.valid` 复位、帧被丢弃。
pub fn jni_sink_active() -> bool {
    EVENT_SINK.read().unwrap().is_some()
}

/// 供 `flutter.rs` 的事件出口旁路调用。非 Android 平台该函数不存在（cfg 门控在调用点）。
pub fn emit_event(session_id: &str, kind: &str, payload: &str) {
    if let Ok(guard) = EVENT_SINK.read() {
        if let Some(sink) = guard.as_ref() {
            sink.emit(session_id, kind, payload);
        }
    }
}

fn jstring_to_string(env: &mut JNIEnv, s: &JString) -> String {
    env.get_string(s)
        .map(|v| -> String { v.into() })
        .unwrap_or_default()
}

fn parse_sid(s: &str) -> Option<SessionID> {
    SessionID::from_str(s).ok()
}

/// 初始化核心：设置应用目录、注册事件回调、缓存 JavaVM。
/// `appDir` / `homeDir` 由宿主传入，供核心读写配置与日志。
#[no_mangle]
pub unsafe extern "system" fn Java_com_termux_app_rustdesk_RustDeskCore_nativeInit(
    mut env: JNIEnv,
    _class: JClass,
    ctx: JObject,
    app_dir: JString,
    home_dir: JString,
    callback: JObject,
) {
    if ctx.is_null() || callback.is_null() {
        log::error!("nativeInit: null context or callback");
        return;
    }
    *config::APP_DIR.write().unwrap() = jstring_to_string(&mut env, &app_dir);
    *config::APP_HOME_DIR.write().unwrap() = jstring_to_string(&mut env, &home_dir);

    if let Ok(vm) = env.get_java_vm() {
        if let Ok(global) = env.new_global_ref(callback) {
            *EVENT_SINK.write().unwrap() = Some(EventSink { vm, callback: global });
            log::info!("rustdesk-core JNI event sink registered");
        } else {
            log::error!("nativeInit: failed to create global ref for callback");
        }
    } else {
        log::error!("nativeInit: failed to get JavaVM");
    }
}

#[no_mangle]
pub extern "system" fn Java_com_termux_app_rustdesk_RustDeskCore_nativeShutdown(
    _env: JNIEnv,
    _class: JClass,
) {
    *EVENT_SINK.write().unwrap() = None;
}

/// 创建会话。参数与 `session_add_sync` 一一对应，只做类型转换后直接转发。
#[no_mangle]
pub unsafe extern "system" fn Java_com_termux_app_rustdesk_RustDeskCore_nativeSessionAdd(
    mut env: JNIEnv,
    _class: JClass,
    session_id: JString,
    peer_id: JString,
    password: JString,
    force_relay: jboolean,
    switch_uuid: JString,
    conn_token: JString,
) -> jstring {
    let sid_str = jstring_to_string(&mut env, &session_id);
    let sid = match parse_sid(&sid_str) {
        Some(v) => v,
        None => {
            log::error!("nativeSessionAdd: invalid session id: {}", sid_str);
            return std::ptr::null_mut();
        }
    };
    let token = jstring_to_string(&mut env, &conn_token);
    let res = flutter_ffi::session_add_sync(
        sid,
        jstring_to_string(&mut env, &peer_id),
        false, // is_file_transfer
        false, // is_view_camera
        false, // is_port_forward
        false, // is_rdp
        false, // is_terminal
        jstring_to_string(&mut env, &switch_uuid),
        force_relay != 0,
        jstring_to_string(&mut env, &password),
        false, // is_shared_password
        if token.is_empty() { None } else { Some(token) },
    );
    let out = res.0;
    match env.new_string(out) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// 启动会话。等价 `session_start_` 的连接启动逻辑，但不构造 FRB `StreamSink`，
/// 事件出口由 `emit_event` 旁路承担。实现见 `flutter::session_start_jni`。
#[no_mangle]
pub unsafe extern "system" fn Java_com_termux_app_rustdesk_RustDeskCore_nativeSessionStart(
    mut env: JNIEnv,
    _class: JClass,
    session_id: JString,
    peer_id: JString,
) -> jboolean {
    let sid_str = jstring_to_string(&mut env, &session_id);
    match parse_sid(&sid_str) {
        Some(sid) => match crate::flutter::session_start_jni(&sid, &jstring_to_string(&mut env, &peer_id)) {
            Ok(()) => 1,
            Err(err) => {
                log::error!("nativeSessionStart failed: {:?}", err);
                0
            }
        },
        None => {
            log::error!("nativeSessionStart: invalid session id: {}", sid_str);
            0
        }
    }
}

#[no_mangle]
pub unsafe extern "system" fn Java_com_termux_app_rustdesk_RustDeskCore_nativeSessionClose(
    mut env: JNIEnv,
    _class: JClass,
    session_id: JString,
) {
    if let Some(sid) = parse_sid(&jstring_to_string(&mut env, &session_id)) {
        flutter_ffi::session_close(sid);
    }
}

/// 会话级密码 / OS 账号登录。**纯透传**，凭据直达核心，不缓存、不改写。
#[no_mangle]
pub unsafe extern "system" fn Java_com_termux_app_rustdesk_RustDeskCore_nativeSessionLogin(
    mut env: JNIEnv,
    _class: JClass,
    session_id: JString,
    os_username: JString,
    os_password: JString,
    password: JString,
    remember: jboolean,
) {
    if let Some(sid) = parse_sid(&jstring_to_string(&mut env, &session_id)) {
        flutter_ffi::session_login(
            sid,
            jstring_to_string(&mut env, &os_username),
            jstring_to_string(&mut env, &os_password),
            jstring_to_string(&mut env, &password),
            remember != 0,
        );
    }
}

#[no_mangle]
pub unsafe extern "system" fn Java_com_termux_app_rustdesk_RustDeskCore_nativeSessionSend2fa(
    mut env: JNIEnv,
    _class: JClass,
    session_id: JString,
    code: JString,
    trust_this_device: jboolean,
) {
    if let Some(sid) = parse_sid(&jstring_to_string(&mut env, &session_id)) {
        flutter_ffi::session_send2fa(
            sid,
            jstring_to_string(&mut env, &code),
            trust_this_device != 0,
        );
    }
}

/// RustDesk 账号/OIDC 登录。**只透传 op 与 rememberMe**；
/// `id` / `uuid` / `deviceInfo` / `apiDomain` 与授权 URL 全部由核心生成与处理。
#[no_mangle]
pub unsafe extern "system" fn Java_com_termux_app_rustdesk_RustDeskCore_nativeAccountAuth(
    mut env: JNIEnv,
    _class: JClass,
    op: JString,
    remember_me: jboolean,
) {
    flutter_ffi::main_account_auth(jstring_to_string(&mut env, &op), remember_me != 0);
}

#[no_mangle]
pub extern "system" fn Java_com_termux_app_rustdesk_RustDeskCore_nativeAccountAuthCancel(
    _env: JNIEnv,
    _class: JClass,
) {
    flutter_ffi::main_account_auth_cancel();
}

/// 返回 OIDC 登录状态/授权 URL 的 JSON，UI 只负责展示与打开浏览器。
#[no_mangle]
pub extern "system" fn Java_com_termux_app_rustdesk_RustDeskCore_nativeAccountAuthResult(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let mut env = env;
    match env.new_string(flutter_ffi::main_account_auth_result()) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// 帧缓冲字节数（配合 `nativeGetRgba` 使用）。
#[no_mangle]
pub unsafe extern "system" fn Java_com_termux_app_rustdesk_RustDeskCore_nativeGetRgbaSize(
    mut env: JNIEnv,
    _class: JClass,
    session_id: JString,
    display: jint,
) -> jlong {
    match parse_sid(&jstring_to_string(&mut env, &session_id)) {
        Some(sid) => crate::flutter::session_get_rgba_size(sid, display as usize) as jlong,
        None => 0,
    }
}

/// 帧缓冲：返回 Rust 侧 RGBA 裸指针，复用上游已导出的 C ABI `session_get_rgba`
/// （它内部已处理 `display_rgbas` 锁与 valid 判定，避免在此重复 trait 调用）。
/// Kotlin 须在 `Rgba` 事件回调内同步拷贝，不得跨线程持有该指针。
#[no_mangle]
pub unsafe extern "system" fn Java_com_termux_app_rustdesk_RustDeskCore_nativeGetRgba(
    mut env: JNIEnv,
    _class: JClass,
    session_id: JString,
    display: jint,
) -> jlong {
    let sid_str = jstring_to_string(&mut env, &session_id);
    // C ABI 参数是 *const c_char，需 NUL 结尾；仅用于查表，不跨线程保存。
    let c_sid = match std::ffi::CString::new(sid_str) {
        Ok(v) => v,
        Err(_) => return 0,
    };
    // 上游签名写的是 Rust `char` 而非 `c_char`，内部 `c as _` 又转回 `*const c_char`，
    // 故此处的指针类型转换只是绕过签名书写差异，语义仍是 NUL 结尾的字节串。
    crate::flutter::session_get_rgba(c_sid.as_ptr() as *const char, display as usize) as jlong
}

#[no_mangle]
pub unsafe extern "system" fn Java_com_termux_app_rustdesk_RustDeskCore_nativeNextRgba(
    mut env: JNIEnv,
    _class: JClass,
    session_id: JString,
    display: jint,
) {
    if let Some(sid) = parse_sid(&jstring_to_string(&mut env, &session_id)) {
        crate::flutter::session_next_rgba(sid, display as usize);
    }
}

#[no_mangle]
pub unsafe extern "system" fn Java_com_termux_app_rustdesk_RustDeskCore_nativeSetSize(
    mut env: JNIEnv,
    _class: JClass,
    session_id: JString,
    display: jint,
    width: jint,
    height: jint,
) {
    if let Some(sid) = parse_sid(&jstring_to_string(&mut env, &session_id)) {
        crate::flutter::session_set_size(
            sid,
            display as usize,
            width as usize,
            height as usize,
        );
    }
}

#[no_mangle]
pub unsafe extern "system" fn Java_com_termux_app_rustdesk_RustDeskCore_nativeSendMouse(
    mut env: JNIEnv,
    _class: JClass,
    session_id: JString,
    msg: JString,
) {
    if let Some(sid) = parse_sid(&jstring_to_string(&mut env, &session_id)) {
        flutter_ffi::session_send_mouse(sid, jstring_to_string(&mut env, &msg));
    }
}

#[no_mangle]
pub unsafe extern "system" fn Java_com_termux_app_rustdesk_RustDeskCore_nativeInputKey(
    mut env: JNIEnv,
    _class: JClass,
    session_id: JString,
    name: JString,
    down: jboolean,
    press: jboolean,
    alt: jboolean,
    ctrl: jboolean,
    shift: jboolean,
    command: jboolean,
) {
    if let Some(sid) = parse_sid(&jstring_to_string(&mut env, &session_id)) {
        flutter_ffi::session_input_key(
            sid,
            jstring_to_string(&mut env, &name),
            down != 0,
            press != 0,
            alt != 0,
            ctrl != 0,
            shift != 0,
            command != 0,
        );
    }
}

#[no_mangle]
pub unsafe extern "system" fn Java_com_termux_app_rustdesk_RustDeskCore_nativeGetLocalOption(
    mut env: JNIEnv,
    _class: JClass,
    key: JString,
) -> jstring {
    let res = flutter_ffi::main_get_local_option(jstring_to_string(&mut env, &key));
    match env.new_string(res.0) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "system" fn Java_com_termux_app_rustdesk_RustDeskCore_nativeSetLocalOption(
    mut env: JNIEnv,
    _class: JClass,
    key: JString,
    value: JString,
) {
    flutter_ffi::main_set_local_option(
        jstring_to_string(&mut env, &key),
        jstring_to_string(&mut env, &value),
    );
}

#[no_mangle]
pub unsafe extern "system" fn Java_com_termux_app_rustdesk_RustDeskCore_nativeSessionRefresh(
    mut env: JNIEnv,
    _class: JClass,
    session_id: JString,
    display: jint,
) {
    if let Some(sid) = parse_sid(&jstring_to_string(&mut env, &session_id)) {
        flutter_ffi::session_refresh(sid, display as usize);
    }
}

#[no_mangle]
pub unsafe extern "system" fn Java_com_termux_app_rustdesk_RustDeskCore_nativeSessionReconnect(
    mut env: JNIEnv,
    _class: JClass,
    session_id: JString,
    force_relay: jboolean,
) {
    if let Some(sid) = parse_sid(&jstring_to_string(&mut env, &session_id)) {
        flutter_ffi::session_reconnect(sid, force_relay != 0);
    }
}
