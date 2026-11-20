//! uia-probe: a UI Automation (COM client) probe for checking mcbench-ui on Windows.
//!
//! Runs in the interactive desktop session (UI Automation does not cross sessions; see
//! ../README.md for how the scripts start it there). Every command finds the target by
//! process name and fails with a clear message when the process, window or element is missing.
//!
//! ```text
//! uia-probe dump    PROC [raw|control|content] [--stable]      the tree with properties and patterns
//! uia-probe text    PROC SELECTOR [char|word|line|paragraph]    Text pattern: document, units, caret, selection
//! uia-probe act     PROC SELECTOR ACTION [ARG] [--key K] [--wait MS]  invoke|toggle|expand|collapse|select|setvalue|setrange|focus|scrollinto
//! uia-probe focused                                             the element with keyboard focus
//! uia-probe events  PROC SECONDS                                record UIA events from the process's window
//! uia-probe keys    PROC DELAY_MS KEY... [--shots DIR]          SendInput each key, print the focused element after each
//! uia-probe shot    OUT.bmp                                     the whole screen, 32-bit BMP
//! uia-probe table   PROC SELECTOR                               Grid.GetItem for every cell, Table headers, each cell's header items
//! uia-probe launch  EXE [ARGS...]                               ms to a visible window and to its first drawn pixels
//! uia-probe keylat  PROC N                                      ms from SendInput of a key to the composer's pixels changing
//! ```
//!
//! SELECTOR is `key=value[&key=value...][#n]` with keys `id` (AutomationId), `name`, `name~` and `value~` (substrings of the name or the Value pattern's value), `type`
//! (ControlType name, e.g. Button) and `#n` the n-th match (from 0) in raw-view document order.

#![allow(non_upper_case_globals)]

use std::fmt::Write as _;
use std::io::Write as _;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DIB_RGB_COLORS,
    DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, SAFEARRAY,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetSystemMetrics, GetWindowTextW, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    SetForegroundWindow,
};
use windows::core::{BSTR, Interface, Result};
use windows_core::{Ref, implement};

static OUT: Mutex<()> = Mutex::new(());

fn say(line: &str) {
    let _g = OUT.lock().unwrap();
    let mut o = std::io::stdout().lock();
    writeln!(o, "{line}").unwrap();
    o.flush().unwrap();
}

fn die(msg: &str) -> ! {
    eprintln!("uia-probe: {msg}");
    std::process::exit(2)
}

// ControlType ids 50000..=50040, in the order of UIAutomationClient.h.
const CONTROL_TYPES: [&str; 41] = [
    "Button", "Calendar", "CheckBox", "ComboBox", "Edit", "Hyperlink", "Image", "ListItem", "List", "Menu",
    "MenuBar", "MenuItem", "ProgressBar", "RadioButton", "ScrollBar", "Slider", "Spinner", "StatusBar", "Tab",
    "TabItem", "Text", "ToolBar", "ToolTip", "Tree", "TreeItem", "Custom", "Group", "Thumb", "DataGrid",
    "DataItem", "Document", "SplitButton", "Window", "Pane", "Header", "HeaderItem", "Table", "TitleBar",
    "Separator", "SemanticZoom", "AppBar",
];

fn control_type_name(id: UIA_CONTROLTYPE_ID) -> String {
    let i = id.0 - 50000;
    if (0..41).contains(&i) { CONTROL_TYPES[i as usize].to_string() } else { format!("ControlType({})", id.0) }
}

fn control_type_id(name: &str) -> i32 {
    match CONTROL_TYPES.iter().position(|n| n.eq_ignore_ascii_case(name)) {
        Some(i) => 50000 + i as i32,
        None => die(&format!("unknown control type {name:?}")),
    }
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(o, "\\u{{{:x}}}", c as u32);
            }
            c => o.push(c),
        }
    }
    o
}

fn variant_text(v: &VARIANT) -> String {
    // windows 0.62 hands back the raw VARIANT; read the few types UIA properties use.
    unsafe {
        let inner = &v.Anonymous.Anonymous;
        let u = &inner.Anonymous;
        match inner.vt.0 {
            0 => "empty".into(),
            8 => format!("\"{}\"", esc(&u.bstrVal.to_string())),
            11 => (u.boolVal.0 != 0).to_string(),
            2 => u.iVal.to_string(),
            3 | 22 => u.lVal.to_string(),
            19 | 23 => u.ulVal.to_string(),
            5 => u.dblVal.to_string(),
            4 => u.fltVal.to_string(),
            13 => "unknown-object".into(),
            vt => format!("vt={vt:#x}"),
        }
    }
}

struct Probe {
    uia: IUIAutomation,
}

/// The element's pattern, or a clear error when the element does not offer it.
fn pattern<T: Interface>(el: &IUIAutomationElement, id: UIA_PATTERN_ID, action: &str) -> T {
    match unsafe { el.GetCurrentPatternAs::<T>(id) } {
        Ok(p) => p,
        Err(_) => die(&format!("{action}: the element does not offer pattern {}", id.0)),
    }
}

