use anyhow::{anyhow, Context, Result};
use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::TryRecvError;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const APP_ID_MAIN: &str = "dev.jiansyuan.Tautomorphism";
const APP_ID_CONFIG: &str = "dev.jiansyuan.Tautomorphism.Config";
const PRODUCT_NAME: &str = "Tautomorphism";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct AppConfig {
    engine: Engine,
    source_lang: String,
    target_lang: String,
    baidu_target_lang: String,
    google_api_key: String,
    baidu_appid: String,
    baidu_secret: String,
    copy_delay_ms: u64,
    max_chars: usize,
    auto_close_seconds: u32,
    close_on_mouse_leave: bool,
    close_on_mouse_move: bool,
    close_when_pointer_away: bool,
    pointer_away_radius: i32,
    close_on_focus_lost: bool,
    restore_clipboard: bool,
    selectable_text: bool,
    auto_copy_result: bool,
    timeout_seconds: u64,
    force_floating: bool,
    popup_no_focus: bool,
    popup_position: PopupPosition,
    pointer_offset_x: i32,
    pointer_offset_y: i32,
    popup_min_width: i32,
    popup_max_width: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum Engine {
    GoogleFree,
    GoogleCloud,
    Baidu,
}

impl Default for Engine {
    fn default() -> Self {
        Self::GoogleFree
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum PopupPosition {
    NearPointer,
    Center,
    None,
}

impl Default for PopupPosition {
    fn default() -> Self {
        Self::NearPointer
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            engine: Engine::GoogleFree,
            source_lang: "auto".to_string(),
            target_lang: "zh-CN".to_string(),
            baidu_target_lang: "zh".to_string(),
            google_api_key: String::new(),
            baidu_appid: String::new(),
            baidu_secret: String::new(),
            copy_delay_ms: 120,
            max_chars: 800,
            auto_close_seconds: 8,
            close_on_mouse_leave: true,
            close_on_mouse_move: false,
            close_when_pointer_away: true,
            pointer_away_radius: 240,
            close_on_focus_lost: false,
            restore_clipboard: true,
            selectable_text: true,
            auto_copy_result: false,
            timeout_seconds: 8,
            force_floating: true,
            popup_no_focus: true,
            popup_position: PopupPosition::NearPointer,
            pointer_offset_x: 18,
            pointer_offset_y: 18,
            popup_min_width: 90,
            popup_max_width: 560,
        }
    }
}

pub fn run_popup() {
    let app = gtk::Application::builder()
        .application_id(APP_ID_MAIN)
        .build();

    app.connect_activate(|app| build_popup_flow(app));
    app.run();
}

pub fn run_config() {
    let app = gtk::Application::builder()
        .application_id(APP_ID_CONFIG)
        .build();

    app.connect_activate(|app| build_settings_ui(app));
    app.run();
}

fn config_path() -> Result<PathBuf> {
    let dir = dirs::config_dir()
        .ok_or_else(|| anyhow!("cannot find XDG config dir"))?
        .join("tautomorphism");
    fs::create_dir_all(&dir)?;
    Ok(dir.join("config.json"))
}

fn legacy_config_paths() -> Vec<PathBuf> {
    match dirs::config_dir() {
        Some(dir) => vec![
            dir.join("tautologist-ran").join("config.json"),
            dir.join("seltrans").join("config.json"),
        ],
        None => Vec::new(),
    }
}

fn load_config() -> AppConfig {
    if let Ok(path) = config_path() {
        if let Ok(text) = fs::read_to_string(&path) {
            if let Ok(config) = serde_json::from_str::<AppConfig>(&text) {
                return config;
            }
        }
    }

    for path in legacy_config_paths() {
        if let Ok(text) = fs::read_to_string(path) {
            if let Ok(config) = serde_json::from_str::<AppConfig>(&text) {
                let _ = save_config(&config);
                return config;
            }
        }
    }

    AppConfig::default()
}

fn save_config(config: &AppConfig) -> Result<()> {
    let path = config_path()?;
    let text = serde_json::to_string_pretty(config)?;
    fs::write(path, text)?;
    Ok(())
}

fn build_popup_flow(app: &gtk::Application) {
    let mut app_hold = Some(app.hold());

    let config = load_config();
    let old_clipboard = if config.restore_clipboard { read_clipboard().ok() } else { None };

    let selected_text = match acquire_selected_text(&config) {
        Ok(text) if !text.trim().is_empty() => normalize_clip(text, config.max_chars),
        Ok(_) => "No selected text".to_string(),
        Err(err) => format!("Selection error: {err}"),
    };

    if config.restore_clipboard {
        if let Some(old) = old_clipboard {
            let _ = set_clipboard(&old);
        }
    }

    let (sender, receiver) = std::sync::mpsc::channel::<Result<String, String>>();
    let text_for_thread = selected_text.clone();
    let config_for_thread = config.clone();
    thread::spawn(move || {
        let output = translate(&text_for_thread, &config_for_thread).map_err(|e| e.to_string());
        let _ = sender.send(output);
    });

    let app_for_result = app.clone();
    let config_for_result = config.clone();
    glib::timeout_add_local(Duration::from_millis(20), move || match receiver.try_recv() {
        Ok(message) => {
            let text = match message {
                Ok(text) => text,
                Err(err) => format!("Translation failed: {err}"),
            };
            if config_for_result.auto_copy_result {
                let _ = set_clipboard(&text);
            }
            show_result_popup(&app_for_result, &config_for_result, &text);
            drop(app_hold.take());
            glib::ControlFlow::Break
        }
        Err(TryRecvError::Empty) => glib::ControlFlow::Continue,
        Err(TryRecvError::Disconnected) => {
            show_result_popup(&app_for_result, &config_for_result, "Translation worker exited");
            drop(app_hold.take());
            glib::ControlFlow::Break
        }
    });
}

fn show_result_popup(app: &gtk::Application, config: &AppConfig, text: &str) {
    let width = popup_width_for_text(text, config);
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title(PRODUCT_NAME)
        .default_width(width)
        .resizable(false)
        .decorated(false)
        .build();

    window.set_focusable(!config.popup_no_focus);
    window.set_can_focus(!config.popup_no_focus);

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.set_margin_top(7);
    root.set_margin_bottom(7);
    root.set_margin_start(9);
    root.set_margin_end(9);
    root.set_size_request(width, -1);

    let label = gtk::Label::new(Some(text.trim()));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_max_width_chars(((width - 18) / 8).clamp(8, 140));
    label.set_selectable(config.selectable_text);
    label.set_hexpand(false);
    root.append(&label);

    window.set_child(Some(&root));

    let key = gtk::EventControllerKey::new();
    let app_for_escape = app.clone();
    key.connect_key_pressed(move |_, keyval, _, _| {
        if keyval == gtk::gdk::Key::Escape {
            app_for_escape.quit();
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    window.add_controller(key);

    install_close_behaviour(&window, app, config);
    install_auto_close(app, config.auto_close_seconds);
    install_pointer_away_close(app, config);

    if config.popup_no_focus {
        window.set_visible(true);
    } else {
        window.present();
    }

    schedule_hyprland_popup_placement(config.clone());
}

fn popup_width_for_text(text: &str, config: &AppConfig) -> i32 {
    let min_width = config.popup_min_width.clamp(60, 600);
    let max_width = config.popup_max_width.clamp(min_width, 1400);
    let mut max_line_px = 0;

    for line in text.lines() {
        let mut px = 0;
        for ch in line.chars().take(120) {
            px += if ch.is_ascii() { 8 } else { 15 };
        }
        max_line_px = max_line_px.max(px);
    }

    if max_line_px == 0 {
        return min_width;
    }

    (max_line_px + 32).clamp(min_width, max_width)
}

fn install_close_behaviour(
    window: &gtk::ApplicationWindow,
    app: &gtk::Application,
    config: &AppConfig,
) {
    if config.close_on_mouse_leave || config.close_on_mouse_move {
        let motion = gtk::EventControllerMotion::new();

        if config.close_on_mouse_leave {
            let app_for_leave = app.clone();
            motion.connect_leave(move |_| app_for_leave.quit());
        }

        if config.close_on_mouse_move {
            let app_for_motion = app.clone();
            motion.connect_motion(move |_, _, _| app_for_motion.quit());
        }

        window.add_controller(motion);
    }

    if config.close_on_focus_lost {
        let focus = gtk::EventControllerFocus::new();
        let app_for_focus = app.clone();
        focus.connect_leave(move |_| app_for_focus.quit());
        window.add_controller(focus);
    }
}

fn install_auto_close(app: &gtk::Application, seconds: u32) {
    if seconds == 0 {
        return;
    }
    let app_for_timer = app.clone();
    glib::timeout_add_local_once(Duration::from_secs(seconds as u64), move || app_for_timer.quit());
}

fn install_pointer_away_close(app: &gtk::Application, config: &AppConfig) {
    if !config.close_when_pointer_away {
        return;
    }

    let Some((origin_x, origin_y)) = hyprland_cursor_pos() else {
        return;
    };

    let radius = config.pointer_away_radius.max(32);
    let app_for_poll = app.clone();
    glib::timeout_add_local(Duration::from_millis(120), move || {
        if let Some((x, y)) = hyprland_cursor_pos() {
            let dx = x - origin_x;
            let dy = y - origin_y;
            if dx * dx + dy * dy > radius * radius {
                app_for_poll.quit();
                return glib::ControlFlow::Break;
            }
        }
        glib::ControlFlow::Continue
    });
}

fn schedule_hyprland_popup_placement(config: AppConfig) {
    for delay in [25_u64, 70, 140, 260] {
        let config_for_hypr = config.clone();
        glib::timeout_add_local_once(Duration::from_millis(delay), move || {
            apply_hyprland_popup_rules(&config_for_hypr);
        });
    }
}

fn apply_hyprland_popup_rules(config: &AppConfig) {
    let selector = own_hypr_window_selector().unwrap_or_else(|| "activewindow".to_string());

    if config.force_floating || config.popup_position != PopupPosition::None {
        run_hypr_dispatch("setfloating", &selector);
    }

    match config.popup_position {
        PopupPosition::NearPointer => {
            if let Some((x, y)) = hyprland_cursor_pos() {
                let x = (x + config.pointer_offset_x).max(0);
                let y = (y + config.pointer_offset_y).max(0);
                let arg = format!("exact {x} {y},{selector}");
                run_hypr_dispatch("movewindowpixel", &arg);
            }
        }
        PopupPosition::Center => run_hypr_dispatch("centerwindow", &selector),
        PopupPosition::None => {}
    }
}

fn run_hypr_dispatch(dispatcher: &str, arg: &str) {
    let _ = Command::new("hyprctl")
        .args(["dispatch", dispatcher, arg])
        .status();
}

fn own_hypr_window_selector() -> Option<String> {
    let output = Command::new("hyprctl").args(["clients", "-j"]).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let value: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let clients = value.as_array()?;
    let pid = std::process::id() as u64;
    let mut fallback: Option<String> = None;

    for client in clients {
        if client.get("pid").and_then(|v| v.as_u64()) != Some(pid) {
            continue;
        }

        let address = client.get("address").and_then(|v| v.as_str())?;
        let selector = format!("address:{address}");
        let class = client.get("class").and_then(|v| v.as_str()).unwrap_or_default();

        if class == APP_ID_MAIN || class.contains("Tautomorphism") {
            return Some(selector);
        }

        if fallback.is_none() {
            fallback = Some(selector);
        }
    }

    fallback
}

fn hyprland_cursor_pos() -> Option<(i32, i32)> {
    let output = Command::new("hyprctl").arg("cursorpos").output().ok()?;
    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let cleaned = text.trim().replace(',', " ");
    let mut parts = cleaned.split_whitespace();
    let x = parts.next()?.parse::<i32>().ok()?;
    let y = parts.next()?.parse::<i32>().ok()?;
    Some((x, y))
}

fn build_settings_ui(app: &gtk::Application) {
    let config = load_config();

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Tautomorphism Settings")
        .default_width(660)
        .default_height(680)
        .resizable(true)
        .decorated(true)
        .build();

    let outer = gtk::Box::new(gtk::Orientation::Vertical, 10);
    outer.set_margin_top(12);
    outer.set_margin_bottom(12);
    outer.set_margin_start(12);
    outer.set_margin_end(12);

    let scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .hexpand(true)
        .build();
    let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
    scroll.set_child(Some(&root));
    outer.append(&scroll);

    let translation_box = add_section(&root, "Translation");
    let engine = gtk::ComboBoxText::new();
    engine.append(Some("google_free"), "Google Free");
    engine.append(Some("google_cloud"), "Google Cloud");
    engine.append(Some("baidu"), "Baidu");
    engine.set_active_id(Some(match config.engine {
        Engine::GoogleFree => "google_free",
        Engine::GoogleCloud => "google_cloud",
        Engine::Baidu => "baidu",
    }));
    add_row(&translation_box, "Engine", &engine);

    let source_lang = gtk::Entry::builder().text(&config.source_lang).build();
    add_row(&translation_box, "From", &source_lang);

    let target_lang = gtk::Entry::builder().text(&config.target_lang).build();
    add_row(&translation_box, "To", &target_lang);

    let baidu_target = gtk::Entry::builder().text(&config.baidu_target_lang).build();
    add_row(&translation_box, "Baidu to", &baidu_target);

    let api_box = add_section(&root, "API");
    let google_key = gtk::Entry::builder().text(&config.google_api_key).build();
    google_key.set_visibility(false);
    add_row(&api_box, "Google key", &google_key);

    let baidu_appid = gtk::Entry::builder().text(&config.baidu_appid).build();
    add_row(&api_box, "Baidu appid", &baidu_appid);

    let baidu_secret = gtk::Entry::builder().text(&config.baidu_secret).build();
    baidu_secret.set_visibility(false);
    add_row(&api_box, "Baidu secret", &baidu_secret);

    let selection_box = add_section(&root, "Selection");
    let copy_delay = spin(0.0, 1000.0, 10.0, config.copy_delay_ms as f64);
    add_row(&selection_box, "Copy delay", &copy_delay);

    let max_chars = spin(80.0, 5000.0, 20.0, config.max_chars as f64);
    add_row(&selection_box, "Max chars", &max_chars);

    let restore_clipboard = gtk::CheckButton::with_label("Restore clipboard");
    restore_clipboard.set_active(config.restore_clipboard);
    selection_box.append(&restore_clipboard);

    let popup_box = add_section(&root, "Popup");
    let popup_position = gtk::ComboBoxText::new();
    popup_position.append(Some("near_pointer"), "Near pointer");
    popup_position.append(Some("center"), "Center");
    popup_position.append(Some("none"), "Do not move");
    popup_position.set_active_id(Some(match config.popup_position {
        PopupPosition::NearPointer => "near_pointer",
        PopupPosition::Center => "center",
        PopupPosition::None => "none",
    }));
    add_row(&popup_box, "Position", &popup_position);

    let offset_x = spin(-300.0, 300.0, 1.0, config.pointer_offset_x as f64);
    add_row(&popup_box, "Offset X", &offset_x);

    let offset_y = spin(-300.0, 300.0, 1.0, config.pointer_offset_y as f64);
    add_row(&popup_box, "Offset Y", &offset_y);

    let min_width = spin(60.0, 600.0, 10.0, config.popup_min_width as f64);
    add_row(&popup_box, "Min width", &min_width);

    let max_width = spin(120.0, 1400.0, 10.0, config.popup_max_width as f64);
    add_row(&popup_box, "Max width", &max_width);

    let auto_close = spin(0.0, 120.0, 1.0, config.auto_close_seconds as f64);
    add_row(&popup_box, "Auto-close", &auto_close);

    let force_floating = gtk::CheckButton::with_label("Floating popup");
    force_floating.set_active(config.force_floating);
    popup_box.append(&force_floating);

    let popup_no_focus = gtk::CheckButton::with_label("Do not focus popup");
    popup_no_focus.set_active(config.popup_no_focus);
    popup_box.append(&popup_no_focus);

    let selectable = gtk::CheckButton::with_label("Selectable result");
    selectable.set_active(config.selectable_text);
    popup_box.append(&selectable);

    let auto_copy = gtk::CheckButton::with_label("Copy result");
    auto_copy.set_active(config.auto_copy_result);
    popup_box.append(&auto_copy);

    let close_box = add_section(&root, "Close");
    let close_leave = gtk::CheckButton::with_label("Mouse leaves popup");
    close_leave.set_active(config.close_on_mouse_leave);
    close_box.append(&close_leave);

    let close_move = gtk::CheckButton::with_label("Mouse moves inside popup");
    close_move.set_active(config.close_on_mouse_move);
    close_box.append(&close_move);

    let close_pointer_away = gtk::CheckButton::with_label("Pointer moves away");
    close_pointer_away.set_active(config.close_when_pointer_away);
    close_box.append(&close_pointer_away);

    let pointer_radius = spin(32.0, 1200.0, 10.0, config.pointer_away_radius as f64);
    add_row(&close_box, "Away radius", &pointer_radius);

    let close_focus = gtk::CheckButton::with_label("Focus lost");
    close_focus.set_active(config.close_on_focus_lost);
    close_box.append(&close_focus);

    let network_box = add_section(&root, "Network");
    let timeout = spin(2.0, 60.0, 1.0, config.timeout_seconds as f64);
    add_row(&network_box, "Timeout", &timeout);

    let bottom = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    bottom.set_halign(gtk::Align::Fill);

    let status = gtk::Label::new(None);
    status.set_xalign(0.0);
    status.set_hexpand(true);

    let reset = gtk::Button::with_label("Reset");
    let close = gtk::Button::with_label("Close");
    let save = gtk::Button::with_label("Save");
    save.add_css_class("suggested-action");

    let status_for_save = status.clone();
    save.connect_clicked(move |_| {
        let min_width_value = min_width.value() as i32;
        let max_width_value = (max_width.value() as i32).max(min_width_value);

        let new_config = AppConfig {
            engine: match engine.active_id().as_deref() {
                Some("google_cloud") => Engine::GoogleCloud,
                Some("baidu") => Engine::Baidu,
                _ => Engine::GoogleFree,
            },
            source_lang: source_lang.text().trim().to_string(),
            target_lang: target_lang.text().trim().to_string(),
            baidu_target_lang: baidu_target.text().trim().to_string(),
            google_api_key: google_key.text().to_string(),
            baidu_appid: baidu_appid.text().to_string(),
            baidu_secret: baidu_secret.text().to_string(),
            copy_delay_ms: copy_delay.value() as u64,
            max_chars: max_chars.value() as usize,
            auto_close_seconds: auto_close.value() as u32,
            close_on_mouse_leave: close_leave.is_active(),
            close_on_mouse_move: close_move.is_active(),
            close_when_pointer_away: close_pointer_away.is_active(),
            pointer_away_radius: pointer_radius.value() as i32,
            close_on_focus_lost: close_focus.is_active(),
            restore_clipboard: restore_clipboard.is_active(),
            selectable_text: selectable.is_active(),
            auto_copy_result: auto_copy.is_active(),
            timeout_seconds: timeout.value() as u64,
            force_floating: force_floating.is_active(),
            popup_no_focus: popup_no_focus.is_active(),
            popup_position: match popup_position.active_id().as_deref() {
                Some("center") => PopupPosition::Center,
                Some("none") => PopupPosition::None,
                _ => PopupPosition::NearPointer,
            },
            pointer_offset_x: offset_x.value() as i32,
            pointer_offset_y: offset_y.value() as i32,
            popup_min_width: min_width_value,
            popup_max_width: max_width_value,
        };

        match save_config(&new_config) {
            Ok(_) => status_for_save.set_text("Saved"),
            Err(err) => status_for_save.set_text(&format!("Save failed: {err}")),
        }
    });

    let status_for_reset = status.clone();
    reset.connect_clicked(move |_| match save_config(&AppConfig::default()) {
        Ok(_) => status_for_reset.set_text("Defaults saved"),
        Err(err) => status_for_reset.set_text(&format!("Reset failed: {err}")),
    });

    let window_for_close = window.clone();
    close.connect_clicked(move |_| window_for_close.close());

    bottom.append(&status);
    bottom.append(&reset);
    bottom.append(&close);
    bottom.append(&save);
    outer.append(&bottom);

    window.set_child(Some(&outer));
    window.present();
}

fn add_section(root: &gtk::Box, title: &str) -> gtk::Box {
    let frame = gtk::Frame::new(Some(title));
    frame.set_hexpand(true);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 8);
    body.set_margin_top(10);
    body.set_margin_bottom(10);
    body.set_margin_start(10);
    body.set_margin_end(10);

    frame.set_child(Some(&body));
    root.append(&frame);
    body
}

fn add_row<W: IsA<gtk::Widget>>(root: &gtk::Box, label: &str, widget: &W) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let lab = gtk::Label::new(Some(label));
    lab.set_xalign(0.0);
    lab.set_width_chars(16);
    widget.set_hexpand(true);
    row.append(&lab);
    row.append(widget);
    root.append(&row);
}

fn spin(min: f64, max: f64, step: f64, value: f64) -> gtk::SpinButton {
    let adj = gtk::Adjustment::new(value, min, max, step, step * 10.0, 0.0);
    gtk::SpinButton::new(Some(&adj), step, 0)
}

fn acquire_selected_text(config: &AppConfig) -> Result<String> {
    send_copy_hyprctl()?;
    thread::sleep(Duration::from_millis(config.copy_delay_ms));
    read_clipboard()
}

fn send_copy_hyprctl() -> Result<()> {
    let status = Command::new("hyprctl")
        .args(["dispatch", "sendshortcut", "CTRL, C, activewindow"])
        .status()
        .context("failed to run hyprctl")?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("hyprctl sendshortcut failed"))
    }
}

fn read_clipboard() -> Result<String> {
    let output = Command::new("wl-paste")
        .args(["--no-newline"])
        .output()
        .context("failed to run wl-paste")?;
    if !output.status.success() {
        return Err(anyhow!("wl-paste failed"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn set_clipboard(text: &str) -> Result<()> {
    let mut child = Command::new("wl-copy")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .context("failed to run wl-copy")?;
    if let Some(stdin) = child.stdin.as_mut() {
        use std::io::Write;
        stdin.write_all(text.as_bytes())?;
    }
    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("wl-copy failed"))
    }
}

fn normalize_clip(mut text: String, max_chars: usize) -> String {
    text = text.trim().replace('\r', "");
    if text.chars().count() > max_chars {
        let mut clipped: String = text.chars().take(max_chars).collect();
        clipped.push('…');
        clipped
    } else {
        text
    }
}

fn translate(text: &str, config: &AppConfig) -> Result<String> {
    if text.starts_with("Selection error:") || text == "No selected text" {
        return Ok(text.to_string());
    }
    match config.engine {
        Engine::GoogleFree => translate_google_free(text, config),
        Engine::GoogleCloud => translate_google_cloud(text, config),
        Engine::Baidu => translate_baidu(text, config),
    }
}

#[derive(Debug, Deserialize)]
struct GoogleCloudResponse {
    data: GoogleCloudData,
}

#[derive(Debug, Deserialize)]
struct GoogleCloudData {
    translations: Vec<GoogleCloudTranslation>,
}

#[derive(Debug, Deserialize)]
struct GoogleCloudTranslation {
    #[serde(rename = "translatedText")]
    translated_text: String,
}

fn client(config: &AppConfig) -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(config.timeout_seconds.max(1)))
        .user_agent("Mozilla/5.0 Tautomorphism/0.7.1")
        .build()?)
}

