//! The fonts the window draws with: the page's two stacks as each platform
//! resolves them, with the app's own last entry.
//!
//! The page's `--font-ui` and `--font-body` end in `system-ui, sans-serif`
//! and `--font-data` in `monospace`; a browser resolves those through the
//! system. The app does the same, in the process, and carries one sans and
//! one monospace family of its own as the last entry of each stack, so text
//! draws whatever fonts the system has, including none:
//!
//! | Stack | macOS | Windows | Linux | Last entry |
//! |---|---|---|---|---|
//! | sans (`ui`) | `.SystemUIFont` (SF Pro) | Segoe UI | fontconfig's match for `system-ui` | [`SANS`] |
//! | monospace (`data`) | `MAC_MONO` (SF Mono) | Consolas | fontconfig's match for `monospace` | [`MONO`] |
//!
//! The first entry of a stack that GPUI's text system loads itself
//! (`TextSystem::loads_font`: a family it has, with a face it can use as a
//! primary font) is the family; the embedded one always loads. [`SANS`] is
//! also the last entry of GPUI's own fallback stack (`push_fallback_font`),
//! which resolves `.SystemUIFont` and any family that cannot be loaded, so
//! no lookup can fail.
//!
//! On Linux the system's answer is fontconfig's, as Chromium resolves
//! `system-ui` and `monospace`: libfontconfig, opened at run time (it is
//! not linked), matches the pattern with the system's and the user's
//! configuration exactly as `fc-match` does. With no libfontconfig on the
//! system, or no font for it to match, the system gives no family and the
//! stack's last entry is used. GPUI's font database (cosmic-text's
//! `fontdb`) reads fontconfig's files too, but only for the folders and the
//! `<alias>` rules, and takes the first family of the last alias for a
//! generic name, installed or not: on a machine whose configuration assigns
//! the family in a `<match>` rule it names a different family than
//! fontconfig does (adipc: `Nimbus Sans Narrow` and `Nimbus Mono PS` where
//! fontconfig matches `Liberation Sans` and `JetBrainsMono Nerd Font`), and
//! it has no `system-ui` at all.
//!
//! The embedded families, both under the SIL Open Font License 1.1 (the
//! licence texts are beside the files in `assets/fonts/`), variable in
//! weight (vendor patch 13 draws every weight the stylesheet asks for):
//!
//! | Family | File | Bytes | sha256 of the file | From |
//! |---|---|---|---|---|
//! | Inter Variable 4.1 | `InterVariable.ttf` | 879,708 | `4989b125924991b90d05b2d16e0e388c48f7d5bb8b30539bbf9c755278d0ccaf` | `InterVariable.ttf` in <https://github.com/rsms/inter/releases/download/v4.1/Inter-4.1.zip> (sha256 `9883fdd4a49d4fb66bd8177ba6625ef9a64aa45899767dde3d36aa425756b11e`), with its `LICENSE.txt` |
//! | JetBrains Mono 2.304 | `JetBrainsMono-wght.ttf` | 303,144 | `662a196d58f1183bf2d77428b6d5283fe3f45161ab021bea4036bc98e5cac016` | `fonts/variable/JetBrainsMono[wght].ttf` in <https://github.com/JetBrains/JetBrainsMono/releases/download/v2.304/JetBrainsMono-2.304.zip> (sha256 `6f6376c6ed2960ea8a963cd7387ec9d76e3f629125bc33d1fdcd7eb7012f7bbf`), with its `OFL.txt` |
//!
//! Only the upright faces are embedded: italic text in an embedded family
//! draws upright.

use std::sync::OnceLock;

use gpui_kit::{App, SharedString, TextSystem, font};

use super::theme::Fonts;

/// The embedded sans family's name.
pub const SANS: &str = "Inter Variable";

/// The embedded monospace family's name.
pub const MONO: &str = "JetBrains Mono";