impl Probe {
    fn new() -> Probe {
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            CoInitializeEx(None, COINIT_MULTITHREADED).ok().expect("CoInitializeEx");
            let uia: IUIAutomation =
                CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER).expect("CoCreateInstance(CUIAutomation8)");
            Probe { uia }
        }
    }

    /// The top-level UIA element of the first window of the process named `proc` (without .exe).
    fn window(&self, proc: &str) -> IUIAutomationElement {
        let pids = pids_of(proc);
        if pids.is_empty() {
            die(&format!("no process named {proc}"));
        }
        unsafe {
            let root = self.uia.GetRootElement().unwrap();
            let walker = self.uia.RawViewWalker().unwrap();
            let mut ch = walker.GetFirstChildElement(&root).ok();
            while let Some(el) = ch {
                if pids.contains(&(el.CurrentProcessId().unwrap() as u32)) {
                    return el;
                }
                ch = walker.GetNextSiblingElement(&el).ok();
            }
        }
        die(&format!("process {proc} ({pids:?}) has no top-level window in UIA"))
    }

    fn walker(&self, view: &str) -> IUIAutomationTreeWalker {
        unsafe {
            match view {
                "raw" => self.uia.RawViewWalker().unwrap(),
                "control" => self.uia.ControlViewWalker().unwrap(),
                "content" => self.uia.ContentViewWalker().unwrap(),
                v => die(&format!("unknown view {v:?} (raw, control, content)")),
            }
        }
    }

    fn describe(&self, el: &IUIAutomationElement, origin: Option<(i32, i32)>) -> String {
        let mut s = String::new();
        unsafe {
            let ct = el.CurrentControlType().unwrap();
            let _ = write!(s, "{}", control_type_name(ct));
            let _ = write!(s, " name=\"{}\"", esc(&el.CurrentName().unwrap_or_default().to_string()));
            let _ = write!(s, " lct=\"{}\"", esc(&el.CurrentLocalizedControlType().unwrap_or_default().to_string()));
            let _ = write!(s, " id=\"{}\"", esc(&el.CurrentAutomationId().unwrap_or_default().to_string()));
            let help = el.CurrentHelpText().unwrap_or_default().to_string();
            if !help.is_empty() {
                let _ = write!(s, " help=\"{}\"", esc(&help));
            }
            let class = el.CurrentClassName().unwrap_or_default().to_string();
            if !class.is_empty() {
                let _ = write!(s, " class=\"{}\"", esc(&class));
            }
            let fw = el.CurrentFrameworkId().unwrap_or_default().to_string();
            if !fw.is_empty() {
                let _ = write!(s, " fw=\"{}\"", esc(&fw));
            }
            let b = |r: Result<windows::core::BOOL>| if r.map(|b| b.as_bool()).unwrap_or(false) { 1 } else { 0 };
            let _ = write!(
                s,
                " enabled={} kbfocusable={} focus={} offscreen={} ctl={} content={}",
                b(el.CurrentIsEnabled()),
                b(el.CurrentIsKeyboardFocusable()),
                b(el.CurrentHasKeyboardFocus()),
                b(el.CurrentIsOffscreen()),
                b(el.CurrentIsControlElement()),
                b(el.CurrentIsContentElement()),
            );
            let r = el.CurrentBoundingRectangle().unwrap_or_default();
            match origin {
                Some((ox, oy)) => {
                    let _ = write!(s, " rel=[{},{},{}x{}]", r.left - ox, r.top - oy, r.right - r.left, r.bottom - r.top);
                }
                None => {
                    let _ = write!(s, " rect=[{},{},{}x{}]", r.left, r.top, r.right - r.left, r.bottom - r.top);
                }
            }
            if let Ok(e8) = el.cast::<IUIAutomationElement8>() {
                let hl = e8.CurrentHeadingLevel().unwrap_or_default();
                if hl.0 != HeadingLevel_None.0 {
                    let _ = write!(s, " heading={}", hl.0 - HeadingLevel_None.0);
                }
            }
            if let Ok(e4) = el.cast::<IUIAutomationElement4>() {
                let pos = e4.CurrentPositionInSet().unwrap_or(0);
                let size = e4.CurrentSizeOfSet().unwrap_or(0);
                let level = e4.CurrentLevel().unwrap_or(0);
                if pos > 0 || size > 0 {
                    let _ = write!(s, " set={pos}/{size}");
                }
                if level > 0 {
                    let _ = write!(s, " level={level}");
                }
            }
            for (pid, label) in [
                (UIA_LiveSettingPropertyId, "live"),
                (UIA_ItemStatusPropertyId, "status"),
                (UIA_AcceleratorKeyPropertyId, "accel"),
                (UIA_AccessKeyPropertyId, "accesskey"),
                (UIA_FullDescriptionPropertyId, "description"),
            ] {
                if let Ok(v) = el.GetCurrentPropertyValue(pid) {
                    let t = variant_text(&v);
                    if t != "\"\"" && t != "0" && t != "empty" && t != "unknown-object" {
                        let _ = write!(s, " {label}={t}");
                    }
                }
            }
            s.push_str(&self.patterns(el));
        }
        s
    }

    fn patterns(&self, el: &IUIAutomationElement) -> String {
        let mut names = Vec::new();
        let mut vals = String::new();
        unsafe {
            for id in 10000..=10045 {
                let pid = UIA_PATTERN_ID(id);
                let Ok(unk) = el.GetCurrentPattern(pid) else { continue };
                let _ = &unk;
                let name = self.uia.GetPatternProgrammaticName(pid).map(|b| b.to_string()).unwrap_or(format!("{id}"));
                names.push(name.trim_end_matches("Pattern").to_string());
                match pid {
                    UIA_InvokePatternId => {}
                    UIA_ExpandCollapsePatternId => {
                        let p: IUIAutomationExpandCollapsePattern = unk.cast().unwrap();
                        let st = p.CurrentExpandCollapseState().unwrap();
                        let st = match st {
                            ExpandCollapseState_Collapsed => "Collapsed",
                            ExpandCollapseState_Expanded => "Expanded",
                            ExpandCollapseState_PartiallyExpanded => "PartiallyExpanded",
                            ExpandCollapseState_LeafNode => "LeafNode",
                            _ => "?",
                        };
                        let _ = write!(vals, " expand={st}");
                    }
                    UIA_TogglePatternId => {
                        let p: IUIAutomationTogglePattern = unk.cast().unwrap();
                        let st = match p.CurrentToggleState().unwrap() {
                            ToggleState_Off => "Off",
                            ToggleState_On => "On",
                            ToggleState_Indeterminate => "Indeterminate",
                            _ => "?",
                        };
                        let _ = write!(vals, " toggle={st}");
                    }
                    UIA_SelectionItemPatternId => {
                        let p: IUIAutomationSelectionItemPattern = unk.cast().unwrap();
                        let _ = write!(vals, " selected={}", p.CurrentIsSelected().unwrap().as_bool() as u8);
                    }
                    UIA_SelectionPatternId => {
                        let p: IUIAutomationSelectionPattern = unk.cast().unwrap();
                        let _ = write!(
                            vals,
                            " multiselect={} selectionrequired={}",
                            p.CurrentCanSelectMultiple().unwrap().as_bool() as u8,
                            p.CurrentIsSelectionRequired().unwrap().as_bool() as u8
                        );
                    }
                    UIA_ValuePatternId => {
                        let p: IUIAutomationValuePattern = unk.cast().unwrap();
                        let _ = write!(
                            vals,
                            " value=\"{}\" readonly={}",
                            esc(&p.CurrentValue().unwrap_or_default().to_string()),
                            p.CurrentIsReadOnly().unwrap().as_bool() as u8
                        );
                    }
                    UIA_RangeValuePatternId => {
                        let p: IUIAutomationRangeValuePattern = unk.cast().unwrap();
                        let _ = write!(
                            vals,
                            " range={} min={} max={} step={} readonly={}",
                            p.CurrentValue().unwrap(),
                            p.CurrentMinimum().unwrap(),
                            p.CurrentMaximum().unwrap(),
                            p.CurrentSmallChange().unwrap(),
                            p.CurrentIsReadOnly().unwrap().as_bool() as u8
                        );
                    }
                    UIA_TextPatternId => {
                        let p: IUIAutomationTextPattern = unk.cast().unwrap();
                        let doc = p.DocumentRange().unwrap();
                        let t = doc.GetText(200).unwrap_or_default().to_string();
                        let _ = write!(vals, " text=\"{}\"", esc(&t));
                    }
                    UIA_GridPatternId => {
                        let p: IUIAutomationGridPattern = unk.cast().unwrap();
                        let _ = write!(vals, " grid={}x{}", p.CurrentRowCount().unwrap(), p.CurrentColumnCount().unwrap());
                    }
                    UIA_GridItemPatternId => {
                        let p: IUIAutomationGridItemPattern = unk.cast().unwrap();
                        let _ = write!(
                            vals,
                            " cell=r{}c{} span={}x{}",
                            p.CurrentRow().unwrap(),
                            p.CurrentColumn().unwrap(),
                            p.CurrentRowSpan().unwrap(),
                            p.CurrentColumnSpan().unwrap()
                        );
                    }
                    UIA_TablePatternId => {
                        let p: IUIAutomationTablePattern = unk.cast().unwrap();
                        let _ = write!(vals, " major={}", p.CurrentRowOrColumnMajor().unwrap().0);
                    }
                    UIA_ScrollPatternId => {
                        let p: IUIAutomationScrollPattern = unk.cast().unwrap();
                        let _ = write!(
                            vals,
                            " vscroll={}:{:.1}% hscroll={}:{:.1}%",
                            p.CurrentVerticallyScrollable().unwrap().as_bool() as u8,
                            p.CurrentVerticalScrollPercent().unwrap(),
                            p.CurrentHorizontallyScrollable().unwrap().as_bool() as u8,
                            p.CurrentHorizontalScrollPercent().unwrap()
                        );
                    }
                    _ => {}
                }
            }
        }
        format!(" patterns=[{}]{}", names.join(","), vals)
    }

    fn dump(&self, proc: &str, view: &str, stable: bool) {
        let win = self.window(proc);
        let walker = self.walker(view);
        let r = unsafe { win.CurrentBoundingRectangle().unwrap() };
        let origin = if stable { Some((r.left, r.top)) } else { None };
        let mut count = 0usize;
        self.dump_rec(&walker, &win, 0, origin, &mut count);
        say(&format!("elements: {count} (view {view})"));
    }

    fn dump_rec(
        &self,
        walker: &IUIAutomationTreeWalker,
        el: &IUIAutomationElement,
        depth: usize,
        origin: Option<(i32, i32)>,
        count: &mut usize,
    ) {
        *count += 1;
        say(&format!("{}{}", "  ".repeat(depth), self.describe(el, origin)));
        unsafe {
            let mut ch = walker.GetFirstChildElement(el).ok();
            while let Some(c) = ch {
                self.dump_rec(walker, &c, depth + 1, origin, count);
                ch = walker.GetNextSiblingElement(&c).ok();
            }
        }
    }

    fn find(&self, proc: &str, selector: &str) -> IUIAutomationElement {
        let (conds, nth) = match selector.rsplit_once('#') {
            Some((c, n)) => (c, n.parse::<usize>().unwrap_or_else(|_| die(&format!("bad #n in {selector:?}")))),
            None => (selector, 0),
        };
        let mut want: Vec<(String, String)> = Vec::new();
        for part in conds.split('&') {
            let (k, v) = part.split_once('=').unwrap_or_else(|| die(&format!("bad selector part {part:?}")));
            if !matches!(k, "id" | "name" | "name~" | "value~" | "type") {
                die(&format!("unknown selector key {k:?} (id, name, name~ and value~ for a substring, type)"));
            }
            want.push((k.to_string(), v.to_string()));
        }
        let win = self.window(proc);
        let walker = self.walker("raw");
        let mut hits = Vec::new();
        self.find_rec(&walker, &win, &want, &mut hits);
        match hits.into_iter().nth(nth) {
            Some(e) => e,
            None => die(&format!("no element matches {selector:?} in {proc}")),
        }
    }

    fn find_rec(
        &self,
        walker: &IUIAutomationTreeWalker,
        el: &IUIAutomationElement,
        want: &[(String, String)],
        hits: &mut Vec<IUIAutomationElement>,
    ) {
        unsafe {
            let ok = want.iter().all(|(k, v)| match k.as_str() {
                "id" => el.CurrentAutomationId().unwrap_or_default() == v.as_str(),
                "name" => el.CurrentName().unwrap_or_default() == v.as_str(),
                "name~" => el.CurrentName().unwrap_or_default().to_string().contains(v.as_str()),
                "value~" => el
                    .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                    .and_then(|p| p.CurrentValue())
                    .is_ok_and(|v2| v2.to_string().contains(v.as_str())),
                "type" => el.CurrentControlType().unwrap().0 == control_type_id(v),
                _ => unreachable!(),
            });
            if ok {
                hits.push(el.clone());
            }
            let mut ch = walker.GetFirstChildElement(el).ok();
            while let Some(c) = ch {
                self.find_rec(walker, &c, want, hits);
                ch = walker.GetNextSiblingElement(&c).ok();
            }
        }
    }

    fn text(&self, proc: &str, selector: &str, unit: &str) {
        let el = self.find(proc, selector);
        say(&format!("element: {}", self.describe(&el, None)));
        let unit_v = match unit {
            "char" => TextUnit_Character,
            "word" => TextUnit_Word,
            "line" => TextUnit_Line,
            "paragraph" => TextUnit_Paragraph,
            "format" => TextUnit_Format,
            u => die(&format!("unknown unit {u:?} (char, word, line, paragraph, format)")),
        };
        unsafe {
            let tp: IUIAutomationTextPattern = el
                .GetCurrentPatternAs(UIA_TextPatternId)
                .unwrap_or_else(|e| die(&format!("element has no Text pattern: {e}")));
            let doc = tp.DocumentRange().unwrap();
            let all = doc.GetText(-1).unwrap().to_string();
            say(&format!("document: \"{}\" ({} UTF-16 units)", esc(&all), all.encode_utf16().count()));
            say(&format!("supported selection: {}", tp.SupportedTextSelection().unwrap().0));
            // Walk by unit from the start.
            let r = doc.Clone().unwrap();
            r.MoveEndpointByRange(TextPatternRangeEndpoint_End, &r, TextPatternRangeEndpoint_Start).unwrap();
            let mut i = 0;
            loop {
                let u = r.Clone().unwrap();
                u.ExpandToEnclosingUnit(unit_v).unwrap();
                let t = u.GetText(-1).unwrap().to_string();
                say(&format!("{unit}[{i}] = \"{}\"", esc(&t)));
                i += 1;
                let moved = r.Move(unit_v, 1).unwrap();
                // Moving onto the document's end leaves a degenerate range there: stop.
                let at_end =
                    r.CompareEndpoints(TextPatternRangeEndpoint_Start, &doc, TextPatternRangeEndpoint_End).unwrap() >= 0;
                if moved == 0 || at_end || i >= 2000 {
                    break;
                }
            }
            say(&format!("{unit} count: {i}"));
            // Selection and caret, as offsets in UTF-16 units from the document start.
            let sel = tp.GetSelection().unwrap();
            for k in 0..sel.Length().unwrap() {
                let s = sel.GetElement(k).unwrap();
                let (a, b) = self.offsets(&doc, &s);
                say(&format!("selection[{k}] = {a}..{b} \"{}\"", esc(&s.GetText(-1).unwrap().to_string())));
            }
            if let Ok(tp2) = el.GetCurrentPatternAs::<IUIAutomationTextPattern2>(UIA_TextPattern2Id) {
                let mut active = windows::core::BOOL(0);
                let c = tp2.GetCaretRange(&mut active).unwrap();
                let (a, _) = self.offsets(&doc, &c);
                say(&format!("caret = {a} active={}", active.as_bool() as u8));
            } else {
                say("caret: no TextPattern2");
            }
        }
    }

    fn short(&self, el: &IUIAutomationElement) -> String {
        unsafe {
            let v = el
                .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                .and_then(|p| p.CurrentValue())
                .map(|b| b.to_string())
                .unwrap_or_default();
            format!("{} \"{}\"", control_type_name(el.CurrentControlType().unwrap()), esc(&v))
        }
    }

    fn array(&self, a: &IUIAutomationElementArray) -> String {
        unsafe {
            let n = a.Length().unwrap();
            (0..n).map(|i| self.short(&a.GetElement(i).unwrap())).collect::<Vec<_>>().join(", ")
        }
    }

    fn table(&self, proc: &str, selector: &str) {
        let el = self.find(proc, selector);
        unsafe {
            let grid: IUIAutomationGridPattern = pattern(&el, UIA_GridPatternId, "table");
            let table: IUIAutomationTablePattern = pattern(&el, UIA_TablePatternId, "table");
            let (rows, columns) = (grid.CurrentRowCount().unwrap(), grid.CurrentColumnCount().unwrap());
            say(&format!("grid {rows} x {columns}, row or column major {}", table.CurrentRowOrColumnMajor().unwrap().0));
            say(&format!("column headers: [{}]", self.array(&table.GetCurrentColumnHeaders().unwrap())));
            say(&format!("row headers: [{}]", self.array(&table.GetCurrentRowHeaders().unwrap())));
            for r in 0..rows {
                for c in 0..columns {
                    let cell = grid.GetItem(r, c).unwrap();
                    let item: IUIAutomationGridItemPattern = pattern(&cell, UIA_GridItemPatternId, "table");
                    let ti: IUIAutomationTableItemPattern = pattern(&cell, UIA_TableItemPatternId, "table");
                    let same = self.uia.CompareElements(&item.CurrentContainingGrid().unwrap(), &el).unwrap().as_bool();
                    say(&format!(
                        "GetItem({r},{c}) = {} at r{}c{}, containing grid is the table: {same}, column header items [{}], row header items [{}]",
                        self.short(&cell),
                        item.CurrentRow().unwrap(),
                        item.CurrentColumn().unwrap(),
                        self.array(&ti.GetCurrentColumnHeaderItems().unwrap()),
                        self.array(&ti.GetCurrentRowHeaderItems().unwrap())
                    ));
                }
            }
        }
    }

    fn offsets(&self, doc: &IUIAutomationTextRange, r: &IUIAutomationTextRange) -> (usize, usize) {
        unsafe {
            let pre = doc.Clone().unwrap();
            pre.MoveEndpointByRange(TextPatternRangeEndpoint_End, r, TextPatternRangeEndpoint_Start).unwrap();
            let a = pre.GetText(-1).unwrap().to_string().encode_utf16().count();
            let len = r.GetText(-1).unwrap().to_string().encode_utf16().count();
            (a, a + len)
        }
    }

    fn act(&self, proc: &str, selector: &str, action: &str, arg: Option<&str>) {
        let el = self.find(proc, selector);
        say(&format!("before: {}", self.describe(&el, None)));
        unsafe {
            match action {
                "invoke" => pattern::<IUIAutomationInvokePattern>(&el, UIA_InvokePatternId, action).Invoke(),
                "toggle" => pattern::<IUIAutomationTogglePattern>(&el, UIA_TogglePatternId, action).Toggle(),
                "expand" => pattern::<IUIAutomationExpandCollapsePattern>(&el, UIA_ExpandCollapsePatternId, action)
                    .Expand(),
                "collapse" => pattern::<IUIAutomationExpandCollapsePattern>(&el, UIA_ExpandCollapsePatternId, action)
                    .Collapse(),
                "select" => pattern::<IUIAutomationSelectionItemPattern>(&el, UIA_SelectionItemPatternId, action)
                    .Select(),
                "setvalue" => pattern::<IUIAutomationValuePattern>(&el, UIA_ValuePatternId, action)
                    .SetValue(&BSTR::from(arg.unwrap_or_else(|| die("setvalue needs a value")))),
                "setrange" => pattern::<IUIAutomationRangeValuePattern>(&el, UIA_RangeValuePatternId, action)
                    .SetValue(arg.unwrap_or_else(|| die("setrange needs a number")).parse().unwrap()),
                "focus" => el.SetFocus(),
                "scrollinto" => pattern::<IUIAutomationScrollItemPattern>(&el, UIA_ScrollItemPatternId, action)
                    .ScrollIntoView(),
                a => die(&format!("unknown action {a:?}")),
            }
            .unwrap_or_else(|e| die(&format!("{action} failed: {e}")));
        }
        std::thread::sleep(Duration::from_millis(400));
        say(&format!("after:  {}", self.describe(&self.find(proc, selector), None)));
    }

    fn focused_line(&self) -> String {
        unsafe {
            let fg = GetForegroundWindow();
            let mut buf = [0u16; 256];
            let n = GetWindowTextW(fg, &mut buf);
            let title = String::from_utf16_lossy(&buf[..n as usize]);
            match self.uia.GetFocusedElement() {
                Ok(el) => format!("{} [foreground \"{}\"]", self.describe(&el, None), esc(&title)),
                Err(e) => format!("no focused element: {e} [foreground \"{}\"]", esc(&title)),
            }
        }
    }

    fn keys(&self, proc: &str, delay_ms: u64, keys: &[String], shots: Option<&str>) {
        let win = self.window(proc);
        unsafe {
            let hwnd = HWND(win.CurrentNativeWindowHandle().unwrap().0);
            let _ = SetForegroundWindow(hwnd);
        }
        std::thread::sleep(Duration::from_millis(delay_ms));
        say(&format!("start: {}", self.focused_line()));
        for (i, k) in keys.iter().enumerate() {
            send_combo(k);
            std::thread::sleep(Duration::from_millis(delay_ms));
            say(&format!("key[{i}] {k}: {}", self.focused_line()));
            if let Some(dir) = shots {
                shot(&format!("{dir}\\key{i:02}.bmp"));
            }
        }
    }

    fn events(&self, proc: &str, seconds: u64) {
        let win = self.window(proc);
        let t0 = Instant::now();
        unsafe {
            let focus: IUIAutomationFocusChangedEventHandler = Handler { t0, probe: self.uia.clone() }.into();
            self.uia.AddFocusChangedEventHandler(None, &focus).unwrap();
            let props: IUIAutomationPropertyChangedEventHandler = Handler { t0, probe: self.uia.clone() }.into();
            let ids = [
                UIA_NamePropertyId,
                UIA_ValueValuePropertyId,
                UIA_ExpandCollapseExpandCollapseStatePropertyId,
                UIA_ToggleToggleStatePropertyId,
                UIA_SelectionItemIsSelectedPropertyId,
                UIA_IsEnabledPropertyId,
                UIA_HasKeyboardFocusPropertyId,
                UIA_RangeValueValuePropertyId,
                UIA_IsOffscreenPropertyId,
            ];
            self.uia
                .AddPropertyChangedEventHandlerNativeArray(&win, TreeScope_Subtree, None, &props, &ids)
                .unwrap();
            let structure: IUIAutomationStructureChangedEventHandler = Handler { t0, probe: self.uia.clone() }.into();
            self.uia.AddStructureChangedEventHandler(&win, TreeScope_Subtree, None, &structure).unwrap();
            let auto: IUIAutomationEventHandler = Handler { t0, probe: self.uia.clone() }.into();
            for ev in [
                UIA_Text_TextChangedEventId,
                UIA_Text_TextSelectionChangedEventId,
                UIA_LiveRegionChangedEventId,
                UIA_Invoke_InvokedEventId,
                UIA_SelectionItem_ElementSelectedEventId,
                UIA_MenuOpenedEventId,
                UIA_MenuClosedEventId,
                UIA_Window_WindowOpenedEventId,
                UIA_Window_WindowClosedEventId,
            ] {
                self.uia.AddAutomationEventHandler(ev, &win, TreeScope_Subtree, None, &auto).unwrap();
            }
            let u5: IUIAutomation5 = self.uia.cast().unwrap();
            let note: IUIAutomationNotificationEventHandler = Handler { t0, probe: self.uia.clone() }.into();
            u5.AddNotificationEventHandler(&win, TreeScope_Subtree, None, &note).unwrap();
            say(&format!("recording events of {proc} for {seconds} s"));
            std::thread::sleep(Duration::from_secs(seconds));
            self.uia.RemoveAllEventHandlers().unwrap();
            say("recording done");
        }
    }
}

