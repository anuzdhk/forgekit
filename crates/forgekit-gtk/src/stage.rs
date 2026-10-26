//! Stage: zoom into and draw on a frozen screen, for presenting.
//!
//! Mouse: scroll to zoom where you point, drag to draw, right-drag to pan.
//! Keys: P pen, H highlighter, L line, A arrow, R rectangle, O ellipse,
//! 1-6 colors, [ ] thinner/thicker, W whiteboard, K blackboard, E erase all,
//! Ctrl+Z undo, Ctrl+C copy, Ctrl+S save, 0 reset zoom, Esc exit.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{cairo, gdk, glib};

use crate::freeze::{self, Crop, Frozen};
use crate::widgets::{self as w, hbox};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tool {
    Pen,
    Highlighter,
    Line,
    Arrow,
    Rect,
    Ellipse,
}

const TOOLS: [(Tool, &str, &str, &str); 6] = [
    (Tool::Pen, "pencil", "Pen (P)", "p"),
    (Tool::Highlighter, "highlight", "Highlighter (H)", "h"),
    (Tool::Line, "line", "Line (L)", "l"),
    (Tool::Arrow, "arrow-up-right", "Arrow (A)", "a"),
    (Tool::Rect, "square", "Rectangle (R)", "r"),
    (Tool::Ellipse, "circle", "Ellipse (O)", "o"),
];

const COLORS: [&str; 6] = ["#ef4444", "#facc15", "#22c55e", "#3b82f6", "#a855f7", "#ffffff"];
const WIDTHS: [f64; 4] = [3.0, 6.0, 10.0, 16.0];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Board {
    Screen,
    White,
    Black,
}

#[derive(Clone)]
struct Stroke {
    tool: Tool,
    color: &'static str,
    /// In screenshot pixels.
    width: f64,
    points: Vec<(f64, f64)>,
}

struct State {
    frozen: Rc<Frozen>,
    crop: Option<Crop>,
    zoom: f64,
    /// View center, in crop coordinates.
    center: (f64, f64),
    strokes: Vec<Stroke>,
    current: Option<Stroke>,
    tool: Tool,
    color: usize,
    width: usize,
    board: Board,
    pan_from: Option<((f64, f64), (f64, f64))>,
    size: (f64, f64),
}

impl State {
    fn crop(&self) -> Crop {
        self.crop.unwrap_or(Crop { x: 0.0, y: 0.0, w: self.frozen.width as f64, h: self.frozen.height as f64 })
    }

    /// Screen pixels per screenshot pixel at zoom 1.
    fn base(&self) -> f64 {
        let c = self.crop();
        (self.size.0 / c.w).min(self.size.1 / c.h).max(0.0001)
    }

    fn to_image(&self, x: f64, y: f64) -> (f64, f64) {
        let s = self.base() * self.zoom;
        ((x - self.size.0 / 2.0) / s + self.center.0, (y - self.size.1 / 2.0) / s + self.center.1)
    }

    fn clamp_center(&mut self) {
        let c = self.crop();
        let s = self.base() * self.zoom;
        let (hw, hh) = (self.size.0 / 2.0 / s, self.size.1 / 2.0 / s);
        let fit = |v: f64, half: f64, len: f64| if half * 2.0 >= len { len / 2.0 } else { v.clamp(half, len - half) };
        self.center = (fit(self.center.0, hw, c.w), fit(self.center.1, hh, c.h));
    }

    /// Zoom by `factor`, keeping the point under (x, y) still.
    fn zoom_at(&mut self, factor: f64, x: f64, y: f64) {
        let before = self.to_image(x, y);
        self.zoom = (self.zoom * factor).clamp(1.0, 12.0);
        let after = self.to_image(x, y);
        self.center.0 += before.0 - after.0;
        self.center.1 += before.1 - after.1;
        self.clamp_center();
    }
}

fn rgb(hex: &str) -> (f64, f64, f64) {
    let n = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0xffffff);
    (((n >> 16) & 255) as f64 / 255.0, ((n >> 8) & 255) as f64 / 255.0, (n & 255) as f64 / 255.0)
}

