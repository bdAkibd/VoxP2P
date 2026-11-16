// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    std::panic::set_hook(Box::new(|info| {
        let backtrace = std::backtrace::Backtrace::capture();
        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Box<dyn Any>".to_string()
        };
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown location".to_string());

        let msg = format!("VOXP2P FATAL PANIC at {location}:\n{payload}\n\nStack Backtrace:\n{backtrace:?}");
        eprintln!("{msg}");

        // Save to current directory
        let _ = std::fs::write("voxp2p_crash.log", &msg);

        // Save to LocalAppData
        if let Some(appdata) = std::env::var_os("LOCALAPPDATA") {
            let dir = std::path::PathBuf::from(appdata).join("VoxP2P");
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(dir.join("crash.log"), &msg);
        }

        // Save to AppData/Roaming
        if let Some(appdata) = std::env::var_os("APPDATA") {
            let dir = std::path::PathBuf::from(appdata).join("com.voxp2p.app");
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(dir.join("crash.log"), &msg);
        }

        #[cfg(target_os = "windows")]
        {
            use std::ffi::CString;
            let title = CString::new("VoxP2P Fatal Error").unwrap_or_default();
            let body = CString::new(format!(
                "VoxP2P encountered an unhandled error:\n\n{location}\n{payload}\n\nA crash log was written to crash.log."
            )).unwrap_or_default();
            unsafe {
                unsafe extern "system" {
                    fn MessageBoxA(hwnd: isize, text: *const i8, caption: *const i8, utype: u32) -> i32;
                }
                MessageBoxA(0, body.as_ptr(), title.as_ptr(), 0x00000010 /* MB_ICONERROR */);
            }
        }
    }));

    voxp2p_lib::run();
}
