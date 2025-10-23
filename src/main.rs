use std::collections::HashSet;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::ptr;

use cocoa::appkit::{NSApp, NSApplication, NSButton, NSPanel, NSScreen, NSView, NSWindow};
use cocoa::base::{id, nil, YES};
use cocoa::foundation::{NSPoint, NSRect, NSSize, NSString};

#[macro_use]
extern crate objc;

#[link(name = "CoreGraphics", kind = "framework")]
#[link(name = "Foundation", kind = "framework")]
#[link(name = "AppKit", kind = "framework")]
extern "C" {
    fn CGWindowListCopyWindowInfo(option: u32, relative_to_window: u32) -> *const c_void;

    fn CFArrayGetCount(array: *const c_void) -> isize;
    fn CFArrayGetValueAtIndex(array: *const c_void, idx: isize) -> *const c_void;
    fn CFDictionaryGetValue(dict: *const c_void, key: *const c_void) -> *const c_void;
    fn CFStringCreateWithCString(
        allocator: *const c_void,
        cstr: *const c_char,
        encoding: u32,
    ) -> *const c_void;
    fn CFStringGetLength(string: *const c_void) -> isize;
    fn CFStringGetCString(
        string: *const c_void,
        buffer: *mut c_char,
        buffer_size: isize,
        encoding: u32,
    ) -> bool;
    fn CFRelease(cf: *const c_void);
    fn CFNumberGetValue(number: *const c_void, number_type: i32, value_ptr: *mut c_void) -> bool;
}

const K_CF_STRING_ENCODING_UTF8: u32 = 0x08000100;
const K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY: u32 = 1;
const K_CF_NUMBER_DOUBLE_TYPE: i32 = 13;

fn main() {
    let ignored_apps = get_ignored_apps();

    println!("SEARCHING FOR WINDOWS WITH TITLE 'Open'...\n");

    match find_open_windows(&ignored_apps) {
        Ok(results) => {
            println!("Scanned {} total windows", results.total_windows);

            if !results.open_windows.is_empty() {
                println!(
                    "\nFOUND {} WINDOWS WITH TITLE 'Open':",
                    results.open_windows.len()
                );

                let mut panels: Vec<id> = Vec::new();

                for window in &results.open_windows {
                    println!("  '{}' from {}", window.title, window.app_name);
                    println!("     Bounds: {}", window.bounds);
                    println!("     Window Number: {}", window.window_number);
                    println!("     PID: {}", window.pid);

                    if let Some(panel) = create_overlay_panel(&window) {
                        panels.push(panel);
                        println!("     Created overlay panel");
                    } else {
                        println!("     Failed to create overlay panel");
                    }
                    println!();
                }

                if !panels.is_empty() {
                    println!(
                        "{} overlay panels created. Press Ctrl+C to exit.",
                        panels.len()
                    );

                    unsafe {
                        let app = NSApp();
                        if app != nil {
                            println!("NSApp initialized successfully");

                            use cocoa::appkit::NSApplicationActivationPolicy;
                            app.setActivationPolicy_(
                                NSApplicationActivationPolicy::NSApplicationActivationPolicyAccessory,
                            );

                            app.activateIgnoringOtherApps_(YES);

                            println!("Starting NSApplication run loop...");

                            app.run();
                        } else {
                            println!("Failed to get NSApp - panels may not be visible");
                        }
                    }
                }
            } else {
                println!("\nNo windows with title 'Open' found");
            }
        }
        Err(e) => {
            println!("Error: {}", e);
        }
    }

    println!("Complete!");
}