fn draw_stroke(cr: &cairo::Context, s: &Stroke) {
    let Some(&first) = s.points.first() else { return };
    let last = *s.points.last().unwrap();
    let (r, g, b) = rgb(s.color);
    let (alpha, width) = if s.tool == Tool::Highlighter { (0.35, s.width * 3.0) } else { (1.0, s.width) };
    cr.set_source_rgba(r, g, b, alpha);
    cr.set_line_width(width);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);
    match s.tool {
        Tool::Pen | Tool::Highlighter => {
            cr.move_to(first.0, first.1);
            for p in &s.points[1..] {
                cr.line_to(p.0, p.1);
            }
        }
        Tool::Line => {
            cr.move_to(first.0, first.1);
            cr.line_to(last.0, last.1);
        }
        Tool::Arrow => {
            cr.move_to(first.0, first.1);
            cr.line_to(last.0, last.1);
            let angle = (last.1 - first.1).atan2(last.0 - first.0);
            let head = (s.width * 4.0).max(14.0);
            for side in [-0.5f64, 0.5] {
                cr.move_to(last.0, last.1);
                cr.line_to(last.0 - head * (angle + side).cos(), last.1 - head * (angle + side).sin());
            }
        }
        Tool::Rect => {
            cr.rectangle(first.0.min(last.0), first.1.min(last.1), (last.0 - first.0).abs(), (last.1 - first.1).abs());
        }
        Tool::Ellipse => {
            let (cx, cy) = ((first.0 + last.0) / 2.0, (first.1 + last.1) / 2.0);
            let (rx, ry) = (((last.0 - first.0) / 2.0).abs().max(1.0), ((last.1 - first.1) / 2.0).abs().max(1.0));
            cr.save().ok();
            cr.translate(cx, cy);
            cr.scale(rx, ry);
            cr.arc(0.0, 0.0, 1.0, 0.0, std::f64::consts::TAU);
            cr.restore().ok();
        }
    }
    let _ = cr.stroke();
}

/// Background and strokes in crop coordinates (no zoom).
fn paint_scene(cr: &cairo::Context, st: &State) {
    let c = st.crop();
    match st.board {
        Board::Screen => {
            let _ = cr.set_source_surface(&st.frozen.surface, -c.x, -c.y);
            let _ = cr.paint();
        }
        Board::White | Board::Black => {
            let v = if st.board == Board::White { 1.0 } else { 0.08 };
            cr.set_source_rgb(v, v, v);
            let _ = cr.paint();
        }
    }
    for s in st.strokes.iter().chain(st.current.iter()) {
        draw_stroke(cr, s);
    }
}

/// The annotated screen at full resolution, as PNG bytes.
fn render_png(st: &State) -> Option<Vec<u8>> {
    let c = st.crop();
    let (w, h) = (c.w as u32, c.h as u32);
    let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, w as i32, h as i32).ok()?;
    {
        let cr = cairo::Context::new(&surface).ok()?;
        paint_scene(&cr, st);
    }
    surface.flush();
    let stride = surface.stride() as usize;
    let data = surface.data().ok()?;
    // Cairo stores premultiplied BGRA; PNG wants straight RGBA.
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h as usize {
        for x in 0..w as usize {
            let p = &data[y * stride + x * 4..y * stride + x * 4 + 4];
            let a = p[3] as u32;
            let un = |c: u8| (c as u32 * 255 + a / 2).checked_div(a).map_or(0, |v| v.min(255) as u8);
            rgba.extend_from_slice(&[un(p[2]), un(p[1]), un(p[0]), p[3]]);
        }
    }
    let img = image::RgbaImage::from_raw(w, h, rgba)?;
    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
    Some(png)
}

/// Open Stage on a fresh frozen screen.
pub fn open(app: &adw::Application) {
    let app2 = app.clone();
    freeze::freeze(app, move |frozen| Some(build(&app2, frozen).upcast()));
}