#[implement(
    IUIAutomationFocusChangedEventHandler,
    IUIAutomationPropertyChangedEventHandler,
    IUIAutomationStructureChangedEventHandler,
    IUIAutomationEventHandler,
    IUIAutomationNotificationEventHandler
)]
struct Handler {
    t0: Instant,
    probe: IUIAutomation,
}

impl Handler {
    fn line(&self, kind: &str, sender: &Ref<'_, IUIAutomationElement>, rest: &str) {
        let p = Probe { uia: self.probe.clone() };
        let who = match sender.as_ref() {
            Some(el) => p.describe(el, None),
            None => "(no sender)".into(),
        };
        say(&format!("{:>8.3} {kind}{rest} | {who}", self.t0.elapsed().as_secs_f64()));
    }
}

impl IUIAutomationFocusChangedEventHandler_Impl for Handler_Impl {
    fn HandleFocusChangedEvent(&self, sender: Ref<'_, IUIAutomationElement>) -> Result<()> {
        self.line("focus", &sender, "");
        Ok(())
    }
}

impl IUIAutomationPropertyChangedEventHandler_Impl for Handler_Impl {
    fn HandlePropertyChangedEvent(
        &self,
        sender: Ref<'_, IUIAutomationElement>,
        propertyid: UIA_PROPERTY_ID,
        newvalue: &VARIANT,
    ) -> Result<()> {
        let name = unsafe { self.probe.GetPropertyProgrammaticName(propertyid) }.map(|b| b.to_string()).unwrap_or_default();
        self.line("property", &sender, &format!(" {name}={}", variant_text(newvalue)));
        Ok(())
    }
}

