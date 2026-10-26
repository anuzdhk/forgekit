//! Caliper: measure on a frozen screen. Hover to see the distances to the
//! nearest edges, drag to measure a box, click to copy the size. Esc exits.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use forgekit_core::tools::caliper::{self, Image};
use gtk::{cairo, gdk, glib};

use crate::freeze::{self, Crop, Frozen};

struct State {
    frozen: Rc<Frozen>,
    crop: Option<Crop>,
    size: (f64, f64),
    pointer: Option<(f64, f64)>,
    /// A dragged box, in screenshot pixels.
    drag: Option<((f64, f64), (f64, f64))>,
}

impl State {
    fn crop(&self) -> Crop {
        self.crop.unwrap_or(Crop { x: 0.0, y: 0.0, w: self.frozen.width as f64, h: self.frozen.height as f64 })
    }

    /// (scale, x offset, y offset) mapping crop pixels to window pixels.
    fn fit(&self) -> (f64, f64, f64) {
        let c = self.crop();
        let s = (self.size.0 / c.w).min(self.size.1 / c.h).max(0.0001);
        (s, (self.size.0 - c.w * s) / 2.0, (self.size.1 - c.h * s) / 2.0)
    }

    /// Window point → whole-screenshot pixel.
    fn to_px(&self, x: f64, y: f64) -> (f64, f64) {
        let (s, ox, oy) = self.fit();
        let c = self.crop();
        (((x - ox) / s).clamp(0.0, c.w - 1.0) + c.x, ((y - oy) / s).clamp(0.0, c.h - 1.0) + c.y)
    }

    fn to_window(&self, px: f64, py: f64) -> (f64, f64) {
        let (s, ox, oy) = self.fit();
        let c = self.crop();
        ((px - c.x) * s + ox, (py - c.y) * s + oy)
    }

    /// Edge distances from a pixel, kept inside this monitor.
    fn spacing(&self, px: f64, py: f64) -> caliper::Spacing {
        let f = &self.frozen;
        let img = Image { width: f.width, height: f.height, rgba: &f.rgba };
        let (x, y) = (px as u32, py as u32);
        let mut s = caliper::spacing(&img, x, y, 24);
        let c = self.crop();
        s.left = s.left.min(x - c.x as u32);
        s.up = s.up.min(y - c.y as u32);
        s.right = s.right.min((c.x + c.w) as u32 - 1 - x);
        s.down = s.down.min((c.y + c.h) as u32 - 1 - y);
        s
    }
}

const ACCENT: (f64, f64, f64) = (1.0, 0.27, 0.42);

fn label(cr: &cairo::Context, text: &str, x: f64, y: f64) {
    cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
    cr.set_font_size(13.0);
    let Ok(ext) = cr.text_extents(text) else { return };
    let (w, h) = (ext.width() + 16.0, 24.0);
    let (bx, by) = (x + 14.0, y + 14.0);
    cr.set_source_rgba(0.08, 0.08, 0.1, 0.88);
    cr.rectangle(bx, by, w, h);
    let _ = cr.fill();
    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.move_to(bx + 8.0, by + 16.5);
    let _ = cr.show_text(text);
}

pub fn open(app: &adw::Application) {
    let app2 = app.clone();
    freeze::freeze(app, move |frozen| Some(build(&app2, frozen).upcast()));
}