/// [`SANS`]'s file.
pub const SANS_FILE: &[u8] = include_bytes!("../../assets/fonts/InterVariable.ttf");

/// [`MONO`]'s file.
pub const MONO_FILE: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono-wght.ttf");

/// SF Mono's family as the system names it (`/System/Library/Fonts/SFNSMono.ttf`,
/// which Chromium's `ui-monospace` draws); the file is added to the text
/// system at start because a family whose name starts with a dot is not
/// listed.
#[cfg(target_os = "macos")]
pub const MAC_MONO: &str = ".SF NS Mono";

/// The file of [`MAC_MONO`].
#[cfg(target_os = "macos")]
pub const MAC_MONO_FILE: &str = "/System/Library/Fonts/SFNSMono.ttf";

/// Adds the embedded families to the text system, makes [`SANS`] the last
/// entry of GPUI's fallback stack, and returns the families the two stacks
/// resolve to. `setup` calls it first, before anything lays out text.
pub fn install(cx: &mut App) -> Fonts {
    let text = cx.text_system();
    add(text);
    let (ui, data) = system_families();
    Fonts {
        ui: first(text, ui.clone(), SANS),
        data: first(text, data.clone(), MONO),
    }
}

/// Adds the embedded families (and SF Mono on macOS) to `text` and makes
/// [`SANS`] the last entry of its fallback stack.
pub fn add(text: &TextSystem) {
    text.add_fonts(vec![SANS_FILE.into(), MONO_FILE.into()])
        .expect("the embedded fonts load");
    #[cfg(target_os = "macos")]
    {
        let mono = std::fs::read(MAC_MONO_FILE)
            .unwrap_or_else(|e| panic!("cannot read SF Mono ({MAC_MONO_FILE}): {e}"));
        text.add_fonts(vec![mono.into()]).expect("SF Mono loads");
    }
    text.push_fallback_font(font(SANS));
}

/// A stack of two entries: the system's family when `text` loads it, else
/// `last` (an embedded family, which always loads).
pub fn first(text: &TextSystem, system: Option<String>, last: &str) -> SharedString {
    system
        .filter(|family| text.loads_font(&font(family.clone())))
        .unwrap_or_else(|| last.to_string())
        .into()
}

/// The system's sans and monospace families (each the first entry of its
/// stack), as the module's table says; asked once per process.
pub fn system_families() -> &'static (Option<String>, Option<String>) {
    static FAMILIES: OnceLock<(Option<String>, Option<String>)> = OnceLock::new();
    FAMILIES.get_or_init(|| {
        #[cfg(target_os = "macos")]
        {
            (Some(".SystemUIFont".into()), Some(MAC_MONO.into()))
        }
        #[cfg(target_os = "windows")]
        {
            (Some("Segoe UI".into()), Some("Consolas".into()))
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            match fontconfig::Library::open() {
                Some(library) => (library.family("system-ui"), library.family("monospace")),
                None => (None, None),
            }
        }
    })
}