fn create_overlay_panel(window: &OpenWindow) -> Option<id> {
    unsafe {
        println!("Creating NSPanel overlay for {} window...", window.app_name);

        let (cg_x, cg_y, orig_width, orig_height) = parse_bounds_values(&window.bounds)?;

        let main_screen = NSScreen::mainScreen(nil);
        let screen_frame = NSScreen::frame(main_screen);
        let screen_height = screen_frame.size.height;
        let ns_y = screen_height - cg_y - orig_height;

        println!("📺 Screen height: {}", screen_height);

        let panel_width = orig_width + 300.0;
        let panel_height = orig_height;
        let panel_x = cg_x;
        let panel_y = ns_y;

        println!(
            "Original CG coords: x={}, y={}, w={}, h={}",
            cg_x, cg_y, orig_width, orig_height
        );
        println!(
            "Panel NS coords: x={}, y={}, w={}, h={}",
            panel_x, panel_y, panel_width, panel_height
        );

        let panel_frame = NSRect::new(
            NSPoint::new(panel_x, panel_y),
            NSSize::new(panel_width, panel_height),
        );

        use cocoa::appkit::{NSBackingStoreType, NSWindowStyleMask};

        let style_mask = NSWindowStyleMask::NSBorderlessWindowMask;

        let panel: id = NSPanel::alloc(nil).initWithContentRect_styleMask_backing_defer_(
            panel_frame,
            style_mask,
            NSBackingStoreType::NSBackingStoreBuffered,
            false,
        );

        if panel == nil {
            return None;
        }

        panel.setLevel_(10);

        use cocoa::base::NO;
        panel.setOpaque_(NO);
        panel.setAlphaValue_(0.9);
        panel.setHasShadow_(YES);
        panel.setMovableByWindowBackground_(YES);

        let window_title = NSString::alloc(nil).init_str("PANEL DETECTOR OVERLAY");
        NSWindow::setTitle_(panel, window_title);

        let content_view: id = NSView::initWithFrame_(
            NSView::alloc(nil),
            NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(panel_width, panel_height),
            ),
        );

        if content_view == nil {
            return None;
        }

        panel.setContentView_(content_view);

        let button_width = panel_width * 0.8; // 80% of panel width
        let button_height = panel_height * 0.3; // 30% of panel height
        let button_x = (panel_width - button_width) / 2.0; // Center horizontally
        let button_y = (panel_height - button_height) / 2.0; // Center vertically

        let button_frame = NSRect::new(
            NSPoint::new(button_x, button_y),
            NSSize::new(button_width, button_height),
        );

        let button: id = NSButton::initWithFrame_(NSButton::alloc(nil), button_frame);
        if button == nil {
            return None;
        }

        let title_str = format!("PANEL DETECTED: {}", window.app_name);
        let title = NSString::alloc(nil).init_str(&title_str);
        NSButton::setTitle_(button, title);

        content_view.addSubview_(button);

        let close_button_size = 30.0;
        let close_button_margin = 10.0;
        let close_button_frame = NSRect::new(
            NSPoint::new(
                panel_width - close_button_size - close_button_margin,
                panel_height - close_button_size - close_button_margin,
            ),
            NSSize::new(close_button_size, close_button_size),
        );

        let close_button: id = NSButton::initWithFrame_(NSButton::alloc(nil), close_button_frame);
        if close_button != nil {
            let close_title = NSString::alloc(nil).init_str("✕");
            NSButton::setTitle_(close_button, close_title);

            let _: () = msg_send![close_button, setTarget: panel];
            let _: () = msg_send![close_button, setAction: sel!(orderOut:)];

            content_view.addSubview_(close_button);
        }

        panel.makeKeyAndOrderFront_(nil);
        panel.orderFrontRegardless();

        println!("Panel should now be visible!");
        println!(
            "   Panel frame: x={}, y={}, w={}, h={}",
            panel_x, panel_y, panel_width, panel_height
        );

        Some(panel)
    }
}

fn parse_bounds_values(bounds_str: &str) -> Option<(f64, f64, f64, f64)> {
    let mut x = 0.0;
    let mut y = 0.0;
    let mut w = 0.0;
    let mut h = 0.0;

    for part in bounds_str.split(", ") {
        if let Some(val_str) = part.strip_prefix("x:") {
            x = val_str.parse().ok()?;
        } else if let Some(val_str) = part.strip_prefix("y:") {
            y = val_str.parse().ok()?;
        } else if let Some(val_str) = part.strip_prefix("w:") {
            w = val_str.parse().ok()?;
        } else if let Some(val_str) = part.strip_prefix("h:") {
            h = val_str.parse().ok()?;
        }
    }

    Some((x, y, w, h))
}

fn get_ignored_apps() -> HashSet<String> {
    let mut ignored = HashSet::new();
    ignored.insert("notification center".to_lowercase());
    ignored.insert("notificationcenter".to_lowercase());
    ignored.insert("sketchybar".to_lowercase());
    ignored.insert("borders".to_lowercase());
    ignored.insert("control center".to_lowercase());
    ignored.insert("controlcenter".to_lowercase());
    ignored.insert("dock".to_lowercase());
    ignored.insert("menubar".to_lowercase());
    ignored.insert("spotlight".to_lowercase());
    ignored
}

#[derive(Debug)]
struct OpenWindowResults {
    total_windows: usize,
    open_windows: Vec<OpenWindow>,
}

#[derive(Debug)]
struct OpenWindow {
    title: String,
    app_name: String,
    bounds: String,
    window_number: i64,
    pid: i32,
}

