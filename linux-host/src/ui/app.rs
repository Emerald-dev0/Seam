use std::ffi::{c_char, c_double, c_int, CString};
use std::ptr;

use crate::config::HostConfig;
use crate::diagnostics::run_self_test;
use crate::protocol::wire::EdgeSide;
use crate::transport::adb::{list_devices, locate_adb};

type GtkWidget = libc::c_void;
type GtkWindow = libc::c_void;

unsafe extern "C" {
    fn gtk_init_check(argc: *mut c_int, argv: *mut *mut *mut c_char) -> c_int;
    fn gtk_window_new(type_: c_int) -> *mut GtkWidget;
    fn gtk_window_set_title(window: *mut GtkWindow, title: *const c_char);
    fn gtk_window_set_default_size(window: *mut GtkWindow, width: c_int, height: c_int);
    fn gtk_window_set_position(window: *mut GtkWindow, position: c_int);
    fn gtk_box_new(orientation: c_int, spacing: c_int) -> *mut GtkWidget;
    fn gtk_label_new(str_: *const c_char) -> *mut GtkWidget;
    fn gtk_button_new_with_label(label: *const c_char) -> *mut GtkWidget;
    fn gtk_combo_box_text_new() -> *mut GtkWidget;
    fn gtk_combo_box_text_append_text(combo: *mut GtkWidget, text: *const c_char);
    fn gtk_combo_box_set_active(combo: *mut GtkWidget, index_: c_int);
    fn gtk_scale_new_with_range(orientation: c_int, min: c_double, max: c_double, step: c_double) -> *mut GtkWidget;
    fn gtk_range_set_value(range: *mut GtkWidget, value: c_double);
    fn gtk_container_add(container: *mut GtkWidget, widget: *mut GtkWidget);
    fn gtk_box_pack_start(box_: *mut GtkWidget, child: *mut GtkWidget, expand: c_int, fill: c_int, padding: c_int);
    fn gtk_widget_show_all(widget: *mut GtkWidget);
    fn gtk_main();
    fn gtk_main_quit();
    fn g_signal_connect_data(
        instance: *mut libc::c_void,
        detailed_signal: *const c_char,
        c_handler: unsafe extern "C" fn(),
        data: *mut libc::c_void,
        destroy_data: *mut libc::c_void,
        connect_flags: c_int,
    ) -> libc::c_ulong;
}

unsafe extern "C" fn on_destroy() {
    gtk_main_quit();
}

