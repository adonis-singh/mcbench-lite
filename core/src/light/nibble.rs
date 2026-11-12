// SWMRNibbleArray (moonrise's Starlight): one section's 4096 light values as
// nibbles, with the state Starlight keeps for it and the single-writer copy
// the light engine changes before it is made visible to readers.
//
// States: 0 null, 1 uninitialised, 2 initialised, 3 hidden.

pub const SIZE: usize = 2048;

pub const NULL: u8 = 0;
pub const UNINIT: u8 = 1;
pub const INIT: u8 = 2;
pub const HIDDEN: u8 = 3;

pub type Bytes = Box<[u8; SIZE]>;

fn zeroed() -> Bytes {
    Box::new([0; SIZE])
}

// storageUpdating: nothing, the visible array itself (as just after
// updateVisible), or an array of its own (a working copy).
enum Updating {
    None,
    Visible,
    Own(Bytes),
}

pub struct Nibble {
    state_updating: u8,
    state_visible: u8,
    updating: Updating,
    updating_dirty: bool,
    visible: Option<Bytes>,
}

// SWMRNibbleArray.SaveState.
pub struct SaveState {
    pub data: Option<Bytes>,
    pub state: u8,
}

impl Nibble {
    // new SWMRNibbleArray(null, isNullNibble).
    pub fn new(is_null: bool) -> Nibble {
        let state = if is_null { NULL } else { UNINIT };
        Nibble { state_updating: state, state_visible: state, updating: Updating::None, updating_dirty: false, visible: None }
    }

    // The working array, where it differs from the visible one.
    pub fn own_bytes(&self) -> Option<&[u8; SIZE]> {
        match &self.updating {
            Updating::Own(bytes) => Some(bytes),
            _ => None,
        }
    }

    // The nibble as McbenchEngine.state writes it (see Lighting::light_text).
    pub fn token(&self) -> String {
        let mut token = format!("{}{}{}", self.state_visible, self.state_updating, if self.updating_dirty { "d" } else { "" });
        if let Some(visible) = &self.visible {
            token.push_str(&format!(":{:016x}", fnv(&visible[..])));
        }
        if let Some(own) = self.own_bytes() {
            token.push_str(&format!("/{:016x}", fnv(own)));
        }
        token
    }

    pub fn save_state(&self) -> Option<SaveState> {
        match self.state_visible {
            NULL => None,
            UNINIT => Some(SaveState { data: None, state: UNINIT }),
            _ => {
                let data = self.visible.as_ref().expect("an initialised nibble holds data");
                if data.iter().all(|&b| b == 0) {
                    if self.state_visible == INIT {
                        Some(SaveState { data: None, state: UNINIT })
                    } else {
                        None
                    }
                } else {
                    Some(SaveState { data: Some(data.clone()), state: self.state_visible })
                }
            }
        }
    }

    // The bottom layer of the working array (what extrudeLower copies), or
    // none for a nibble with no working array.
    pub fn bottom_layer(&self) -> Option<[u8; 128]> {
        assert!(self.state_updating != NULL, "java.lang.IllegalArgumentException");
        self.updating_bytes().map(|bytes| {
            let mut layer = [0u8; 128];
            layer.copy_from_slice(&bytes[..128]);
            layer
        })
    }

    // extrudeLower(other): this nibble gets the bottom layer of `other` in
    // every layer.
    pub fn extrude_lower(&mut self, layer: Option<[u8; 128]>) {
        let Some(src) = layer else {
            self.set_uninitialised();
            return;
        };
        if !self.updating_dirty {
            if matches!(self.updating, Updating::None) {
                self.state_updating = INIT;
            }
            self.updating = Updating::Own(zeroed());
            self.updating_dirty = true;
        }
        let Updating::Own(into) = &mut self.updating else { unreachable!() };
        for y in 0..16 {
            into[y << 7..(y << 7) + 128].copy_from_slice(&src);
        }
    }

    fn updating_bytes(&self) -> Option<&[u8; SIZE]> {
        match &self.updating {
            Updating::None => None,
            Updating::Visible => Some(self.visible.as_ref().expect("a visible array")),
            Updating::Own(bytes) => Some(bytes),
        }
    }

    pub fn set_full(&mut self) {
        if self.state_updating != HIDDEN {
            self.state_updating = INIT;
        }
        self.updating = Updating::Own(Box::new([0xFF; SIZE]));
        self.updating_dirty = true;
    }

