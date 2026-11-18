/// Mobile background foreground service bridge.
/// On Android, activates a foreground service with a persistent notification and wake lock
/// during active 1-on-1 calls and room voice sessions to prevent Android OOM termination.

#[cfg(target_os = "android")]
pub fn set_foreground_call_service(active: bool, title: &str, text: &str) {
    let ctx = ndk_context::android_context();
    let vm_ptr = ctx.vm();
    let act_ptr = ctx.context();

    if vm_ptr.is_null() || act_ptr.is_null() {
        eprintln!("[VoxP2P] ndk_context VM or Context is null, cannot toggle CallService");
        return;
    }

    unsafe {
        match jni::JavaVM::from_raw(vm_ptr.cast()) {
            Ok(vm) => {
                if let Ok(mut env) = vm.attach_current_thread() {
                    let activity = jni::objects::JObject::from_raw(act_ptr.cast());
                    let class_name = "com/voxp2p/app/CallService";
                    match env.find_class(class_name) {
                        Ok(service_class) => {
                            if active {
                                let j_title = env.new_string(title).unwrap_or_default();
                                let j_text = env.new_string(text).unwrap_or_default();
                                let _ = env.call_static_method(
                                    service_class,
                                    "startService",
                                    "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;)V",
                                    &[
                                        jni::objects::JValue::Object(&activity),
                                        jni::objects::JValue::Object(&j_title),
                                        jni::objects::JValue::Object(&j_text),
                                    ],
                                );
                            } else {
                                let _ = env.call_static_method(
                                    service_class,
                                    "stopService",
                                    "(Landroid/content/Context;)V",
                                    &[jni::objects::JValue::Object(&activity)],
                                );
                            }
                        }
                        Err(e) => {
                            eprintln!("[VoxP2P] Failed to find CallService class: {e:?}");
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("[VoxP2P] Failed to get JavaVM from raw pointer: {e:?}");
            }
        }
    }
}

#[cfg(not(target_os = "android"))]
pub fn set_foreground_call_service(_active: bool, _title: &str, _text: &str) {
    // No-op on desktop platforms
}