fn build(app: &adw::Application, frozen: Rc<Frozen>) -> adw::ApplicationWindow {
    let win = freeze::fullscreen_window(app, "Stage");
    let st = Rc::new(RefCell::new(State {
        frozen,
        crop: None,
        zoom: 1.0,
        center: (0.0, 0.0),
        strokes: Vec::new(),
        current: None,
        tool: Tool::Pen,
        color: 0,
        width: 1,
        board: Board::Screen,
        pan_from: None,
        size: (1.0, 1.0),
    }));

    let area = gtk::DrawingArea::builder().hexpand(true).vexpand(true).build();
    {
        let (st, win) = (st.clone(), win.clone());
        area.set_draw_func(move |_, cr, wd, ht| {
            let mut s = st.borrow_mut();
            if s.crop.is_none() {
                let crop = freeze::crop_for(&win, &s.frozen);
                s.crop = Some(crop);
                s.center = (crop.w / 2.0, crop.h / 2.0);
            }
            s.size = (wd as f64, ht as f64);
            s.clamp_center();
            cr.set_source_rgb(0.0, 0.0, 0.0);
            let _ = cr.paint();
            let k = s.base() * s.zoom;
            cr.translate(wd as f64 / 2.0, ht as f64 / 2.0);
            cr.scale(k, k);
            cr.translate(-s.center.0, -s.center.1);
            paint_scene(cr, &s);
        });
    }

    // Draw with the left button.
    let draw = gtk::GestureDrag::builder().button(gdk::BUTTON_PRIMARY).build();
    {
        let (st, area) = (st.clone(), area.clone());
        draw.connect_drag_begin(move |_, x, y| {
            let mut s = st.borrow_mut();
            let p = s.to_image(x, y);
            let width = WIDTHS[s.width] / (s.base() * s.zoom);
            s.current = Some(Stroke { tool: s.tool, color: COLORS[s.color], width, points: vec![p, p] });
            area.queue_draw();
        });
    }
    {
        let (st, area) = (st.clone(), area.clone());
        draw.connect_drag_update(move |g, dx, dy| {
            let Some((x0, y0)) = g.start_point() else { return };
            let mut s = st.borrow_mut();
            let p = s.to_image(x0 + dx, y0 + dy);
            if let Some(c) = s.current.as_mut() {
                if matches!(c.tool, Tool::Pen | Tool::Highlighter) {
                    c.points.push(p);
                } else {
                    *c.points.last_mut().unwrap() = p;
                }
            }
            area.queue_draw();
        });
    }
    {
        let (st, area) = (st.clone(), area.clone());
        draw.connect_drag_end(move |_, _, _| {
            let mut s = st.borrow_mut();
            if let Some(c) = s.current.take() {
                s.strokes.push(c);
            }
            area.queue_draw();
        });
    }
    area.add_controller(draw);

    // Pan with the right button.
    let pan = gtk::GestureDrag::builder().button(gdk::BUTTON_SECONDARY).build();
    {
        let st = st.clone();
        pan.connect_drag_begin(move |_, x, y| {
            let mut s = st.borrow_mut();
            s.pan_from = Some(((x, y), s.center));
        });
    }
    {
        let (st, area) = (st.clone(), area.clone());
        pan.connect_drag_update(move |_, dx, dy| {
            let mut s = st.borrow_mut();
            if let Some((_, c0)) = s.pan_from {
                let k = s.base() * s.zoom;
                s.center = (c0.0 - dx / k, c0.1 - dy / k);
                s.clamp_center();
            }
            area.queue_draw();
        });
    }
    area.add_controller(pan);

    // Scroll to zoom at the pointer.
    let pointer = Rc::new(RefCell::new((0.0, 0.0)));
    let motion = gtk::EventControllerMotion::new();
    {
        let pointer = pointer.clone();
        motion.connect_motion(move |_, x, y| *pointer.borrow_mut() = (x, y));
    }
    area.add_controller(motion);
    let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
    {
        let (st, area, pointer) = (st.clone(), area.clone(), pointer.clone());
        scroll.connect_scroll(move |_, _, dy| {
            let (x, y) = *pointer.borrow();
            st.borrow_mut().zoom_at(if dy < 0.0 { 1.25 } else { 0.8 }, x, y);
            area.queue_draw();
            glib::Propagation::Stop
        });
    }
    area.add_controller(scroll);

    // ---- toolbar
    let bar = hbox(4);
    bar.add_css_class("fk-stage-bar");
    bar.set_halign(gtk::Align::Center);
    bar.set_valign(gtk::Align::Start);
    bar.set_margin_top(16);
    let tool_buttons: Rc<Vec<(Tool, gtk::ToggleButton)>> = Rc::new(
        TOOLS
            .iter()
            .map(|(t, icon, tip, _)| {
                let b = gtk::ToggleButton::builder()
                    .icon_name(w::icon_name(icon))
                    .tooltip_text(*tip)
                    .css_classes(["flat"])
                    .build();
                (*t, b)
            })
            .collect(),
    );
    for (i, (t, b)) in tool_buttons.iter().enumerate() {
        if i > 0 {
            b.set_group(Some(&tool_buttons[0].1));
        }
        b.set_active(*t == Tool::Pen);
        let (st, t) = (st.clone(), *t);
        b.connect_toggled(move |b| {
            if b.is_active() {
                st.borrow_mut().tool = t;
            }
        });
        bar.append(b);
    }
    bar.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    let swatches: Rc<Vec<gtk::Button>> = Rc::new(
        COLORS
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let dot = gtk::DrawingArea::builder().content_width(18).content_height(18).build();
                let (r, g, b) = rgb(c);
                dot.set_draw_func(move |_, cr, wd, ht| {
                    cr.set_source_rgb(r, g, b);
                    cr.arc(wd as f64 / 2.0, ht as f64 / 2.0, wd.min(ht) as f64 / 2.0, 0.0, std::f64::consts::TAU);
                    let _ = cr.fill();
                });
                gtk::Button::builder()
                    .child(&dot)
                    .tooltip_text(format!("Color {}", i + 1))
                    .css_classes(["flat", "fk-swatch"])
                    .build()
            })
            .collect(),
    );
    let set_color = {
        let (st, swatches) = (st.clone(), swatches.clone());
        Rc::new(move |i: usize| {
            st.borrow_mut().color = i;
            for (j, b) in swatches.iter().enumerate() {
                if i == j {
                    b.add_css_class("selected");
                } else {
                    b.remove_css_class("selected");
                }
            }
        })
    };
    for (i, b) in swatches.iter().enumerate() {
        let set_color = set_color.clone();
        b.connect_clicked(move |_| set_color(i));
        bar.append(b);
    }
    set_color(0);
    bar.append(&gtk::Separator::new(gtk::Orientation::Vertical));

    let toast = adw::ToastOverlay::new();
    let say = {
        let toast = toast.clone();
        Rc::new(move |m: &str| {
            let t = adw::Toast::new(m);
            t.set_timeout(2);
            toast.add_toast(t);
        })
    };
    let undo = {
        let (st, area) = (st.clone(), area.clone());
        Rc::new(move || {
            st.borrow_mut().strokes.pop();
            area.queue_draw();
        })
    };
    let board = {
        let (st, area) = (st.clone(), area.clone());
        Rc::new(move |b: Board| {
            let mut s = st.borrow_mut();
            s.board = if s.board == b { Board::Screen } else { b };
            drop(s);
            area.queue_draw();
        })
    };
    let copy = {
        let (st, say) = (st.clone(), say.clone());
        Rc::new(move || match render_png(&st.borrow()) {
            Some(png) if crate::util::copy_png(&png) => say("Copied the annotated screen"),
            _ => say("Couldn't copy the image"),
        })
    };
    let save = {
        let (st, say) = (st.clone(), say.clone());
        Rc::new(move || {
            let Some(png) = render_png(&st.borrow()) else { return say("Couldn't save the image") };
            let dir =
                glib::user_special_dir(glib::UserDirectory::Pictures).unwrap_or_else(glib::home_dir).join("Forgekit");
            let name = format!("Stage {}.png", chrono::Local::now().format("%Y-%m-%d %H-%M-%S"));
            let path = dir.join(name);
            match std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, png)) {
                Ok(()) => say(&format!("Saved to Pictures/Forgekit/{}", crate::util::file_name(&path))),
                Err(e) => say(&format!("Couldn't save: {e}")),
            }
        })
    };
    for (icon, tip, action) in [
        ("arrow-back-up", "Undo (Ctrl+Z)", 0),
        ("chalkboard", "Whiteboard (W)", 1),
        ("eraser", "Erase all (E)", 2),
        ("copy", "Copy (Ctrl+C)", 3),
        ("download", "Save (Ctrl+S)", 4),
        ("x", "Exit (Esc)", 5),
    ] {
        let b = gtk::Button::builder().icon_name(w::icon_name(icon)).tooltip_text(tip).css_classes(["flat"]).build();
        let (undo, board, copy, save, st, area, win) =
            (undo.clone(), board.clone(), copy.clone(), save.clone(), st.clone(), area.clone(), win.clone());
        b.connect_clicked(move |_| match action {
            0 => undo(),
            1 => board(Board::White),
            2 => {
                st.borrow_mut().strokes.clear();
                area.queue_draw();
            }
            3 => copy(),
            4 => save(),
            _ => win.close(),
        });
        bar.append(&b);
    }

    let hint = gtk::Label::builder()
        .label("Scroll to zoom · drag to draw · right-drag to pan · Esc to exit")
        .css_classes(["fk-stage-hint"])
        .halign(gtk::Align::Center)
        .valign(gtk::Align::End)
        .margin_bottom(20)
        .build();
    {
        let hint = hint.clone();
        glib::timeout_add_local_once(std::time::Duration::from_secs(4), move || hint.set_visible(false));
    }

    // ---- keys
    let keys = gtk::EventControllerKey::new();
    {
        let (st, area, win, tool_buttons) = (st.clone(), area.clone(), win.clone(), tool_buttons.clone());
        keys.connect_key_pressed(move |_, key, _, mods| {
            let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
            let name = key.to_lower().name().map(|n| n.to_string()).unwrap_or_default();
            match (ctrl, name.as_str()) {
                (_, "Escape") => win.close(),
                (true, "z") => undo(),
                (true, "c") => copy(),
                (true, "s") => save(),
                (false, "w") => board(Board::White),
                (false, "k") => board(Board::Black),
                (false, "e") => {
                    st.borrow_mut().strokes.clear();
                    area.queue_draw();
                }
                (false, "0") => {
                    let mut s = st.borrow_mut();
                    s.zoom = 1.0;
                    s.clamp_center();
                    drop(s);
                    area.queue_draw();
                }
                (false, "plus" | "equal" | "KP_Add") => {
                    let (w, h) = st.borrow().size;
                    st.borrow_mut().zoom_at(1.25, w / 2.0, h / 2.0);
                    area.queue_draw();
                }
                (false, "minus" | "KP_Subtract") => {
                    let (w, h) = st.borrow().size;
                    st.borrow_mut().zoom_at(0.8, w / 2.0, h / 2.0);
                    area.queue_draw();
                }
                (false, "bracketleft") => {
                    let mut s = st.borrow_mut();
                    s.width = s.width.saturating_sub(1);
                }
                (false, "bracketright") => {
                    let mut s = st.borrow_mut();
                    s.width = (s.width + 1).min(WIDTHS.len() - 1);
                }
                (false, d) if d.len() == 1 && ("1"..="6").contains(&d) => set_color(d.parse::<usize>().unwrap() - 1),
                (false, k) => {
                    if let Some((_, b)) =
                        TOOLS.iter().zip(tool_buttons.iter()).find(|((_, _, _, key), _)| *key == k).map(|(_, tb)| tb)
                    {
                        b.set_active(true);
                    } else {
                        return glib::Propagation::Proceed;
                    }
                }
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
    }
    win.add_controller(keys);

    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&area));
    overlay.add_overlay(&bar);
    overlay.add_overlay(&hint);
    toast.set_child(Some(&overlay));
    win.set_content(Some(&toast));
    win.present();
    win
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> State {
        let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 200, 100).unwrap();
        let frozen = Rc::new(Frozen { surface, rgba: vec![0; 200 * 100 * 4], width: 200, height: 100 });
        State {
            frozen,
            crop: Some(Crop { x: 0.0, y: 0.0, w: 200.0, h: 100.0 }),
            zoom: 1.0,
            center: (100.0, 50.0),
            strokes: Vec::new(),
            current: None,
            tool: Tool::Pen,
            color: 0,
            width: 1,
            board: Board::Screen,
            pan_from: None,
            size: (400.0, 200.0),
        }
    }

    #[test]
    fn zoom_keeps_the_pointed_spot_still() {
        let mut s = state();
        assert_eq!(s.to_image(0.0, 0.0), (0.0, 0.0));
        let before = s.to_image(300.0, 150.0);
        s.zoom_at(2.0, 300.0, 150.0);
        let after = s.to_image(300.0, 150.0);
        assert!((before.0 - after.0).abs() < 1e-9 && (before.1 - after.1).abs() < 1e-9);
        // Can't zoom out past the whole screen, and the view stays inside it.
        s.zoom_at(0.1, 0.0, 0.0);
        assert_eq!((s.zoom, s.center), (1.0, (100.0, 50.0)));
    }

    #[test]
    fn renders_annotations() {
        let mut s = state();
        s.strokes.push(Stroke {
            tool: Tool::Arrow,
            color: COLORS[0],
            width: 4.0,
            points: vec![(10.0, 10.0), (90.0, 60.0)],
        });
        let png = render_png(&s).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        assert_eq!(image::load_from_memory(&png).unwrap().width(), 200);
    }
}