fn find_open_windows(ignored_apps: &HashSet<String>) -> Result<OpenWindowResults, String> {
    unsafe {
        let window_list = CGWindowListCopyWindowInfo(K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY, 0);
        if window_list.is_null() {
            return Err("Failed to get window list".to_string());
        }

        let count = CFArrayGetCount(window_list);
        let mut open_windows = Vec::new();
        let mut total_processed = 0;

        for i in 0..count {
            let window_dict = CFArrayGetValueAtIndex(window_list, i);
            if window_dict.is_null() {
                continue;
            }

            let app_name = get_dict_string_safe(window_dict, "kCGWindowOwnerName")
                .unwrap_or_else(|| "Unknown".to_string());

            if should_ignore_app(&app_name, ignored_apps) {
                continue;
            }

            total_processed += 1;

            let title = get_dict_string_safe(window_dict, "kCGWindowName")
                .unwrap_or_else(|| "No Title".to_string());

            if title != "Open" {
                continue;
            }

            let bounds = parse_bounds_from_dict(window_dict);

            let window_number =
                get_dict_number_safe(window_dict, "kCGWindowNumber").unwrap_or(0.0) as i64;

            let pid = get_dict_number_safe(window_dict, "kCGWindowOwnerPID").unwrap_or(0.0) as i32;

            open_windows.push(OpenWindow {
                title,
                app_name: app_name.clone(),
                bounds,
                window_number,
                pid,
            });
        }

        CFRelease(window_list);

        Ok(OpenWindowResults {
            total_windows: total_processed,
            open_windows,
        })
    }
}

fn parse_bounds_from_dict(dict: *const c_void) -> String {
    if let Some(bounds_dict) = get_dict_value(dict, "kCGWindowBounds") {
        let x = get_dict_number_safe(bounds_dict, "X").unwrap_or(0.0);
        let y = get_dict_number_safe(bounds_dict, "Y").unwrap_or(0.0);
        let width = get_dict_number_safe(bounds_dict, "Width").unwrap_or(0.0);
        let height = get_dict_number_safe(bounds_dict, "Height").unwrap_or(0.0);
        format!("x:{}, y:{}, w:{}, h:{}", x, y, width, height)
    } else {
        "bounds_not_found".to_string()
    }
}

fn get_dict_value(dict: *const c_void, key: &str) -> Option<*const c_void> {
    unsafe {
        let key_cstring = CString::new(key).ok()?;
        let cf_key =
            CFStringCreateWithCString(ptr::null(), key_cstring.as_ptr(), K_CF_STRING_ENCODING_UTF8);

        if cf_key.is_null() {
            return None;
        }

        let cf_value = CFDictionaryGetValue(dict, cf_key);
        CFRelease(cf_key);

        if cf_value.is_null() {
            None
        } else {
            Some(cf_value)
        }
    }
}

fn should_ignore_app(app_name: &str, ignored_apps: &HashSet<String>) -> bool {
    let app_lower = app_name.to_lowercase();
    ignored_apps
        .iter()
        .any(|ignored| app_lower.contains(ignored))
}

fn get_dict_string_safe(dict: *const c_void, key: &str) -> Option<String> {
    unsafe {
        let key_cstring = CString::new(key).ok()?;
        let cf_key =
            CFStringCreateWithCString(ptr::null(), key_cstring.as_ptr(), K_CF_STRING_ENCODING_UTF8);

        if cf_key.is_null() {
            return None;
        }

        let cf_value = CFDictionaryGetValue(dict, cf_key);
        CFRelease(cf_key);

        if cf_value.is_null() {
            return None;
        }

        let length = CFStringGetLength(cf_value);
        if length == 0 {
            return Some(String::new());
        }

        let mut buffer = vec![0u8; (length * 4 + 1) as usize];
        let success = CFStringGetCString(
            cf_value,
            buffer.as_mut_ptr() as *mut c_char,
            buffer.len() as isize,
            K_CF_STRING_ENCODING_UTF8,
        );

        if success {
            let c_str = CStr::from_ptr(buffer.as_ptr() as *const c_char);
            Some(c_str.to_string_lossy().into_owned())
        } else {
            None
        }
    }
}

fn get_dict_number_safe(dict: *const c_void, key: &str) -> Option<f64> {
    unsafe {
        let key_cstring = CString::new(key).ok()?;
        let cf_key =
            CFStringCreateWithCString(ptr::null(), key_cstring.as_ptr(), K_CF_STRING_ENCODING_UTF8);

        if cf_key.is_null() {
            return None;
        }

        let cf_value = CFDictionaryGetValue(dict, cf_key);
        CFRelease(cf_key);

        if cf_value.is_null() {
            return None;
        }

        let mut value: f64 = 0.0;
        let success = CFNumberGetValue(
            cf_value,
            K_CF_NUMBER_DOUBLE_TYPE,
            &mut value as *mut f64 as *mut c_void,
        );

        if success {
            Some(value)
        } else {
            None
        }
    }
}