pub fn launch_gtk_app() {
    unsafe {
        let initialized = gtk_init_check(ptr::null_mut(), ptr::null_mut());
        if initialized == 0 {
            eprintln!("[UI] Could not initialize GTK3 (no active X11 display session detected).");
            eprintln!("To run SEAM in headless or terminal mode, use:");
            eprintln!("  seam connect             # Start screen-edge crossing in terminal");
            eprintln!("  seam devices             # List connected devices");
            eprintln!("  seam self-test           # Verify system setup");
            return;
        }

        let config = HostConfig::load();
        let report = run_self_test();

        let title = CString::new("SEAM Control Center (Linux Mint)").unwrap();
        let win = gtk_window_new(0); // GTK_WINDOW_TOPLEVEL
        gtk_window_set_title(win as *mut GtkWindow, title.as_ptr());
        gtk_window_set_default_size(win as *mut GtkWindow, 460, 480);
        gtk_window_set_position(win as *mut GtkWindow, 1); // GTK_WIN_POS_CENTER

        let destroy_sig = CString::new("destroy").unwrap();
        g_signal_connect_data(
            win as *mut libc::c_void,
            destroy_sig.as_ptr(),
            on_destroy,
            ptr::null_mut(),
            ptr::null_mut(),
            0,
        );

        // Main Vertical Container (GTK_ORIENTATION_VERTICAL = 1)
        let main_box = gtk_box_new(1, 12);
        gtk_container_add(win, main_box);

        // Header Title
        let header_str = CString::new("SEAM Universal Control").unwrap();
        let header_label = gtk_label_new(header_str.as_ptr());
        gtk_box_pack_start(main_box, header_label, 0, 0, 8);

        // Status Card
        let status_text = format!(
            "Status: Ready | Devices: {} | X11: {}",
            report.device_count,
            if report.x11_available { "Connected" } else { "Unavailable" }
        );
        let status_cstr = CString::new(status_text).unwrap();
        let status_label = gtk_label_new(status_cstr.as_ptr());
        gtk_box_pack_start(main_box, status_label, 0, 0, 4);

        // Device Selector Label & Combo
        let dev_label_cstr = CString::new("Connected Android Device:").unwrap();
        let dev_label = gtk_label_new(dev_label_cstr.as_ptr());
        gtk_box_pack_start(main_box, dev_label, 0, 0, 2);

        let dev_combo = gtk_combo_box_text_new();
        let adb = locate_adb();
        let devices = list_devices(&adb).unwrap_or_default();
        if devices.is_empty() {
            let none_cstr = CString::new("No devices found (Connect USB / Wi-Fi Debugging)").unwrap();
            gtk_combo_box_text_append_text(dev_combo, none_cstr.as_ptr());
        } else {
            for d in &devices {
                let entry = format!("{} ({})", d.serial, d.model);
                let entry_cstr = CString::new(entry).unwrap();
                gtk_combo_box_text_append_text(dev_combo, entry_cstr.as_ptr());
            }
        }
        gtk_combo_box_set_active(dev_combo, 0);
        gtk_box_pack_start(main_box, dev_combo, 0, 0, 4);

        // Active Edge Boundary Selector
        let edge_label_cstr = CString::new("Screen Boundary Edge (Where phone sits):").unwrap();
        let edge_label = gtk_label_new(edge_label_cstr.as_ptr());
        gtk_box_pack_start(main_box, edge_label, 0, 0, 2);

        let edge_combo = gtk_combo_box_text_new();
        for &edge_name in &["Right Edge (Default)", "Left Edge", "Top Edge", "Bottom Edge"] {
            let cstr = CString::new(edge_name).unwrap();
            gtk_combo_box_text_append_text(edge_combo, cstr.as_ptr());
        }
        let active_idx = match config.active_edge {
            EdgeSide::Right => 0,
            EdgeSide::Left => 1,
            EdgeSide::Top => 2,
            EdgeSide::Bottom => 3,
        };
        gtk_combo_box_set_active(edge_combo, active_idx);
        gtk_box_pack_start(main_box, edge_combo, 0, 0, 4);

        // Velocity Sensitivity Slider
        let sens_label_cstr = CString::new("Pointer Sensitivity Gain:").unwrap();
        let sens_label = gtk_label_new(sens_label_cstr.as_ptr());
        gtk_box_pack_start(main_box, sens_label, 0, 0, 2);

        // GTK_ORIENTATION_HORIZONTAL = 0
        let scale = gtk_scale_new_with_range(0, 0.5, 3.5, 0.1);
        gtk_range_set_value(scale, config.velocity_gain);
        gtk_box_pack_start(main_box, scale, 0, 0, 4);

        // Action Buttons Box
        let btn_box = gtk_box_new(0, 10);
        let start_cstr = CString::new("Start SEAM Connection").unwrap();
        let start_btn = gtk_button_new_with_label(start_cstr.as_ptr());
        gtk_box_pack_start(btn_box, start_btn, 1, 1, 4);

        let panic_cstr = CString::new("Panic Return (Esc)").unwrap();
        let panic_btn = gtk_button_new_with_label(panic_cstr.as_ptr());
        gtk_box_pack_start(btn_box, panic_btn, 1, 1, 4);

        gtk_box_pack_start(main_box, btn_box, 0, 0, 12);

        gtk_widget_show_all(win);
        gtk_main();
    }
}
