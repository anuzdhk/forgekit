//! X11 clipboard backend: XFixes tells us when CLIPBOARD changes owner; we ask
//! the owner for its TARGETS, then for the best format (handling INCR for big
//! images). When restoring an item we become the owner and answer requests.

use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use anyhow::Context;
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xfixes::{ConnectionExt as _, SelectionEventMask};
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, PropMode, Property, SELECTION_NOTIFY_EVENT,
    SelectionNotifyEvent, SelectionRequestEvent, Window, WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{COPY_DEPTH_FROM_PARENT, CURRENT_TIME, NONE};

use super::{Backend, Clip, Command};

pub struct X11Backend;

struct Atoms {
    clipboard: Atom,
    targets: Atom,
    incr: Atom,
    prop: Atom,
    utf8: Atom,
}

enum Fetch {
    Targets,
    Data { mime: String },
    Incr { mime: String, buf: Vec<u8> },
}

struct State {
    conn: RustConnection,
    win: Window,
    atoms: Atoms,
    fetch: Option<(Fetch, Instant)>,
    /// A refused TARGETS request to try again (XWayland's bridge can answer
    /// before it knows the Wayland side's formats): (when, attempts so far).
    retry: Option<(Instant, u8)>,
    /// What we serve while we own the clipboard.
    owned: Option<Clip>,
    attempts: u8,
}

impl Backend for X11Backend {
    fn name(&self) -> &'static str {
        "x11"
    }

    fn run(
        self: Box<Self>,
        mut on_clip: Box<dyn FnMut(Clip) + Send>,
        commands: Receiver<Command>,
        wake: OwnedFd,
    ) -> anyhow::Result<()> {
        let (conn, screen_num) = x11rb::connect(None).context("connecting to the X server")?;
        let root = conn.setup().roots[screen_num].root;
        let win = conn.generate_id()?;
        conn.create_window(
            COPY_DEPTH_FROM_PARENT,
            win,
            root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_OUTPUT,
            x11rb::COPY_FROM_PARENT,
            &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )?;
        let intern = |name: &[u8]| -> anyhow::Result<Atom> { Ok(conn.intern_atom(false, name)?.reply()?.atom) };
        let atoms = Atoms {
            clipboard: intern(b"CLIPBOARD")?,
            targets: intern(b"TARGETS")?,
            incr: intern(b"INCR")?,
            prop: intern(b"FORGEKIT_CLIP")?,
            utf8: intern(b"UTF8_STRING")?,
        };
        conn.xfixes_query_version(5, 0)?.reply().context("XFixes is not available")?;
        conn.xfixes_select_selection_input(win, atoms.clipboard, SelectionEventMask::SET_SELECTION_OWNER)?;
        conn.flush()?;

        let mut st = State { conn, win, atoms, fetch: None, retry: None, owned: None, attempts: 0 };
        loop {
            while let Some(ev) = st.conn.poll_for_event()? {
                if let Some(clip) = st.handle(ev)? {
                    on_clip(clip);
                }
            }
            st.conn.flush()?;
            if let Some((_, started)) = &st.fetch
                && started.elapsed() > Duration::from_secs(5)
            {
                st.fetch = None;
            }
            if let Some((at, n)) = st.retry
                && Instant::now() >= at
                && st.fetch.is_none()
            {
                st.retry = None;
                st.request(st.atoms.targets, Fetch::Targets)?;
                st.attempts = n;
            }
            let timeout = if st.retry.is_some() { 50 } else { 1000 };
            let ready = super::poll(&[st.conn.stream().as_raw_fd(), wake.as_raw_fd()], timeout)?;
            if ready[1] {
                super::drain(&wake);
                while let Ok(cmd) = commands.try_recv() {
                    match cmd {
                        Command::Stop => return Ok(()),
                        Command::Set(clip) => st.take_ownership(clip)?,
                    }
                }
            }
        }
    }
}

impl State {
    fn atom_name(&self, a: Atom) -> Option<String> {
        let r = self.conn.get_atom_name(a).ok()?.reply().ok()?;
        String::from_utf8(r.name).ok()
    }

    fn intern(&self, name: &str) -> anyhow::Result<Atom> {
        Ok(self.conn.intern_atom(false, name.as_bytes())?.reply()?.atom)
    }

    fn request(&mut self, target: Atom, fetch: Fetch) -> anyhow::Result<()> {
        self.conn.convert_selection(self.win, self.atoms.clipboard, target, self.atoms.prop, CURRENT_TIME)?;
        self.fetch = Some((fetch, Instant::now()));
        Ok(())
    }

