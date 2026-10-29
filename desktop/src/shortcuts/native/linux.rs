//! X11 passive grabs with checked replies, repeat edges and connection teardown.
//! This is not a Wayland GlobalShortcuts portal and never claims that scope.
use super::Failure;
use cumulus_core::preferences::shortcuts::Chord;
use std::collections::BTreeMap;
use x11rb::{
    connection::Connection,
    errors::ReplyError,
    protocol::{
        ErrorKind, Event, xkb,
        xproto::{ConnectionExt, GrabMode, ModMask},
    },
    rust_connection::RustConnection,
};
use xkeysym::key;
struct Grab {
    keycode: u8,
    modifiers: u16,
    pressed: bool,
    await_release: bool,
}
pub(super) struct Backend {
    connection: RustConnection,
    root: u32,
    mapping: BTreeMap<u32, u8>,
    ignored: u16,
    variants: Vec<ModMask>,
    grabs: BTreeMap<u32, Grab>,
}
impl Backend {
    pub fn open() -> Result<Self, Failure> {
        // XWayland cannot receive keys while another native Wayland application
        // owns focus. Do not advertise its partial grab as a global shortcut.
        if std::env::var_os("WAYLAND_DISPLAY").is_some_and(|s| !s.is_empty())
            || std::env::var("XDG_SESSION_TYPE").is_ok_and(|s| s == "wayland")
        {
            return Err(Failure::Unavailable);
        }
        Self::connect()
    }
    fn connect() -> Result<Self, Failure> {
        let (connection, screen) =
            RustConnection::connect(None).map_err(|_| Failure::Unavailable)?;
        let reply = xkb::ConnectionExt::xkb_use_extension(&connection, 1, 0)
            .map_err(|_| Failure::Unavailable)?
            .reply()
            .map_err(|_| Failure::Unavailable)?;
        if !reply.supported {
            return Err(Failure::Unsupported);
        }
        let repeat = xkb::ConnectionExt::xkb_per_client_flags(
            &connection,
            xkb::ID::USE_CORE_KBD.into(),
            xkb::PerClientFlag::DETECTABLE_AUTO_REPEAT,
            xkb::PerClientFlag::DETECTABLE_AUTO_REPEAT,
            Default::default(),
            Default::default(),
            Default::default(),
        )
        .map_err(|_| Failure::Unavailable)?
        .reply()
        .map_err(|_| Failure::Unavailable)?;
        if !repeat
            .value
            .contains(xkb::PerClientFlag::DETECTABLE_AUTO_REPEAT)
        {
            return Err(Failure::Unsupported);
        }
        let setup = connection.setup();
        let root = setup.roots[screen].root;
        let first = setup.min_keycode;
        let keys = connection
            .get_keyboard_mapping(first, setup.max_keycode - first + 1)
            .map_err(|_| Failure::Unavailable)?
            .reply()
            .map_err(|_| Failure::Unavailable)?;
        let mut mapping = BTreeMap::new();
        if keys.keysyms_per_keycode == 0 {
            return Err(Failure::Unsupported);
        }
        for (offset, symbols) in keys
            .keysyms
            .chunks(usize::from(keys.keysyms_per_keycode))
            .enumerate()
        {
            for &symbol in symbols {
                mapping.entry(symbol).or_insert(first + offset as u8);
            }
        }
        let modifiers = connection
            .get_modifier_mapping()
            .map_err(|_| Failure::Unavailable)?
            .reply()
            .map_err(|_| Failure::Unavailable)?;
        let per_modifier = modifiers.keycodes.len() / 8;
        if per_modifier == 0 {
            return Err(Failure::Unsupported);
        }
        let locks: Vec<_> = [key::Caps_Lock, key::Num_Lock, key::Scroll_Lock]
            .iter()
            .filter_map(|k| mapping.get(k))
            .copied()
            .collect();
        let mut ignored = 0_u16;
        for (index, codes) in modifiers.keycodes.chunks(per_modifier).enumerate() {
            if codes.iter().any(|c| locks.contains(c)) {
                ignored |= 1 << index;
            }
        }
        // Modifier aliases with Ctrl/Shift/Alt make the chord ambiguous.
        if ignored
            & (u16::from(ModMask::CONTROL) | u16::from(ModMask::SHIFT) | u16::from(ModMask::M1))
            != 0
        {
            return Err(Failure::Unsupported);
        }
        let variants = (0..256_u16)
            .filter(|v| v & !ignored == 0)
            .map(ModMask::from)
            .collect();
        Ok(Self {
            connection,
            root,
            mapping,
            ignored,
            variants,
            grabs: BTreeMap::new(),
        })
    }
    pub fn register(&mut self, id: u32, chord: &Chord) -> Result<(), Failure> {
        let mut modifiers = 0;
        let mut symbol = None;
        for &value in chord.keys() {
            match value {
                16 => modifiers |= u16::from(ModMask::SHIFT),
                17 => modifiers |= u16::from(ModMask::CONTROL),
                18 => modifiers |= u16::from(ModMask::M1),
                value if symbol.is_none() => {
                    symbol = Some(keysym(value).ok_or(Failure::Unsupported)?)
                }
                _ => return Err(Failure::Unsupported),
            }
        }
        let keycode = *self
            .mapping
            .get(&symbol.ok_or(Failure::Unsupported)?)
            .ok_or(Failure::Unsupported)?;
        if self
            .grabs
            .values()
            .any(|g| g.keycode == keycode && g.modifiers == modifiers)
        {
            return Err(Failure::Conflict);
        }
        let held = self
            .connection
            .query_keymap()
            .map_err(|_| Failure::Fatal)?
            .reply()
            .map_err(|_| Failure::Fatal)?;
        for &variant in &self.variants {
            let result = self
                .connection
                .grab_key(
                    false,
                    self.root,
                    ModMask::from(modifiers) | variant,
                    keycode,
                    GrabMode::ASYNC,
                    GrabMode::ASYNC,
                )
                .map_err(|_| Failure::Fatal)?
                .check();
            if let Err(error) = result {
                self.ungrab(keycode, modifiers)?;
                return Err(match error {
                    ReplyError::X11Error(e) if e.error_kind == ErrorKind::Access => {
                        Failure::Conflict
                    }
                    _ => Failure::Fatal,
                });
            }
        }
        let pressed = held.keys[usize::from(keycode / 8)] & (1 << (keycode % 8)) != 0;
        self.grabs.insert(
            id,
            Grab {
                keycode,
                modifiers,
                // A rebind while a key is held must wait for the next physical press.
                pressed,
                await_release: pressed,
            },
        );
        Ok(())
    }
    fn ungrab(&self, keycode: u8, modifiers: u16) -> Result<(), Failure> {
        for &variant in &self.variants {
            self.connection
                .ungrab_key(keycode, self.root, ModMask::from(modifiers) | variant)
                .map_err(|_| Failure::Fatal)?
                .check()
                .map_err(|_| Failure::Fatal)?;
        }
        Ok(())
    }
    pub fn clear(&mut self) -> Result<(), Failure> {
        for grab in self.grabs.values() {
            self.ungrab(grab.keycode, grab.modifiers)?;
        }
        self.grabs.clear();
        Ok(())
    }
    pub fn poll(&mut self) -> Result<Vec<u32>, Failure> {
        let mut pressed = vec![];
        for _ in 0..512 {
            let Some(event) = self
                .connection
                .poll_for_event()
                .map_err(|_| Failure::Fatal)?
            else {
                break;
            };
            match event {
                Event::KeyPress(event) => {
                    let modifiers = u16::from(event.state) & 255 & !self.ignored;
                    for (&id, grab) in &mut self.grabs {
                        if grab.keycode == event.detail
                            && grab.modifiers == modifiers
                            && !grab.pressed
                        {
                            grab.pressed = true;
                            pressed.push(id);
                        }
                    }
                }
                Event::KeyRelease(event) => {
                    for grab in self
                        .grabs
                        .values_mut()
                        .filter(|g| g.keycode == event.detail)
                    {
                        grab.pressed = false;
                        grab.await_release = false;
                    }
                }
                // Existing grabs are based on a specific keymap. Explicit refresh
                // recreates the connection/mapping instead of silently retargeting.
                Event::MappingNotify(_) | Event::Error(_) => return Err(Failure::Fatal),
                _ => (),
            }
        }
        // A key pressed before this connection installed its passive grab can
        // be released without sending us KeyRelease. Query only those initial
        // held keys so the next physical press is not swallowed indefinitely.
        if self.grabs.values().any(|g| g.await_release) {
            let held = self
                .connection
                .query_keymap()
                .map_err(|_| Failure::Fatal)?
                .reply()
                .map_err(|_| Failure::Fatal)?;
            for grab in self.grabs.values_mut().filter(|g| g.await_release) {
                if held.keys[usize::from(grab.keycode / 8)] & (1 << (grab.keycode % 8)) == 0 {
                    grab.pressed = false;
                    grab.await_release = false;
                }
            }
        }
        Ok(pressed)
    }
}
fn keysym(vk: u16) -> Option<u32> {
    Some(match vk {
        48..=57 | 65..=90 => u32::from(vk),
        96..=105 => key::KP_0 + u32::from(vk - 96),
        112..=122 => key::F1 + u32::from(vk - 112),
        32 => key::space,
        33 => key::Page_Up,
        34 => key::Page_Down,
        35 => key::End,
        36 => key::Home,
        37 => key::Left,
        38 => key::Up,
        39 => key::Right,
        40 => key::Down,
        106 => key::KP_Multiply,
        107 => key::KP_Add,
        109 => key::KP_Subtract,
        110 => key::KP_Decimal,
        111 => key::KP_Divide,
        176 => key::XF86_AudioNext,
        177 => key::XF86_AudioPrev,
        178 => key::XF86_AudioStop,
        179 => key::XF86_AudioPlay,
        186 => key::semicolon,
        187 => key::equal,
        188 => key::comma,
        189 => key::minus,
        190 => key::period,
        191 => key::slash,
        192 => key::grave,
        219 => key::bracketleft,
        220 => key::backslash,
        221 => key::bracketright,
        222 => key::apostrophe,
        _ => return None,
    })
}