impl IUIAutomationStructureChangedEventHandler_Impl for Handler_Impl {
    fn HandleStructureChangedEvent(
        &self,
        sender: Ref<'_, IUIAutomationElement>,
        changetype: StructureChangeType,
        _runtimeid: *const SAFEARRAY,
    ) -> Result<()> {
        let t = match changetype {
            StructureChangeType_ChildAdded => "ChildAdded",
            StructureChangeType_ChildRemoved => "ChildRemoved",
            StructureChangeType_ChildrenInvalidated => "ChildrenInvalidated",
            StructureChangeType_ChildrenBulkAdded => "ChildrenBulkAdded",
            StructureChangeType_ChildrenBulkRemoved => "ChildrenBulkRemoved",
            StructureChangeType_ChildrenReordered => "ChildrenReordered",
            _ => "?",
        };
        self.line("structure", &sender, &format!(" {t}"));
        Ok(())
    }
}

impl IUIAutomationEventHandler_Impl for Handler_Impl {
    fn HandleAutomationEvent(&self, sender: Ref<'_, IUIAutomationElement>, eventid: UIA_EVENT_ID) -> Result<()> {
        let name = match eventid {
            UIA_Text_TextChangedEventId => "TextChanged",
            UIA_Text_TextSelectionChangedEventId => "TextSelectionChanged",
            UIA_LiveRegionChangedEventId => "LiveRegionChanged",
            UIA_Invoke_InvokedEventId => "Invoked",
            UIA_SelectionItem_ElementSelectedEventId => "ElementSelected",
            UIA_MenuOpenedEventId => "MenuOpened",
            UIA_MenuClosedEventId => "MenuClosed",
            UIA_Window_WindowOpenedEventId => "WindowOpened",
            UIA_Window_WindowClosedEventId => "WindowClosed",
            _ => "?",
        };
        self.line("event", &sender, &format!(" {name}"));
        Ok(())
    }
}

