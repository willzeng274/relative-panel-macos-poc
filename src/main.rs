mod window_search;

use std::collections::{HashMap, HashSet};
use std::cell::RefCell;
use std::ptr::NonNull;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSButton, NSPanel, NSScreen,
    NSView, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString, NSTimer};

use window_search::{find_windows, WindowSearchCriteria};

const POLL_INTERVAL_SECONDS: f64 = 5.0;

struct PanelManager {
    panels: RefCell<HashMap<i64, Retained<NSPanel>>>,
    search_criteria: WindowSearchCriteria,
}

impl PanelManager {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            panels: RefCell::new(HashMap::new()),
            search_criteria: WindowSearchCriteria::new()
                .with_title("Open")
                .with_ignored_apps(get_ignored_apps()),
        })
    }

    fn check_for_windows(&self) {
        println!("\n[POLL] Searching for windows with title 'Open'...");

        match find_windows(&self.search_criteria) {
            Ok(results) => {
                println!("[POLL] Scanned {} total windows", results.total_windows);

                let current_window_numbers: HashSet<i64> = results
                    .matched_windows
                    .iter()
                    .map(|w| w.window_number)
                    .collect();

                let mut panels = self.panels.borrow_mut();

                panels.retain(|window_number, panel| {
                    let keep = current_window_numbers.contains(window_number);
                    if !keep {
                        println!("[POLL] Removing panel for closed window {}", window_number);
                        panel.orderOut(None);
                    }
                    keep
                });

                for window in &results.matched_windows {
                    if !panels.contains_key(&window.window_number) {
                        println!("\n[POLL] NEW WINDOW DETECTED:");
                        println!("  '{}' from {}", window.title, window.app_name);
                        println!("     App Name: {}", window.app_name);
                        println!("     Bundle ID: {}", window.bundle_identifier.as_ref().unwrap_or(&"N/A".to_string()));
                        println!("     Bounds: {}", window.bounds);
                        println!("     Window Number: {}", window.window_number);
                        println!("     PID: {}", window.pid);
                        println!("     Layer: {}", window.layer);
                        println!("     Alpha: {}", window.alpha);
                        println!("     Sharing State: {}", window.sharing_state);
                        println!("     Memory Usage: {} bytes", window.memory_usage);
                        println!("     Is Onscreen: {}", window.is_onscreen);

                        if let Some(panel) = create_overlay_panel(&window) {
                            panels.insert(window.window_number, panel);
                            println!("     ✓ Created overlay panel");
                        } else {
                            println!("     ✗ Failed to create overlay panel");
                        }
                    }
                }

                println!("[POLL] Currently tracking {} panels", panels.len());
            }
            Err(e) => {
                println!("[POLL] Error: {}", e);
            }
        }
    }
}

fn main() {
    let mtm = MainThreadMarker::new().unwrap();
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    println!("Starting panel detector with {} second polling interval...", POLL_INTERVAL_SECONDS);

    let manager = PanelManager::new();
    let manager_clone = Rc::clone(&manager);

    manager.check_for_windows();

    unsafe {
        NSTimer::scheduledTimerWithTimeInterval_repeats_block(
            POLL_INTERVAL_SECONDS,
            true,
            &block2::RcBlock::new(move |_timer: NonNull<NSTimer>| {
                manager_clone.check_for_windows();
            }),
        );
    }

    println!("Starting NSApplication run loop...");
    app.run();
}

fn create_overlay_panel(window: &window_search::WindowInfo) -> Option<Retained<NSPanel>> {
    println!("Creating NSPanel overlay for {} window...", window.app_name);

    let (cg_x, cg_y, orig_width, orig_height) = parse_bounds_values(&window.bounds)?;

    unsafe {
        let mtm = MainThreadMarker::new().unwrap();
        let main_screen = NSScreen::mainScreen(mtm).unwrap();
        let screen_frame = main_screen.frame();
        let screen_height = screen_frame.size.height;
        let ns_y = screen_height - cg_y - orig_height;

        println!(" Screen height: {}", screen_height);

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

        let style_mask = NSWindowStyleMask::Borderless;

        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            panel_frame,
            style_mask,
            NSBackingStoreType::Buffered,
            false,
        );

        panel.setLevel(10);
        panel.setOpaque(false);
        panel.setAlphaValue(0.9);
        panel.setHasShadow(true);
        panel.setMovableByWindowBackground(true);
        panel.setHidesOnDeactivate(false);
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Stationary,
        );

        let window_title = NSString::from_str("PANEL DETECTOR OVERLAY");
        panel.setTitle(&window_title);

        let content_view = NSView::initWithFrame(
            NSView::alloc(mtm),
            NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(panel_width, panel_height),
            ),
        );

        panel.setContentView(Some(&content_view));

        let button_width = panel_width * 0.8;
        let button_height = panel_height * 0.3;
        let button_x = (panel_width - button_width) / 2.0;
        let button_y = (panel_height - button_height) / 2.0;

        let button_frame = NSRect::new(
            NSPoint::new(button_x, button_y),
            NSSize::new(button_width, button_height),
        );

        let button = NSButton::initWithFrame(NSButton::alloc(mtm), button_frame);

        let title_str = format!("PANEL DETECTED: {}", window.app_name);
        let title = NSString::from_str(&title_str);
        button.setTitle(&title);

        content_view.addSubview(&button);

        let close_button_size = 30.0;
        let close_button_margin = 10.0;
        let close_button_frame = NSRect::new(
            NSPoint::new(
                panel_width - close_button_size - close_button_margin,
                panel_height - close_button_size - close_button_margin,
            ),
            NSSize::new(close_button_size, close_button_size),
        );

        let close_button = NSButton::initWithFrame(NSButton::alloc(mtm), close_button_frame);
        let close_title = NSString::from_str("✕");
        close_button.setTitle(&close_title);
        close_button.setTarget(Some(&panel));
        close_button.setAction(Some(objc2::sel!(orderOut:)));

        content_view.addSubview(&close_button);

        panel.makeKeyAndOrderFront(None);
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