fn build(app: &adw::Application, frozen: Rc<Frozen>) -> adw::ApplicationWindow {
    let win = freeze::fullscreen_window(app, "Caliper");
    let st = Rc::new(RefCell::new(State { frozen, crop: None, size: (1.0, 1.0), pointer: None, drag: None }));
    let area = gtk::DrawingArea::builder().hexpand(true).vexpand(true).build();
    area.set_cursor_from_name(Some("crosshair"));
    {
        let (st, win) = (st.clone(), win.clone());
        area.set_draw_func(move |_, cr, wd, ht| {
            let mut s = st.borrow_mut();
            if s.crop.is_none() {
                s.crop = Some(freeze::crop_for(&win, &s.frozen));
            }
            s.size = (wd as f64, ht as f64);
            let (k, ox, oy) = s.fit();
            let c = s.crop();
            cr.set_source_rgb(0.0, 0.0, 0.0);
            let _ = cr.paint();
            cr.save().ok();
            cr.translate(ox, oy);
            cr.scale(k, k);
            let _ = cr.set_source_surface(&s.frozen.surface, -c.x, -c.y);
            let _ = cr.paint();
            cr.restore().ok();
            cr.set_source_rgb(ACCENT.0, ACCENT.1, ACCENT.2);
            cr.set_line_width(1.0);

            if let Some((a, b)) = s.drag {
                let (x0, y0) = s.to_window(a.0.min(b.0), a.1.min(b.1));
                let (x1, y1) = s.to_window(a.0.max(b.0) + 1.0, a.1.max(b.1) + 1.0);
                cr.set_source_rgba(ACCENT.0, ACCENT.1, ACCENT.2, 0.15);
                cr.rectangle(x0, y0, x1 - x0, y1 - y0);
                let _ = cr.fill_preserve();
                cr.set_source_rgb(ACCENT.0, ACCENT.1, ACCENT.2);
                let _ = cr.stroke();
                let (w, h) = ((a.0 - b.0).abs() as u32 + 1, (a.1 - b.1).abs() as u32 + 1);
                label(cr, &caliper::describe(w, h), x1 - 14.0, y1 - 14.0);
                return;
            }
            let Some((mx, my)) = s.pointer else { return };
            let (px, py) = s.to_px(mx, my);
            let sp = s.spacing(px, py);
            let (lx, _) = s.to_window(px - sp.left as f64, py);
            let (rx, _) = s.to_window(px + sp.right as f64 + 1.0, py);
            let (_, uy) = s.to_window(px, py - sp.up as f64);
            let (_, dy) = s.to_window(px, py + sp.down as f64 + 1.0);
            let (cx, cy) = s.to_window(px + 0.5, py + 0.5);
            cr.move_to(lx, cy);
            cr.line_to(rx, cy);
            cr.move_to(cx, uy);
            cr.line_to(cx, dy);
            for (x, y, horizontal) in [(lx, cy, false), (rx, cy, false), (cx, uy, true), (cx, dy, true)] {
                if horizontal {
                    cr.move_to(x - 5.0, y);
                    cr.line_to(x + 5.0, y);
                } else {
                    cr.move_to(x, y - 5.0);
                    cr.line_to(x, y + 5.0);
                }
            }
            let _ = cr.stroke();
            let (w, h) = sp.size();
            label(cr, &format!("{}  ·  click to copy", caliper::describe(w, h)), cx, cy);
        });
    }

    let motion = gtk::EventControllerMotion::new();
    {
        let (st, area) = (st.clone(), area.clone());
        motion.connect_motion(move |_, x, y| {
            st.borrow_mut().pointer = Some((x, y));
            area.queue_draw();
        });
    }
    area.add_controller(motion);

    let toast = adw::ToastOverlay::new();
    let drag = gtk::GestureDrag::builder().button(gdk::BUTTON_PRIMARY).build();
    {
        let (st, area) = (st.clone(), area.clone());
        drag.connect_drag_update(move |g, dx, dy| {
            if dx.abs() < 3.0 && dy.abs() < 3.0 {
                return;
            }
            let Some((x0, y0)) = g.start_point() else { return };
            let mut s = st.borrow_mut();
            let a = s.to_px(x0, y0);
            let b = s.to_px(x0 + dx, y0 + dy);
            s.drag = Some((a, b));
            area.queue_draw();
        });
    }
    {
        let (st, area, toast) = (st.clone(), area.clone(), toast.clone());
        drag.connect_drag_end(move |g, _, _| {
            let mut s = st.borrow_mut();
            let size = match s.drag.take() {
                Some((a, b)) => ((a.0 - b.0).abs() as u32 + 1, (a.1 - b.1).abs() as u32 + 1),
                None => {
                    let Some((x, y)) = g.start_point() else { return };
                    let (px, py) = s.to_px(x, y);
                    s.spacing(px, py).size()
                }
            };
            let text = caliper::describe(size.0, size.1);
            crate::util::copy_lasting(&text);
            let t = adw::Toast::new(&format!("Copied {text}"));
            t.set_timeout(2);
            toast.add_toast(t);
            area.queue_draw();
        });
    }
    area.add_controller(drag);

    let keys = gtk::EventControllerKey::new();
    {
        let win = win.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                win.close();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }
    win.add_controller(keys);

    let hint = gtk::Label::builder()
        .label("Hover to measure · drag a box · click to copy · Esc to exit")
        .css_classes(["fk-stage-hint"])
        .halign(gtk::Align::Center)
        .valign(gtk::Align::End)
        .margin_bottom(20)
        .build();
    {
        let hint = hint.clone();
        glib::timeout_add_local_once(std::time::Duration::from_secs(4), move || hint.set_visible(false));
    }
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&area));
    overlay.add_overlay(&hint);
    toast.set_child(Some(&overlay));
    win.set_content(Some(&toast));
    win.present();
    win
}