impl IUIAutomationNotificationEventHandler_Impl for Handler_Impl {
    fn HandleNotificationEvent(
        &self,
        sender: Ref<'_, IUIAutomationElement>,
        notificationkind: NotificationKind,
        notificationprocessing: NotificationProcessing,
        displaystring: &BSTR,
        activityid: &BSTR,
    ) -> Result<()> {
        self.line(
            "notification",
            &sender,
            &format!(
                " kind={} processing={} text=\"{}\" activity=\"{}\"",
                notificationkind.0,
                notificationprocessing.0,
                esc(&displaystring.to_string()),
                esc(&activityid.to_string())
            ),
        );
        Ok(())
    }
}

fn pids_of(proc: &str) -> Vec<u32> {
    // tasklist is the one dependency-free way to map an image name to pids.
    let out = std::process::Command::new("tasklist")
        .args(["/FO", "CSV", "/NH", "/FI", &format!("IMAGENAME eq {proc}.exe")])
        .output()
        .expect("tasklist");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.split("\",\"").nth(1).and_then(|p| p.parse().ok()))
        .collect()
}

fn vk(name: &str) -> (VIRTUAL_KEY, bool) {
    // (key, extended)
    let n = name.to_ascii_lowercase();
    let k = match n.as_str() {
        "tab" => VK_TAB,
        "enter" | "return" => VK_RETURN,
        "esc" | "escape" => VK_ESCAPE,
        "space" => VK_SPACE,
        "backspace" => VK_BACK,
        "delete" | "del" => return (VK_DELETE, true),
        "up" => return (VK_UP, true),
        "down" => return (VK_DOWN, true),
        "left" => return (VK_LEFT, true),
        "right" => return (VK_RIGHT, true),
        "home" => return (VK_HOME, true),
        "end" => return (VK_END, true),
        "pageup" => return (VK_PRIOR, true),
        "pagedown" => return (VK_NEXT, true),
        "insert" => return (VK_INSERT, true),
        "apps" | "menu" => return (VK_APPS, true),
        "ctrl" => VK_CONTROL,
        "shift" => VK_SHIFT,
        "alt" => VK_MENU,
        "win" => return (VK_LWIN, true),
        "capslock" => VK_CAPITAL,
        "numpadplus" => VK_ADD,
        "numpadminus" => VK_SUBTRACT,
        f if f.starts_with('f') && f.len() > 1 && f[1..].parse::<u16>().is_ok() => {
            VIRTUAL_KEY(VK_F1.0 + f[1..].parse::<u16>().unwrap() - 1)
        }
        c if c.len() == 1 && c.as_bytes()[0].is_ascii_alphanumeric() => VIRTUAL_KEY(c.to_ascii_uppercase().as_bytes()[0] as u16),
        "." => VK_OEM_PERIOD,
        "," => VK_OEM_COMMA,
        "/" => VK_OEM_2,
        "-" => VK_OEM_MINUS,
        "=" => VK_OEM_PLUS,
        _ => die(&format!("unknown key {name:?}")),
    };
    (k, false)
}