    pub fn set_non_null(&mut self) {
        if self.state_updating == HIDDEN {
            self.state_updating = INIT;
        } else if self.state_updating == NULL {
            self.state_updating = UNINIT;
        }
    }

    pub fn set_null(&mut self) {
        self.state_updating = NULL;
        self.updating = Updating::None;
        self.updating_dirty = false;
    }

    pub fn set_uninitialised(&mut self) {
        self.state_updating = UNINIT;
        self.updating = Updating::None;
        self.updating_dirty = false;
    }

    pub fn set_hidden(&mut self) {
        if self.state_updating != HIDDEN {
            if self.state_updating != INIT {
                self.set_null();
            } else {
                self.state_updating = HIDDEN;
            }
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.state_updating != self.state_visible || self.updating_dirty
    }

    pub fn is_null_updating(&self) -> bool {
        self.state_updating == NULL
    }

    pub fn is_null_visible(&self) -> bool {
        self.state_visible == NULL
    }

    pub fn is_initialised_updating(&self) -> bool {
        self.state_updating == INIT
    }

    fn swap_updating_and_mark_dirty(&mut self) {
        if !self.updating_dirty {
            let copy = match self.updating_bytes() {
                None => zeroed(),
                Some(bytes) => Box::new(*bytes),
            };
            self.updating = Updating::Own(copy);
            if self.state_updating != HIDDEN {
                self.state_updating = INIT;
            }
            self.updating_dirty = true;
        }
    }

    // updateVisible: whether anything was visible-changed.
    pub fn update_visible(&mut self) -> bool {
        if !self.is_dirty() {
            return false;
        }
        if self.state_updating != NULL && self.state_updating != UNINIT {
            match std::mem::replace(&mut self.updating, Updating::Visible) {
                Updating::None => panic!("java.lang.NullPointerException: no storage to make visible"),
                Updating::Visible => {}
                Updating::Own(bytes) => match &mut self.visible {
                    None => self.visible = Some(bytes),
                    Some(visible) => visible.copy_from_slice(&*bytes),
                },
            }
        } else {
            self.visible = None;
        }
        self.updating_dirty = false;
        self.state_visible = self.state_updating;
        true
    }

    // Where the working array's bytes are, for the light engine's loops,
    // which change nothing of a nibble but its values (Run::fast_table):
    // `read` is null for a nibble with no working array (it reads as 0),
    // `write` is null until the nibble's working array is its own (a first
    // write goes through set, which makes it so). Both stay valid while
    // nothing but set changes the nibble.
    pub fn bytes(&mut self) -> (*const u8, *mut u8) {
        match &mut self.updating {
            Updating::None => (std::ptr::null(), std::ptr::null_mut()),
            Updating::Visible => (self.visible.as_ref().expect("a visible array").as_ptr(), std::ptr::null_mut()),
            Updating::Own(bytes) => {
                let write = if self.updating_dirty { bytes.as_mut_ptr() } else { std::ptr::null_mut() };
                (bytes.as_ptr(), write)
            }
        }
    }

    #[inline]
    pub fn get_updating(&self, index: usize) -> i32 {
        match &self.updating {
            Updating::None => 0,
            Updating::Visible => (self.visible.as_ref().unwrap()[index >> 1] >> ((index & 1) << 2) & 15) as i32,
            Updating::Own(bytes) => (bytes[index >> 1] >> ((index & 1) << 2) & 15) as i32,
        }
    }

    #[inline]
    pub fn get_visible(&self, index: usize) -> i32 {
        match &self.visible {
            None => 0,
            Some(bytes) => (bytes[index >> 1] >> ((index & 1) << 2) & 15) as i32,
        }
    }

    #[inline]
    pub fn set(&mut self, index: usize, value: i32) {
        if !self.updating_dirty {
            self.swap_updating_and_mark_dirty();
        }
        let shift = (index & 1) << 2;
        let Updating::Own(bytes) = &mut self.updating else { unreachable!() };
        let at = index >> 1;
        // (byte)(old & (240 >>> shift) | value << shift)
        bytes[at] = (bytes[at] as u32 & (240 >> shift) | (value as u32) << shift) as u8;
    }
}

// FNV-1a over bytes, 64 bits.
pub fn fnv(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}
