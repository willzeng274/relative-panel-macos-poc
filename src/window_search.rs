use std::collections::HashSet;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::ptr;

use objc2_app_kit::NSRunningApplication;

const K_CF_STRING_ENCODING_UTF8: u32 = 0x08000100;
const K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY: u32 = 1;
const K_CF_NUMBER_DOUBLE_TYPE: i32 = 13;

#[link(name = "CoreGraphics", kind = "framework")]
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

#[derive(Debug, Clone)]
pub struct WindowInfo {
    pub title: String,
    pub app_name: String,
    pub bundle_identifier: Option<String>,
    pub bounds: String,
    pub window_number: i64,
    pub pid: i32,
    pub layer: i32,
    pub alpha: f64,
    pub sharing_state: i32,
    pub memory_usage: i64,
    pub is_onscreen: bool,
}

#[derive(Debug)]
pub struct WindowSearchResults {
    pub total_windows: usize,
    pub matched_windows: Vec<WindowInfo>,
}

#[derive(Debug, Clone)]
pub struct WindowSearchCriteria {
    title: Option<String>,
    app_name: Option<String>,
    ignored_apps: HashSet<String>,
}

impl WindowSearchCriteria {
    pub fn new() -> Self {
        Self {
            title: None,
            app_name: None,
            ignored_apps: HashSet::new(),
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_app_name(mut self, app_name: impl Into<String>) -> Self {
        self.app_name = Some(app_name.into());
        self
    }

    pub fn with_ignored_apps(mut self, ignored_apps: HashSet<String>) -> Self {
        self.ignored_apps = ignored_apps;
        self
    }

    pub fn add_ignored_app(mut self, app: impl Into<String>) -> Self {
        self.ignored_apps.insert(app.into().to_lowercase());
        self
    }

    fn matches(&self, window_title: &str, window_app_name: &str) -> bool {
        if should_ignore_app(window_app_name, &self.ignored_apps) {
            return false;
        }

        let title_matches = self
            .title
            .as_ref()
            .map(|t| window_title == t)
            .unwrap_or(true);

        let app_name_matches = self
            .app_name
            .as_ref()
            .map(|a| window_app_name == a)
            .unwrap_or(true);

        title_matches && app_name_matches
    }
}

impl Default for WindowSearchCriteria {
    fn default() -> Self {
        Self::new()
    }
}

pub fn find_windows(criteria: &WindowSearchCriteria) -> Result<WindowSearchResults, String> {
    unsafe {
        let window_list = CGWindowListCopyWindowInfo(K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY, 0);
        if window_list.is_null() {
            return Err("Failed to get window list".to_string());
        }

        let count = CFArrayGetCount(window_list);
        let mut matched_windows = Vec::new();
        let mut total_processed = 0;

        for i in 0..count {
            let window_dict = CFArrayGetValueAtIndex(window_list, i);
            if window_dict.is_null() {
                continue;
            }

            let app_name =
                get_dict_string_safe(window_dict, "kCGWindowOwnerName").unwrap_or_default();

            if should_ignore_app(&app_name, &criteria.ignored_apps) {
                continue;
            }

            total_processed += 1;

            let title = get_dict_string_safe(window_dict, "kCGWindowName").unwrap_or_default();

            if !criteria.matches(&title, &app_name) {
                continue;
            }

            let bounds = parse_bounds_from_dict(window_dict);
            let window_number =
                get_dict_number_safe(window_dict, "kCGWindowNumber").unwrap_or(0.0) as i64;
            let pid = get_dict_number_safe(window_dict, "kCGWindowOwnerPID").unwrap_or(0.0) as i32;
            let layer = get_dict_number_safe(window_dict, "kCGWindowLayer").unwrap_or(0.0) as i32;
            let alpha = get_dict_number_safe(window_dict, "kCGWindowAlpha").unwrap_or(1.0);
            let sharing_state =
                get_dict_number_safe(window_dict, "kCGWindowSharingState").unwrap_or(0.0) as i32;
            let memory_usage =
                get_dict_number_safe(window_dict, "kCGWindowMemoryUsage").unwrap_or(0.0) as i64;
            let is_onscreen =
                get_dict_number_safe(window_dict, "kCGWindowIsOnscreen").unwrap_or(0.0) != 0.0;

            let bundle_identifier = get_bundle_identifier(pid);

            matched_windows.push(WindowInfo {
                title,
                app_name: app_name.clone(),
                bundle_identifier,
                bounds,
                window_number,
                pid,
                layer,
                alpha,
                sharing_state,
                memory_usage,
                is_onscreen,
            });
        }

        CFRelease(window_list);

        Ok(WindowSearchResults {
            total_windows: total_processed,
            matched_windows,
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

fn get_bundle_identifier(pid: i32) -> Option<String> {
    let app = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)?;
    app.bundleIdentifier().map(|id| id.to_string())
}