fn key_input(k: VIRTUAL_KEY, ext: bool, up: bool) -> INPUT {
    let mut flags = KEYBD_EVENT_FLAGS(0);
    if ext {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    let scan = unsafe { MapVirtualKeyW(k.0 as u32, MAPVK_VK_TO_VSC) } as u16;
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: k, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    }
}

/// `ctrl+shift+tab`-style combos; `text:hello` types text as Unicode characters.
fn send_combo(combo: &str) {
    let mut inputs = Vec::new();
    if let Some(text) = combo.strip_prefix("text:") {
        for u in text.encode_utf16() {
            for up in [false, true] {
                let mut flags = KEYEVENTF_UNICODE;
                if up {
                    flags |= KEYEVENTF_KEYUP;
                }
                inputs.push(INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT { wVk: VIRTUAL_KEY(0), wScan: u, dwFlags: flags, time: 0, dwExtraInfo: 0 },
                    },
                });
            }
        }
    } else {
        let parts: Vec<(VIRTUAL_KEY, bool)> = combo.split('+').map(vk).collect();
        for &(k, e) in &parts {
            inputs.push(key_input(k, e, false));
        }
        for &(k, e) in parts.iter().rev() {
            inputs.push(key_input(k, e, true));
        }
    }
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        die(&format!("SendInput sent {sent} of {} events for {combo:?}", inputs.len()));
    }
}

