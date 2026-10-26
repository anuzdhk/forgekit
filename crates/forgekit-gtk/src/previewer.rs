//! Glance as the Files app's previewer. GNOME Files (Nautilus) sends Space
//! to whoever owns `org.gnome.NautilusPreviewer2` (normally GNOME Sushi);
//! Forgekit answers when Sushi isn't installed. D-Bus starts us on demand
//! through a service file (`forgekit_core::tools::glance::install_previewer`).
//!
//! Files that aren't pictures or documents are left alone.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::time::Duration;

use adw::prelude::*;
use forgekit_core::tools::glance::{PREVIEWER_IFACE, PREVIEWER_NAME, PREVIEWER_PATH};
use glib::translate::IntoGlib;
use gtk::{gio, glib};

/// Stay around this long after the last preview so the next one is instant.
const LINGER: Duration = Duration::from_secs(15 * 60);

const XML: &str = r#"<node>
  <interface name="org.gnome.NautilusPreviewer2">
    <method name="ShowFile">
      <arg type="s" name="uri" direction="in"/>
      <arg type="s" name="window_handle" direction="in"/>
      <arg type="b" name="close_if_shown" direction="in"/>
    </method>
    <method name="Close"/>
    <signal name="SelectionEvent">
      <arg type="u" name="direction"/>
    </signal>
    <property name="Visible" type="b" access="read"/>
  </interface>
</node>"#;

thread_local! {
    static SERVING: Cell<bool> = const { Cell::new(false) };
    static HOLD: RefCell<Option<gio::ApplicationHoldGuard>> = const { RefCell::new(None) };
    static LINGER_ID: RefCell<Option<glib::SourceId>> = const { RefCell::new(None) };
}

/// Start answering the Files app (once per process).
pub fn serve(app: &adw::Application) {
    if SERVING.replace(true) {
        return;
    }
    let Some(conn) = app.dbus_connection() else {
        eprintln!("previewer: no session bus");
        return;
    };
    let info = gio::DBusNodeInfo::for_xml(XML).ok().and_then(|n| n.lookup_interface(PREVIEWER_IFACE));
    let Some(info) = info else { return };
    let weak = app.downgrade();
    let registered = conn
        .register_object(PREVIEWER_PATH, &info)
        .method_call(move |_conn, _sender, _path, _iface, method, params, invocation| {
            let Some(app) = weak.upgrade() else { return };
            match method {
                "ShowFile" => {
                    let (uri, _handle, close_if_shown) = params.get::<(String, String, bool)>().unwrap_or_default();
                    let path = gio::File::for_uri(&uri).path().unwrap_or_else(|| PathBuf::from(&uri));
                    if close_if_shown && crate::glance::is_visible() {
                        crate::glance::close();
                    } else {
                        crate::glance::open(&app, &path);
                    }
                    linger(&app);
                }
                "Close" => crate::glance::close(),
                _ => {}
            }
            invocation.return_value(None);
        })
        .property(|_, _, _, _, _| crate::glance::is_visible().to_variant())
        .build();
    if let Err(e) = registered {
        eprintln!("previewer: {e}");
        return;
    }

    // Arrow keys in Glance move the selection in Files, which then sends the next file.
    let c = conn.clone();
    crate::glance::on_arrow(move |dir| {
        let _ = c.emit_signal(
            None,
            PREVIEWER_PATH,
            PREVIEWER_IFACE,
            "SelectionEvent",
            Some(&(dir.into_glib() as u32,).to_variant()),
        );
    });
    // Files asks `Visible` to decide whether selection changes should update the preview.
    let c = conn.clone();
    crate::glance::on_visible(move |v| {
        let changed = glib::VariantDict::new(None);
        changed.insert("Visible", v);
        let _ = c.emit_signal(
            None,
            PREVIEWER_PATH,
            "org.freedesktop.DBus.Properties",
            "PropertiesChanged",
            Some(&(PREVIEWER_IFACE, changed.end(), Vec::<String>::new()).to_variant()),
        );
    });

    // A plain RequestName: gio's name-watching callbacks panic when the bus
    // goes away at logout. 4 = DBUS_NAME_FLAG_DO_NOT_QUEUE.
    conn.call(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "RequestName",
        Some(&(PREVIEWER_NAME, 4u32).to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        -1,
        gio::Cancellable::NONE,
        |r| {
            // 1 = primary owner, 4 = already the owner.
            if !matches!(r.map(|v| v.get::<(u32,)>()), Ok(Some((1 | 4,)))) {
                eprintln!("previewer: couldn't own {PREVIEWER_NAME} (is GNOME Sushi running?)");
            }
        },
    );
    linger(app);
}

/// Keep the app alive after the last preview, then let it exit.
fn linger(app: &adw::Application) {
    HOLD.with_borrow_mut(|h| {
        if h.is_none() {
            *h = Some(app.hold());
        }
    });
    if let Some(id) = LINGER_ID.take() {
        id.remove();
    }
    let id = glib::timeout_add_local_once(LINGER, || {
        LINGER_ID.take();
        HOLD.take();
    });
    LINGER_ID.set(Some(id));
}