    fn handle(&mut self, ev: Event) -> anyhow::Result<Option<Clip>> {
        match ev {
            Event::XfixesSelectionNotify(e) if e.selection == self.atoms.clipboard => {
                if e.owner != self.win && e.owner != NONE {
                    self.attempts = 0;
                    self.retry = None;
                    self.request(self.atoms.targets, Fetch::Targets)?;
                }
            }
            Event::SelectionNotify(e) if e.requestor == self.win => {
                if e.property == NONE {
                    if matches!(self.fetch, Some((Fetch::Targets, _))) && self.attempts < 4 {
                        self.retry = Some((Instant::now() + Duration::from_millis(250), self.attempts + 1));
                    }
                    self.fetch = None;
                    return Ok(None);
                }
                return self.on_selection_notify();
            }
            Event::PropertyNotify(e) if e.window == self.win && e.atom == self.atoms.prop => {
                if e.state == Property::NEW_VALUE
                    && let Some((Fetch::Incr { .. }, _)) = &self.fetch
                {
                    return self.on_incr_chunk();
                }
            }
            Event::SelectionRequest(e) => self.serve(e)?,
            Event::SelectionClear(e) if e.selection == self.atoms.clipboard => self.owned = None,
            _ => {}
        }
        Ok(None)
    }

    fn on_selection_notify(&mut self) -> anyhow::Result<Option<Clip>> {
        let Some((fetch, _)) = self.fetch.take() else { return Ok(None) };
        let reply = self.conn.get_property(true, self.win, self.atoms.prop, AtomEnum::ANY, 0, u32::MAX)?.reply()?;
        match fetch {
            Fetch::Targets => {
                let names: Vec<String> =
                    reply.value32().into_iter().flatten().filter_map(|a| self.atom_name(a)).collect();
                if let Some(mime) = super::choose_format(&names) {
                    let target = self.intern(&mime)?;
                    self.request(target, Fetch::Data { mime })?;
                }
                Ok(None)
            }
            Fetch::Data { mime } if reply.type_ == self.atoms.incr => {
                // Deleting the property (done above) tells the owner to start sending chunks.
                self.fetch = Some((Fetch::Incr { mime, buf: Vec::new() }, Instant::now()));
                Ok(None)
            }
            Fetch::Data { mime } => Ok(Some(Clip { mime: super::storage_mime(&mime), data: reply.value })),
            Fetch::Incr { .. } => Ok(None),
        }
    }

    fn on_incr_chunk(&mut self) -> anyhow::Result<Option<Clip>> {
        let reply = self.conn.get_property(true, self.win, self.atoms.prop, AtomEnum::ANY, 0, u32::MAX)?.reply()?;
        let Some((Fetch::Incr { mime, buf }, started)) = &mut self.fetch else { return Ok(None) };
        *started = Instant::now();
        if reply.value.is_empty() {
            let clip = Clip { mime: super::storage_mime(mime), data: std::mem::take(buf) };
            self.fetch = None;
            return Ok(Some(clip));
        }
        buf.extend_from_slice(&reply.value);
        Ok(None)
    }

    fn take_ownership(&mut self, clip: Clip) -> anyhow::Result<()> {
        self.conn.set_selection_owner(self.win, self.atoms.clipboard, CURRENT_TIME)?;
        self.owned = Some(clip);
        self.conn.flush()?;
        Ok(())
    }

    fn serve(&mut self, e: SelectionRequestEvent) -> anyhow::Result<()> {
        let property = if e.property == NONE { e.target } else { e.property };
        let mut ok = false;
        if let Some(clip) = &self.owned
            && e.selection == self.atoms.clipboard
        {
            let mut offered = vec![self.intern(&clip.mime)?];
            if !clip.is_image() {
                offered.extend([self.atoms.utf8, self.intern("text/plain")?, AtomEnum::STRING.into()]);
            }
            if e.target == self.atoms.targets {
                let mut list = vec![self.atoms.targets];
                list.extend(&offered);
                self.conn.change_property32(PropMode::REPLACE, e.requestor, property, AtomEnum::ATOM, &list)?;
                ok = true;
            } else if offered.contains(&e.target) {
                self.conn.change_property8(PropMode::REPLACE, e.requestor, property, e.target, &clip.data)?;
                ok = true;
            }
        }
        let notify = SelectionNotifyEvent {
            response_type: SELECTION_NOTIFY_EVENT,
            sequence: 0,
            time: e.time,
            requestor: e.requestor,
            selection: e.selection,
            target: e.target,
            property: if ok { property } else { NONE },
        };
        self.conn.send_event(false, e.requestor, EventMask::NO_EVENT, notify)?;
        self.conn.flush()?;
        Ok(())
    }
}