/// The virtual screen as a 32-bit top-down BMP.
fn shot(path: &str) {
    unsafe {
        let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp.into());
        BitBlt(mem, 0, 0, w, h, Some(screen), x, y, SRCCOPY).unwrap();
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut px = vec![0u8; (w * h * 4) as usize];
        let lines = GetDIBits(mem, bmp, 0, h as u32, Some(px.as_mut_ptr().cast()), &mut info, DIB_RGB_COLORS);
        if lines != h {
            die(&format!("GetDIBits copied {lines} of {h} lines"));
        }
        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        let mut f = Vec::with_capacity(54 + px.len());
        let size = 54 + px.len() as u32;
        f.extend_from_slice(b"BM");
        f.extend_from_slice(&size.to_le_bytes());
        f.extend_from_slice(&0u32.to_le_bytes());
        f.extend_from_slice(&54u32.to_le_bytes());
        f.extend_from_slice(&40u32.to_le_bytes());
        f.extend_from_slice(&w.to_le_bytes());
        f.extend_from_slice(&(-h).to_le_bytes());
        f.extend_from_slice(&1u16.to_le_bytes());
        f.extend_from_slice(&32u16.to_le_bytes());
        f.extend_from_slice(&[0u8; 24]);
        f.extend_from_slice(&px);
        std::fs::write(path, f).unwrap_or_else(|e| die(&format!("writing {path}: {e}")));
    }
}

/// The screen's pixels inside `r`, as 32-bit BGRA rows.
fn grab(r: windows::Win32::Foundation::RECT) -> Vec<u8> {
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp.into());
        BitBlt(mem, 0, 0, w, h, Some(screen), r.left, r.top, SRCCOPY).unwrap();
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut px = vec![0u8; (w * h * 4) as usize];
        GetDIBits(mem, bmp, 0, h as u32, Some(px.as_mut_ptr().cast()), &mut info, DIB_RGB_COLORS);
        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        px
    }
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

