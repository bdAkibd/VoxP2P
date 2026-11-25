/// Mobile background foreground service and runtime permission bridge.
/// On Android, activates a foreground service with a persistent notification and wake lock
/// during active 1-on-1 calls and room voice sessions to prevent Android OOM termination,
/// and bridges native runtime permission checks/requests on MainActivity via JNI.

#[cfg(target_os = "android")]
fn clear_jni_exception(env: &mut jni::JNIEnv<'_>) -> bool {
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
        true
    } else {
        false
    }
}

#[cfg(target_os = "android")]
fn with_android_activity<R, F>(op_name: &str, f: F) -> Option<R>
where
    F: FnOnce(&mut jni::JNIEnv<'_>, &jni::objects::JObject<'_>) -> Option<R>,
{
    let ctx = match std::panic::catch_unwind(ndk_context::android_context) {
        Ok(ctx) => ctx,
        Err(_) => {
            eprintln!("[VoxP2P] ndk_context is not initialized, cannot run {op_name}");
            return None;
        }
    };
    let vm_ptr = ctx.vm();
    let act_ptr = ctx.context();

    if vm_ptr.is_null() || act_ptr.is_null() {
        eprintln!("[VoxP2P] ndk_context VM or Context is null, cannot run {op_name}");
        return None;
    }

    unsafe {
        let vm = match jni::JavaVM::from_raw(vm_ptr.cast()) {
            Ok(vm) => vm,
            Err(e) => {
                eprintln!("[VoxP2P] Failed to get JavaVM from raw pointer for {op_name}: {e:?}");
                return None;
            }
        };

        let mut env = match vm.attach_current_thread() {
            Ok(env) => env,
            Err(e) => {
                eprintln!("[VoxP2P] Failed to attach JNI thread for {op_name}: {e:?}");
                return None;
            }
        };

        // `ndk_context` owns this jobject (typically a GlobalRef); ManuallyDrop prevents
        // `JObject::drop` from calling DeleteLocalRef on a non-owned reference.
        let activity =
            std::mem::ManuallyDrop::new(jni::objects::JObject::from_raw(act_ptr.cast()));
        f(&mut env, &activity)
    }
}

#[cfg(target_os = "android")]
fn call_activity_bool_method(method_name: &str) -> bool {
    with_android_activity(method_name, |env, activity| {
        let res = env.call_method(activity, method_name, "()Z", &[]);
        if clear_jni_exception(env) {
            eprintln!("[VoxP2P] JNI exception calling MainActivity.{method_name}");
            return Some(false);
        }
        match res {
            Ok(val) => match val.z() {
                Ok(b) => Some(b),
                Err(e) => {
                    eprintln!("[VoxP2P] Unexpected return type from {method_name}: {e:?}");
                    Some(false)
                }
            },
            Err(e) => {
                eprintln!("[VoxP2P] Failed to call MainActivity.{method_name}: {e:?}");
                Some(false)
            }
        }
    })
    .unwrap_or(false)
}

#[cfg(target_os = "android")]
pub fn set_foreground_call_service(active: bool, title: &str, text: &str) {
    eprintln!("[VoxP2P] set_foreground_call_service(active={active}, title={title})");
    with_android_activity("CallService", |env, activity| {
        if active {
            let j_title = match env.new_string(title) {
                Ok(s) => {
                    if clear_jni_exception(env) {
                        eprintln!("[VoxP2P] JNI exception creating title string");
                        return None;
                    }
                    s
                }
                Err(e) => {
                    let _ = clear_jni_exception(env);
                    eprintln!("[VoxP2P] Failed to create JNI title string: {e:?}");
                    return None;
                }
            };

            let j_text = match env.new_string(text) {
                Ok(s) => {
                    if clear_jni_exception(env) {
                        eprintln!("[VoxP2P] JNI exception creating text string");
                        return None;
                    }
                    s
                }
                Err(e) => {
                    let _ = clear_jni_exception(env);
                    eprintln!("[VoxP2P] Failed to create JNI text string: {e:?}");
                    return None;
                }
            };

            let res = env.call_method(
                activity,
                "startCallService",
                "(Ljava/lang/String;Ljava/lang/String;)V",
                &[
                    jni::objects::JValue::Object(&j_title),
                    jni::objects::JValue::Object(&j_text),
                ],
            );
            if clear_jni_exception(env) || res.is_err() {
                if let Err(e) = res {
                    eprintln!("[VoxP2P] Failed to call MainActivity.startCallService: {e:?}");
                }
            }
        } else {
            let res = env.call_method(activity, "stopCallService", "()V", &[]);
            if clear_jni_exception(env) || res.is_err() {
                if let Err(e) = res {
                    eprintln!("[VoxP2P] Failed to call MainActivity.stopCallService: {e:?}");
                }
            }
        }
        Some(())
    });
}

#[cfg(not(target_os = "android"))]
pub fn set_foreground_call_service(_active: bool, _title: &str, _text: &str) {
    // No-op on desktop platforms
}

#[cfg(target_os = "android")]
pub fn has_android_mic_permission() -> bool {
    call_activity_bool_method("hasRecordAudioPermission")
}

#[cfg(not(target_os = "android"))]
pub fn has_android_mic_permission() -> bool {
    true
}

#[cfg(target_os = "android")]
pub fn request_android_mic_permission() -> bool {
    call_activity_bool_method("ensureAudioPermissions")
}

#[cfg(not(target_os = "android"))]
pub fn request_android_mic_permission() -> bool {
    true
}

#[cfg(target_os = "android")]
pub fn request_android_camera_permission() -> bool {
    call_activity_bool_method("ensureCameraPermissions")
}

#[cfg(not(target_os = "android"))]
pub fn request_android_camera_permission() -> bool {
    true
}