/// fontconfig's matching, in the process, through libfontconfig opened at
/// run time.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub mod fontconfig {
    use std::ffi::{CStr, CString, c_char, c_int, c_void};

    /// `FcMatchPattern`.
    const MATCH_PATTERN: c_int = 0;
    /// `FcResultMatch`.
    const RESULT_MATCH: c_int = 0;

    type InitLoadConfigAndFonts = unsafe extern "C" fn() -> *mut c_void;
    type NameParse = unsafe extern "C" fn(*const u8) -> *mut c_void;
    type ConfigSubstitute = unsafe extern "C" fn(*mut c_void, *mut c_void, c_int) -> c_int;
    type DefaultSubstitute = unsafe extern "C" fn(*mut c_void);
    type FontMatch = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_int) -> *mut c_void;
    type PatternGetString =
        unsafe extern "C" fn(*const c_void, *const c_char, c_int, *mut *const u8) -> c_int;
    type PatternDestroy = unsafe extern "C" fn(*mut c_void);

    /// libfontconfig with the system's configuration and fonts loaded.
    pub struct Library {
        config: *mut c_void,
        name_parse: NameParse,
        config_substitute: ConfigSubstitute,
        default_substitute: DefaultSubstitute,
        font_match: FontMatch,
        pattern_get_string: PatternGetString,
        pattern_destroy: PatternDestroy,
    }

    /// The function `name` of the library `handle`, as the function pointer
    /// type `F`.
    ///
    /// # Safety
    ///
    /// `F` is an `extern "C" fn` type matching the C signature of `name`.
    unsafe fn function<F: Copy>(handle: *mut c_void, name: &str) -> F {
        let c_name = CString::new(name).expect("a symbol name has no NUL");
        // SAFETY: `handle` is a library dlopen returned; dlsym only reads.
        let address = unsafe { libc::dlsym(handle, c_name.as_ptr()) };
        assert!(
            !address.is_null(),
            "libfontconfig.so.1 has no {name}: it is not fontconfig"
        );
        assert_eq!(size_of::<F>(), size_of::<*mut c_void>());
        // SAFETY: a non-null function address as the pointer type the
        // caller vouches for.
        unsafe { std::mem::transmute_copy::<*mut c_void, F>(&address) }
    }

    impl Library {
        /// libfontconfig.so.1 with the configuration and the fonts loaded
        /// (`FcInitLoadConfigAndFonts`, as `fc-match` starts), or `None` when
        /// the system has no libfontconfig or its configuration cannot be
        /// loaded. The library stays loaded for the process's life.
        pub fn open() -> Option<Library> {
            // SAFETY: dlopen of a library by name; a missing one is null.
            let handle = unsafe {
                libc::dlopen(
                    c"libfontconfig.so.1".as_ptr(),
                    libc::RTLD_NOW | libc::RTLD_LOCAL,
                )
            };
            if handle.is_null() {
                return None;
            }
            // SAFETY: each function's type is its C signature (fontconfig.h).
            unsafe {
                let init: InitLoadConfigAndFonts = function(handle, "FcInitLoadConfigAndFonts");
                let config = init();
                if config.is_null() {
                    return None;
                }
                Some(Library {
                    config,
                    name_parse: function(handle, "FcNameParse"),
                    config_substitute: function(handle, "FcConfigSubstitute"),
                    default_substitute: function(handle, "FcDefaultSubstitute"),
                    font_match: function(handle, "FcFontMatch"),
                    pattern_get_string: function(handle, "FcPatternGetString"),
                    pattern_destroy: function(handle, "FcPatternDestroy"),
                })
            }
        }

        /// The first family of the font fontconfig matches for `pattern`
        /// (`fc-match -f '%{family[0]}' <pattern>`), or `None` when it
        /// matches none (no fonts).
        pub fn family(&self, pattern: &str) -> Option<String> {
            let c_pattern = CString::new(pattern).expect("a pattern has no NUL");
            // SAFETY: the calls of fc-match's own sequence on a pattern this
            // function owns and destroys; the family string belongs to the
            // matched pattern and is copied before that is destroyed.
            unsafe {
                let query = (self.name_parse)(c_pattern.as_ptr().cast());
                assert!(!query.is_null(), "fontconfig cannot parse {pattern}");
                (self.config_substitute)(self.config, query, MATCH_PATTERN);
                (self.default_substitute)(query);
                let mut result = RESULT_MATCH;
                let matched = (self.font_match)(self.config, query, &mut result);
                (self.pattern_destroy)(query);
                if matched.is_null() {
                    return None;
                }
                let mut family: *const u8 = std::ptr::null();
                let found = (self.pattern_get_string)(matched, c"family".as_ptr(), 0, &mut family);
                let name = (found == RESULT_MATCH && !family.is_null())
                    .then(|| CStr::from_ptr(family.cast()).to_string_lossy().into_owned());
                (self.pattern_destroy)(matched);
                name
            }
        }
    }
}