/// Starts EXE, then times the first visible top-level window of its process and the first change
/// of that window's client pixels from what the screen showed there when the window appeared.
fn launch(exe: &str, rest: &[String]) {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, GetWindowThreadProcessId, IsWindowVisible};
    let t0 = Instant::now();
    // The program outlives the probe: it is measured, then left running for the next check.
    #[allow(clippy::zombie_processes)]
    let child = std::process::Command::new(exe).args(rest).spawn().unwrap_or_else(|e| die(&format!("starting {exe}: {e}")));
    let pid = child.id();
    let find = || -> Option<HWND> {
        let mut hwnd = HWND::default();
        loop {
            hwnd = unsafe { windows::Win32::UI::WindowsAndMessaging::FindWindowExW(None, Some(hwnd), None, None) }.ok()?;
            let mut p = 0u32;
            unsafe { GetWindowThreadProcessId(hwnd, Some(&mut p)) };
            if p == pid && unsafe { IsWindowVisible(hwnd) }.as_bool() {
                return Some(hwnd);
            }
        }
    };
    let hwnd = loop {
        if let Some(h) = find() {
            break h;
        }
        if t0.elapsed() > Duration::from_secs(60) {
            die("no visible window within 60 s");
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    let t_window = t0.elapsed().as_secs_f64() * 1000.0;
    let mut r = Default::default();
    unsafe { GetWindowRect(hwnd, &mut r) }.unwrap();
    // The middle half of the window, away from the frame Windows draws itself.
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    let inner = windows::Win32::Foundation::RECT { left: r.left + w / 4, top: r.top + h / 4, right: r.right - w / 4, bottom: r.bottom - h / 4 };
    let first = grab(inner);
    loop {
        if grab(inner) != first {
            break;
        }
        if t0.elapsed() > Duration::from_secs(60) {
            die("the window's pixels did not change within 60 s");
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let t_pixels = t0.elapsed().as_secs_f64() * 1000.0;
    say(&format!("pid {pid}: visible window after {t_window:.1} ms, first drawn pixels after {t_pixels:.1} ms"));
}

/// N times: SendInput one letter to the focused composer and time until its pixels change, then
/// Backspace and wait for the pixels to settle back.
fn keylat(p: &Probe, proc: &str, n: usize) {
    let el = p.find(proc, "type=Edit");
    unsafe { el.SetFocus() }.unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let r = unsafe { el.CurrentBoundingRectangle() }.unwrap();
    let mut v = Vec::new();
    for _ in 0..n {
        let before = grab(r);
        let t0 = Instant::now();
        send_combo("x");
        loop {
            if grab(r) != before {
                break;
            }
            if t0.elapsed() > Duration::from_secs(5) {
                die("the composer did not change within 5 s of a key");
            }
        }
        v.push(t0.elapsed().as_secs_f64() * 1000.0);
        send_combo("backspace");
        let t1 = Instant::now();
        while grab(r) != before && t1.elapsed() < Duration::from_secs(5) {}
        std::thread::sleep(Duration::from_millis(150));
    }
    let list: Vec<String> = v.iter().map(|x| format!("{x:.1}")).collect();
    let (min, max) = (v.iter().cloned().fold(f64::MAX, f64::min), v.iter().cloned().fold(0.0, f64::max));
    say(&format!("key-to-screen ms over {n}: median {:.1}, min {min:.1}, max {max:.1}; all {}", median(&mut v), list.join(" ")));
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a = |i: usize| args.get(i).map(String::as_str).unwrap_or_else(|| die("missing argument; see the header of main.rs"));
    match a(0) {
        "dump" => {
            let p = Probe::new();
            let view = args.get(2).map(String::as_str).filter(|v| !v.starts_with("--")).unwrap_or("raw");
            p.dump(a(1), view, args.iter().any(|x| x == "--stable"));
        }
        "text" => Probe::new().text(a(1), a(2), args.get(3).map(String::as_str).unwrap_or("word")),
        "act" => {
            // act PROC SELECTOR ACTION [ARG] [--key COMBO] [--wait MS]: the key (SendInput to the
            // foreground window) and the wait come first, in this process, for actions on elements
            // that live only briefly (Stop while a turn streams).
            let p = Probe::new();
            let mut rest: Vec<String> = Vec::new();
            let mut i = 4;
            let mut wait = 0u64;
            while i < args.len() {
                match args[i].as_str() {
                    "--key" => {
                        send_combo(a(i + 1));
                        i += 2;
                    }
                    "--wait" => {
                        wait = a(i + 1).parse().unwrap_or_else(|_| die("--wait takes milliseconds"));
                        i += 2;
                    }
                    _ => {
                        rest.push(args[i].clone());
                        i += 1;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(wait));
            p.act(a(1), a(2), a(3), rest.first().map(String::as_str));
        }
        "focused" => say(&Probe::new().focused_line()),
        "events" => Probe::new().events(a(1), a(2).parse().unwrap_or_else(|_| die("SECONDS must be a number"))),
        "keys" => {
            let p = Probe::new();
            let delay = a(2).parse().unwrap_or_else(|_| die("DELAY_MS must be a number"));
            let mut keys = Vec::new();
            let mut shots = None;
            let mut i = 3;
            while i < args.len() {
                if args[i] == "--shots" {
                    shots = Some(a(i + 1).to_string());
                    i += 2;
                } else {
                    keys.push(args[i].clone());
                    i += 1;
                }
            }
            p.keys(a(1), delay, &keys, shots.as_deref());
        }
        "shot" => shot(a(1)),
        "table" => Probe::new().table(a(1), a(2)),
        "launch" => launch(a(1), &args[2..]),
        "keylat" => keylat(&Probe::new(), a(1), a(2).parse().unwrap_or_else(|_| die("N must be a number"))),
        c => die(&format!("unknown command {c:?}; see the header of main.rs")),
    }
}
