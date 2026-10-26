//! A frozen screen for Stage and Caliper: hide Forgekit's own windows, take
//! a screenshot through the portal, and show it in a fullscreen window.
//! Drawing and measuring then happen on that still image, which works on
//! every desktop (no overlays or window tricks needed).

use std::rc::Rc;

use adw::prelude::*;
use gtk::{cairo, gdk, glib};

/// The screenshot, ready for cairo (premultiplied BGRA) and for pixel reads (RGBA).
pub struct Frozen {
    pub surface: cairo::ImageSurface,
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Part of the screenshot (in its pixels) that shows one monitor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crop {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Decode the screenshot (on a worker thread).
fn decode(path: &std::path::Path) -> Result<(Vec<u8>, u32, u32), String> {
    let img = image::open(path).map_err(|e| e.to_string())?.to_rgba8();
    let (width, height) = img.dimensions();
    Ok((img.into_raw(), width, height))
}

/// Build the cairo surface (on the UI thread; cairo surfaces can't cross threads).
fn load(rgba: Vec<u8>, width: u32, height: u32) -> Result<Frozen, String> {
    let mut surface =
        cairo::ImageSurface::create(cairo::Format::ARgb32, width as i32, height as i32).map_err(|e| e.to_string())?;
    let stride = surface.stride() as usize;
    {
        let mut data = surface.data().map_err(|e| e.to_string())?;
        for y in 0..height as usize {
            for x in 0..width as usize {
                let s = (y * width as usize + x) * 4;
                let d = y * stride + x * 4;
                let a = rgba[s + 3] as u32;
                let pm = |c: u8| ((c as u32 * a + 127) / 255) as u8;
                data[d] = pm(rgba[s + 2]);
                data[d + 1] = pm(rgba[s + 1]);
                data[d + 2] = pm(rgba[s]);
                data[d + 3] = a as u8;
            }
        }
    }
    Ok(Frozen { surface, rgba, width, height })
}

/// Hide our windows, capture, then call `ready` with the image. Windows we
/// hid come back when the window returned by `ready` closes.
pub fn freeze(app: &adw::Application, ready: impl FnOnce(Rc<Frozen>) -> Option<gtk::Window> + 'static) {
    let hidden: Vec<gtk::Window> = app.windows().into_iter().filter(|w| w.is_visible()).collect();
    for w in &hidden {
        w.set_visible(false);
    }
    // Keep the app alive while no window is open (e.g. started from a shortcut).
    let hold = app.hold();
    // Give the compositor a moment to actually remove them from the screen.
    let app = app.clone();
    glib::timeout_add_local_once(std::time::Duration::from_millis(300), move || {
        crate::util::background(
            || -> Result<(Vec<u8>, u32, u32), String> {
                let path = forgekit_core::tools::screen::capture_blocking(false).map_err(|e| format!("{e:#}"))?;
                let decoded = decode(&path);
                // The portal saved a file for us; don't leave it in Pictures.
                let _ = std::fs::remove_file(&path);
                decoded
            },
            move |r| {
                let _hold = hold;
                let restore = move || {
                    for w in &hidden {
                        w.set_visible(true);
                    }
                };
                match r.and_then(|(rgba, w, h)| load(rgba, w, h)) {
                    Ok(f) => match ready(Rc::new(f)) {
                        Some(win) => {
                            win.connect_close_request(move |_| {
                                restore();
                                glib::Propagation::Proceed
                            });
                        }
                        None => restore(),
                    },
                    Err(e) => {
                        restore();
                        forgekit_core::tools::notify("Couldn't capture the screen", &e);
                        let _ = &app;
                    }
                }
            },
        );
    });
}

/// A borderless fullscreen window for a frozen screen.
pub fn fullscreen_window(app: &adw::Application, title: &str) -> adw::ApplicationWindow {
    // Monitor-sized, in case the desktop doesn't allow fullscreen.
    let (dw, dh) = gdk::Display::default()
        .and_then(|d| d.monitors().item(0).and_downcast::<gdk::Monitor>())
        .map(|m| (m.geometry().width(), m.geometry().height()))
        .unwrap_or((1280, 800));
    let w = adw::ApplicationWindow::builder()
        .application(app)
        .title(title)
        .decorated(false)
        .default_width(dw)
        .default_height(dh)
        .build();
    w.add_css_class("fk-frozen");
    w.fullscreen();
    w
}

/// Which part of the screenshot is the monitor this window ended up on.
/// The portal captures every monitor as one image of their bounding box.
pub fn crop_for(window: &impl IsA<gtk::Native>, frozen: &Frozen) -> Crop {
    let whole = Crop { x: 0.0, y: 0.0, w: frozen.width as f64, h: frozen.height as f64 };
    let Some(display) = gdk::Display::default() else { return whole };
    let monitors: Vec<gdk::Monitor> = (0..display.monitors().n_items())
        .filter_map(|i| display.monitors().item(i).and_downcast::<gdk::Monitor>())
        .collect();
    if monitors.len() < 2 {
        return whole;
    }
    let Some(mine) = window.surface().and_then(|s| display.monitor_at_surface(&s)) else { return whole };
    let (mut ux0, mut uy0, mut ux1, mut uy1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for m in &monitors {
        let g = m.geometry();
        ux0 = ux0.min(g.x());
        uy0 = uy0.min(g.y());
        ux1 = ux1.max(g.x() + g.width());
        uy1 = uy1.max(g.y() + g.height());
    }
    let scale = frozen.width as f64 / (ux1 - ux0).max(1) as f64;
    let g = mine.geometry();
    Crop {
        x: (g.x() - ux0) as f64 * scale,
        y: (g.y() - uy0) as f64 * scale,
        w: g.width() as f64 * scale,
        h: g.height() as f64 * scale,
    }
}