fn translate_google_free(text: &str, config: &AppConfig) -> Result<String> {
    let sl = if config.source_lang.trim().is_empty() {
        "auto"
    } else {
        config.source_lang.trim()
    };
    let tl = if config.target_lang.trim().is_empty() {
        "zh-CN"
    } else {
        config.target_lang.trim()
    };
    let url = format!(
        "https://translate.googleapis.com/translate_a/single?client=gtx&sl={}&tl={}&dt=t&q={}",
        urlencoding::encode(sl),
        urlencoding::encode(tl),
        urlencoding::encode(text)
    );
    let value: serde_json::Value = client(config)?.get(url).send()?.error_for_status()?.json()?;
    let mut out = String::new();
    if let Some(items) = value.get(0).and_then(|v| v.as_array()) {
        for item in items {
            if let Some(s) = item.get(0).and_then(|v| v.as_str()) {
                out.push_str(s);
            }
        }
    }
    if out.trim().is_empty() {
        Err(anyhow!("empty result"))
    } else {
        Ok(out)
    }
}

fn translate_google_cloud(text: &str, config: &AppConfig) -> Result<String> {
    let key = config.google_api_key.trim();
    if key.is_empty() {
        return Err(anyhow!("Google key is empty"));
    }
    let target = if config.target_lang.trim().is_empty() {
        "zh-CN"
    } else {
        config.target_lang.trim()
    };
    let url = format!(
        "https://translation.googleapis.com/language/translate/v2?key={}",
        urlencoding::encode(key)
    );
    let mut params = vec![
        ("q", text.to_string()),
        ("target", target.to_string()),
        ("format", "text".to_string()),
    ];
    if config.source_lang.trim() != "auto" && !config.source_lang.trim().is_empty() {
        params.push(("source", config.source_lang.trim().to_string()));
    }
    let parsed: GoogleCloudResponse = client(config)?
        .post(url)
        .form(&params)
        .send()?
        .error_for_status()?
        .json()?;
    parsed
        .data
        .translations
        .first()
        .map(|t| html_escape::decode_html_entities(&t.translated_text).to_string())
        .ok_or_else(|| anyhow!("no translation"))
}

