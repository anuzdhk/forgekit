//! Wayland clipboard backend for compositors with `wlr-data-control`
//! (KDE Plasma, Sway, Hyprland, labwc and other wlroots compositors).

use std::io::{Read, Write};
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::sync::mpsc::Receiver;

use anyhow::Context;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, event_created_child};
use wayland_protocols_wlr::data_control::v1::client::{
    zwlr_data_control_device_v1::{self, ZwlrDataControlDeviceV1},
    zwlr_data_control_manager_v1::ZwlrDataControlManagerV1,
    zwlr_data_control_offer_v1::{self, ZwlrDataControlOfferV1},
    zwlr_data_control_source_v1::{self, ZwlrDataControlSourceV1},
};

use super::{Backend, Clip, Command};

pub struct WlrBackend;

#[derive(Default)]
struct State {
    /// Formats announced for each live offer.
    offers: Vec<(ZwlrDataControlOfferV1, Vec<String>)>,
    /// The offer that just became the selection, waiting to be read.
    pending: Option<ZwlrDataControlOfferV1>,
    /// Data we serve while our source is the selection.
    serving: Option<(ZwlrDataControlSourceV1, Clip)>,
    /// The next selection event is our own source coming back; reading it
    /// would block on ourselves.
    skip_next: bool,
}

impl Backend for WlrBackend {
    fn name(&self) -> &'static str {
        "wlr-data-control"
    }

    fn run(
        self: Box<Self>,
        mut on_clip: Box<dyn FnMut(Clip) + Send>,
        commands: Receiver<Command>,
        wake: OwnedFd,
    ) -> anyhow::Result<()> {
        let conn = Connection::connect_to_env().context("connecting to the compositor")?;
        let (globals, mut queue): (_, EventQueue<State>) = registry_queue_init(&conn)?;
        let qh = queue.handle();
        let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=7, ())?;
        let manager: ZwlrDataControlManagerV1 = globals.bind(&qh, 1..=2, ()).context("no wlr-data-control")?;
        let device = manager.get_data_device(&seat, &qh, ());
        let mut state = State::default();

        loop {
            queue.dispatch_pending(&mut state)?;
            if let Some(offer) = state.pending.take() {
                let formats =
                    state.offers.iter().find(|(o, _)| *o == offer).map(|(_, f)| f.clone()).unwrap_or_default();
                if let Some(mime) = super::choose_format(&formats)
                    && let Ok(data) = receive(&conn, &offer, &mime)
                {
                    on_clip(Clip { mime: super::storage_mime(&mime), data });
                }
            }
            queue.flush()?;
            let Some(guard) = queue.prepare_read() else { continue };
            let ready = super::poll(&[guard.connection_fd().as_raw_fd(), wake.as_raw_fd()], 1000)?;
            if ready[0] {
                guard.read()?;
            } else {
                drop(guard);
            }
            if ready[1] {
                super::drain(&wake);
                while let Ok(cmd) = commands.try_recv() {
                    match cmd {
                        Command::Stop => return Ok(()),
                        Command::Set(clip) => {
                            let source = manager.create_data_source(&qh, ());
                            source.offer(clip.mime.clone());
                            if !clip.is_image() {
                                for m in ["text/plain", "UTF8_STRING", "STRING", "TEXT"] {
                                    source.offer(m.into());
                                }
                            }
                            device.set_selection(Some(&source));
                            state.skip_next = true;
                            if let Some((old, _)) = state.serving.replace((source, clip)) {
                                old.destroy();
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Ask the current owner for `mime` and read it through a pipe.
fn receive(conn: &Connection, offer: &ZwlrDataControlOfferV1, mime: &str) -> anyhow::Result<Vec<u8>> {
    let (read, write) = super::wake_pipe()?;
    // The read end must block; wake_pipe makes both ends non-blocking.
    // SAFETY: fcntl on a valid fd.
    unsafe { libc::fcntl(read.as_raw_fd(), libc::F_SETFL, 0) };
    offer.receive(mime.to_string(), write.as_fd());
    conn.flush()?;
    drop(write);
    let mut data = Vec::new();
    std::fs::File::from(read).take(64 * 1024 * 1024).read_to_end(&mut data)?;
    Ok(data)
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(_: &mut Self, _: &wl_seat::WlSeat, _: wl_seat::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<ZwlrDataControlManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwlrDataControlManagerV1,
        _: <ZwlrDataControlManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrDataControlDeviceV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZwlrDataControlDeviceV1,
        event: zwlr_data_control_device_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_data_control_device_v1::Event::DataOffer { id } => state.offers.push((id, Vec::new())),
            zwlr_data_control_device_v1::Event::Selection { id } => {
                // Drop offers that are no longer current.
                state.offers.retain(|(o, _)| {
                    let keep = Some(o) == id.as_ref();
                    if !keep {
                        o.destroy();
                    }
                    keep
                });
                if std::mem::take(&mut state.skip_next) {
                    return;
                }
                state.pending = id;
            }
            zwlr_data_control_device_v1::Event::Finished => {}
            _ => {}
        }
    }

    event_created_child!(State, ZwlrDataControlDeviceV1, [
        zwlr_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ZwlrDataControlOfferV1, ()),
    ]);
}

impl Dispatch<ZwlrDataControlOfferV1, ()> for State {
    fn event(
        state: &mut Self,
        offer: &ZwlrDataControlOfferV1,
        event: zwlr_data_control_offer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_data_control_offer_v1::Event::Offer { mime_type } = event
            && let Some((_, formats)) = state.offers.iter_mut().find(|(o, _)| o == offer)
        {
            formats.push(mime_type);
        }
    }
}

impl Dispatch<ZwlrDataControlSourceV1, ()> for State {
    fn event(
        state: &mut Self,
        source: &ZwlrDataControlSourceV1,
        event: zwlr_data_control_source_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_data_control_source_v1::Event::Send { fd, .. } => {
                if let Some((_, clip)) = state.serving.as_ref().filter(|(s, _)| s == source) {
                    let data = clip.data.clone();
                    // Write off-thread so a slow reader can't stall the event loop.
                    std::thread::spawn(move || {
                        let _ = std::fs::File::from(fd).write_all(&data);
                    });
                }
            }
            zwlr_data_control_source_v1::Event::Cancelled => {
                if state.serving.as_ref().is_some_and(|(s, _)| s == source) {
                    state.serving = None;
                }
                source.destroy();
            }
            _ => {}
        }
    }
}