#[derive(Debug, Deserialize)]
struct BaiduResponse {
    trans_result: Option<Vec<BaiduItem>>,
    error_code: Option<String>,
    error_msg: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BaiduItem {
    dst: String,
}

fn translate_baidu(text: &str, config: &AppConfig) -> Result<String> {
    let appid = config.baidu_appid.trim();
    let secret = config.baidu_secret.trim();
    if appid.is_empty() || secret.is_empty() {
        return Err(anyhow!("Baidu credentials are empty"));
    }
    let from = if config.source_lang.trim().is_empty() || config.source_lang.trim() == "auto" {
        "auto"
    } else {
        config.source_lang.trim()
    };
    let to = if config.baidu_target_lang.trim().is_empty() {
        "zh"
    } else {
        config.baidu_target_lang.trim()
    };
    let salt = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis().to_string();
    let sign_raw = format!("{appid}{text}{salt}{secret}");
    let sign = format!("{:x}", md5::compute(sign_raw));
    let params = [
        ("q", text),
        ("from", from),
        ("to", to),
        ("appid", appid),
        ("salt", &salt),
        ("sign", &sign),
    ];
    let parsed: BaiduResponse = client(config)?
        .post("https://fanyi-api.baidu.com/api/trans/vip/translate")
        .form(&params)
        .send()?
        .error_for_status()?
        .json()?;
    if let Some(items) = parsed.trans_result {
        let out = items.into_iter().map(|x| x.dst).collect::<Vec<_>>().join("\n");
        if !out.trim().is_empty() {
            return Ok(out);
        }
    }
    Err(anyhow!(
        "Baidu error {} {}",
        parsed.error_code.unwrap_or_else(|| "unknown".to_string()),
        parsed.error_msg.unwrap_or_else(|| "empty result".to_string())
    ))
}
