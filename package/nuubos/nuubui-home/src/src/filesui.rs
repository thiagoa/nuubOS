/*
 * Files application and storage pages of nuubUI.
 *
 *   58 Files (EPIC-031/036)        two-pane commander (components/
 *                                  files_app.slint, user request
 *                                  2026-10-12): places (storage, USB drives,
 *                                  network shares) or a folder per pane;
 *                                  copy/move go to the other pane (jobs);
 *                                  Add to Library (nuubos-library-import
 *                                  decides where games go, jobs)
 *   61 Removable Media (EPIC-032)  nuubos-mediactl (Settings > Storage)
 *   62 Backup (EPIC-035)           Settings > System > Troubleshooting:
 *                                  the console backup (storageui)
 *   28 Recovery (EPIC-035/049/050) next to it: RESTORE (storageui),
 *                                  REPAIR & RESET (main.rs,
 *                                  nuubos-recoveryctl)
 *
 * 61, 62 and 28 are Settings pages on the generic rows of home.slint. No
 * file policy lives here: paths are checked by nuubos-filesctl, shares by
 * nuubos-sharesctl.
 */

use crate::gameui::{
    apply_rows, apply_rows_sections, arm_or, armed, armed_detail, emulation, focused_key, move_selection, notice, row,
    run_tool, show, tool_then,
};
use crate::{handle_settings_action, navigate_settings_view, open_settings_choice, open_system_keyboard,
    play_ui_sound, tr, tr_arg, write_ui_context, AppHint, FilePane, FileRow, GenRow, HomeWindow};
use slint::{ComponentHandle, ModelRc, Timer, TimerMode, VecModel};
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub const KEYBOARD_FOLDER: i32 = 16;
pub const KEYBOARD_RENAME: i32 = 17;
/* Add Network Share, a guided flow (user report 2026-10-13: it was not
 * clear how to add one): the PCs and NAS found on the network (user request
 * 2026-10-14) or a typed server, its shared folders (listed, or typed),
 * user and password when the server asks for them. */
pub const KEYBOARD_SHARE_NAME: i32 = 18; /* the server */
pub const KEYBOARD_SHARE_URL: i32 = 19; /* the shared folder */
pub const KEYBOARD_SHARE_USER: i32 = 20;
pub const KEYBOARD_SHARE_PASSWORD: i32 = 21;

const FILESCTL: &str = "/usr/bin/nuubos-filesctl";
const MEDIACTL: &str = "/usr/sbin/nuubos-mediactl";
const SHARESCTL: &str = "/usr/sbin/nuubos-sharesctl";
const JOBCTL: &str = "/usr/bin/nuubos-jobctl";
const LIBRARY_IMPORT: &str = "/usr/bin/nuubos-library-import";
const SHARES_ROOT: &str = "/run/nuubos/shares";

pub const FILES_VIEW: i32 = 58;
pub const BACKUP_VIEW: i32 = 62;
pub const RECOVERY_VIEW: i32 = 28;

/* The Files app owns Home input. */
pub static ACTIVE: AtomicBool = AtomicBool::new(false);

/* FileRow icons (files_app.slint). */
const I_FOLDER: i32 = 0;
const I_FILE: i32 = 1;
const I_IMAGE: i32 = 2;
const I_AUDIO: i32 = 3;
const I_VIDEO: i32 = 4;
const I_ARCHIVE: i32 = 5;
const I_DRIVE: i32 = 6;
const I_SD: i32 = 7;
const I_USB: i32 = 8;
const I_SHARE: i32 = 9;
const I_ADD: i32 = 10;
const I_UP: i32 = 11;
const I_ROM: i32 = 12;

#[derive(Clone, Default)]
struct Entry {
    dir: bool,
    size: i64,
    mtime: i64,
    name: String,
}

#[derive(Clone, Default)]
struct Place {
    /* "root:<path>", "share:<name>" (not connected), "addshare". */
    key: String,
    title: String,
    detail: String,
    icon: i32,
    usage: f32,
    dim: bool,
}

#[derive(Default)]
struct Pane {
    /* "" = the places. */
    cwd: String,
    entries: Vec<Entry>,
    index: usize,
    marks: BTreeSet<String>,
    loading: bool,
}

#[derive(Clone, Copy, Default, PartialEq)]
enum Sort {
    #[default]
    Name,
    Size,
    Date,
}

/* Add to Library: one selected item, its plan (nuubos-library-import plan)
 * and the systems chosen for its questions. */
#[derive(Clone, Default)]
struct ImportItem {
    src: String,
    files: usize,
    /* (ask number, label, files, candidates (id, name)) */
    asks: Vec<(usize, String, usize, Vec<(String, String)>)>,
    answers: Vec<String>,
}

#[derive(Default)]
struct State {
    panes: [Pane; 2],
    active: usize,
    sort: Sort,
    places: Vec<Place>,
    /* (root path, label) of every location, for the pane titles. */
    roots: Vec<(String, String, i64)>,
    busy: bool,
    progress: f32,
    progress_label: String,
    details: String,
    utc_offset: i64,
    shares: Vec<(String, String, String, bool)>,
    /* Add Network Share in progress: (server, shared folder, user), the
     * password once given and the server's name for the titles. */
    share_draft: (String, String, String),
    share_secret: String,
    share_label: String,
    volumes: Vec<(String, String, i64, i64)>,
    focused: String,
    notice_timer: Option<Timer>,
    imports: Vec<ImportItem>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn human(bytes: i64) -> String {
    let b = bytes as f64;
    if b >= 1e9 {
        format!("{:.1} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.1} MB", b / 1e6)
    } else if b >= 1e3 {
        format!("{:.0} KB", b / 1e3)
    } else {
        format!("{} B", bytes)
    }
}

/* /media/<name>: "sd2" is the games card in the TF2 slot. */
pub fn volume_label(ui: &HomeWindow, name: &str) -> String {
    if name == "sd2" { tr(ui, 64, "SD2") } else { name.to_owned() }
}

fn parent(path: &str) -> String {
    match path.rfind('/') {
        Some(0) | None => String::new(),
        Some(i) => path[..i].to_owned(),
    }
}

fn base(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn parse_kv_lines<'a>(out: &'a str, key: &str) -> Vec<Vec<&'a str>> {
    out.lines()
        .filter_map(|l| l.strip_prefix(key).and_then(|r| r.strip_prefix('=')))
        .map(|v| v.split('\t').collect())
        .collect()
}

fn icon_for(name: &str) -> i32 {
    match name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default().as_str() {
        "png" | "jpg" | "jpeg" | "bmp" | "gif" | "webp" => I_IMAGE,
        "mp3" | "flac" | "ogg" | "opus" | "m4a" | "aac" | "wav" => I_AUDIO,
        "mkv" | "mp4" | "m4v" | "avi" | "webm" | "mov" | "ts" | "mpg" | "mpeg" => I_VIDEO,
        "zip" | "7z" | "rar" | "tar" | "gz" | "xz" | "bz2" => I_ARCHIVE,
        "sfc" | "smc" | "nes" | "gba" | "gbc" | "gb" | "md" | "gen" | "n64" | "z64" | "nds" | "pce" | "iso" | "chd"
        | "cue" | "pbp" | "sms" | "gg" | "a26" | "lnx" | "ngp" | "ws" | "wsc" | "32x" => I_ROM,
        _ => I_FILE,
    }
}

/* Civil date from days since 1970-01-01 (Howard Hinnant's algorithm). */
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }, m, d)
}

/* Local date and time of a file (the offset is read once when Files opens). */
fn date_time(ui: &HomeWindow, mtime: i64, offset: i64) -> String {
    let t = mtime + offset;
    let (y, m, d) = civil(t.div_euclid(86400));
    let secs = t.rem_euclid(86400);
    format!("{} {:02}:{:02}", crate::datetime::date_label(ui, &format!("{y:04}-{m:02}-{d:02}")), secs / 3600, (secs / 60) % 60)
}

fn utc_offset() -> i64 {
    let out = run_tool("/bin/date", &["+%z"], None);
    let v = out.trim();
    if v.len() != 5 {
        return 0;
    }
    let sign = if v.starts_with('-') { -1 } else { 1 };
    let h: i64 = v[1..3].parse().unwrap_or(0);
    let m: i64 = v[3..5].parse().unwrap_or(0);
    sign * (h * 3600 + m * 60)
}

/* ---------------------------------------------------------------- */
/* Rendering                                                         */
/* ---------------------------------------------------------------- */

fn root_of<'a>(st: &'a State, path: &str) -> Option<&'a (String, String, i64)> {
    st.roots.iter().filter(|(r, _, _)| path == r || path.starts_with(&format!("{r}/"))).max_by_key(|(r, _, _)| r.len())
}

/* The rows of a pane: ".." first in a folder. */
fn pane_rows(st: &State, p: usize) -> Vec<FileRow> {
    let pane = &st.panes[p];
    if pane.cwd.is_empty() {
        return st
            .places
            .iter()
            .map(|pl| FileRow { name: pl.title.clone().into(), detail: pl.detail.clone().into(), icon: pl.icon, marked: false,
                dim: pl.dim, usage: pl.usage })
            .collect();
    }
    let mut rows = vec![FileRow { name: "..".into(), detail: "".into(), icon: I_UP, marked: false, dim: false, usage: -1.0 }];
    rows.extend(pane.entries.iter().map(|e| FileRow {
        name: e.name.clone().into(),
        detail: if e.dir { "".into() } else { human(e.size).into() },
        icon: if e.dir { I_FOLDER } else { icon_for(&e.name) },
        marked: pane.marks.contains(&e.name),
        dim: false,
        usage: -1.0,
    }));
    rows
}

fn pane_model(ui: &HomeWindow, st: &State, p: usize) -> FilePane {
    let pane = &st.panes[p];
    let (title, path) = if pane.cwd.is_empty() {
        (tr(ui, 1138, "Places"), String::new())
    } else {
        match root_of(st, &pane.cwd) {
            Some((r, label, _)) => (label.clone(), pane.cwd[r.len()..].to_owned()),
            None => (base(&pane.cwd).to_owned(), pane.cwd.clone()),
        }
    };
    let path = if path.is_empty() && !pane.cwd.is_empty() { "/".to_owned() } else { path };
    FilePane {
        title: title.into(),
        path: path.into(),
        rows: ModelRc::from(Rc::new(VecModel::from(pane_rows(st, p)))),
        index: pane.index as i32,
        places: pane.cwd.is_empty(),
        loading: pane.loading,
    }
}

/* The focused entry of the active pane (None on "..", places). */
fn focused_entry(st: &State) -> Option<Entry> {
    let pane = &st.panes[st.active];
    if pane.cwd.is_empty() || pane.index == 0 {
        return None;
    }
    pane.entries.get(pane.index - 1).cloned()
}

fn status(ui: &HomeWindow, st: &State) -> (String, String) {
    let pane = &st.panes[st.active];
    if pane.cwd.is_empty() {
        let p = st.places.get(pane.index);
        return (p.map(|p| format!("{} • {}", p.title, p.detail)).unwrap_or_default(), String::new());
    }
    let left = if !st.details.is_empty() {
        st.details.clone()
    } else {
        match focused_entry(st) {
            Some(e) if e.dir => format!("{} • {}", e.name, tr_arg(ui, 1159, "Modified {0}", &date_time(ui, e.mtime, st.utc_offset))),
            Some(e) => format!("{} • {} • {}", e.name, human(e.size), date_time(ui, e.mtime, st.utc_offset)),
            None => pane.cwd.clone(),
        }
    };
    let free = root_of(st, &pane.cwd).map(|(_, _, f)| tr_arg(ui, 622, "{0} free", &human(*f))).unwrap_or_default();
    let right = if !pane.marks.is_empty() {
        format!("{} • {}", tr_arg(ui, 1141, "{0} selected", &pane.marks.len().to_string()), free)
    } else {
        format!("{} • {}", tr_arg(ui, 1142, "{0} items", &pane.entries.len().to_string()), free)
    };
    (left, right)
}

fn sort_label(ui: &HomeWindow, sort: Sort) -> String {
    let name = match sort {
        Sort::Name => tr(ui, 1146, "Name"),
        Sort::Size => tr(ui, 1147, "Size"),
        Sort::Date => tr(ui, 1148, "Date"),
    };
    tr_arg(ui, 1160, "Sort: {0}", &name)
}

fn hints(ui: &HomeWindow, st: &State) {
    let hint = |kind: &str, role: i32, label: String| AppHint { kind: kind.into(), role, label: label.into() };
    let pane = &st.panes[st.active];
    let mut h = Vec::new();
    /* On a file confirm opens the actions too (West's hint says so). */
    let on_file = focused_entry(st).map(|e| !e.dir).unwrap_or(false);
    if !on_file {
        h.push(hint("face", 0, tr(ui, 212, "Open")));
    }
    h.push(hint("face", 1, tr(ui, 186, "Back")));
    if !pane.cwd.is_empty() {
        h.push(hint("face", 2, tr(ui, 1153, "Mark")));
    }
    h.push(hint("face", 3, tr(ui, 1152, "Actions")));
    if !pane.cwd.is_empty() {
        h.push(hint("select", 0, tr(ui, 1145, "Sort")));
    }
    ui.set_app_hints(ModelRc::from(Rc::new(VecModel::from(h))));
}

fn render_files(ui: &HomeWindow) {
    STATE.with(|st| {
        let st = st.borrow();
        ui.set_files_left(pane_model(ui, &st, 0));
        ui.set_files_right(pane_model(ui, &st, 1));
        ui.set_files_active(st.active as i32);
        let (l, r) = status(ui, &st);
        ui.set_files_status(l.into());
        ui.set_files_status_right(r.into());
        ui.set_files_progress(if st.busy { st.progress } else { -1.0 });
        ui.set_files_progress_label(st.progress_label.clone().into());
        ui.set_files_sort_label(sort_label(ui, st.sort).into());
        hints(ui, &st);
    });
}

/* Cursor moves touch only the index of the active pane (the rows model of
 * a big folder is not rebuilt). */
fn render_cursor(ui: &HomeWindow) {
    STATE.with(|st| {
        let st = st.borrow();
        let p = st.active;
        let mut pane = if p == 0 { ui.get_files_left() } else { ui.get_files_right() };
        pane.index = st.panes[p].index as i32;
        if p == 0 { ui.set_files_left(pane) } else { ui.set_files_right(pane) }
        let (l, r) = status(ui, &st);
        ui.set_files_status(l.into());
        ui.set_files_status_right(r.into());
        hints(ui, &st);
    });
}

fn files_notice(ui: &HomeWindow, text: String) {
    ui.set_files_notice(text.into());
    let weak = ui.as_weak();
    let timer = Timer::default();
    timer.start(TimerMode::SingleShot, Duration::from_millis(3500), move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_files_notice("".into());
        }
    });
    STATE.with(|st| st.borrow_mut().notice_timer = Some(timer));
}

/* ---------------------------------------------------------------- */
/* Loading                                                           */
/* ---------------------------------------------------------------- */

fn sort_entries(entries: &mut [Entry], sort: Sort) {
    entries.sort_by(|a, b| {
        b.dir.cmp(&a.dir).then(match sort {
            Sort::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            Sort::Size => b.size.cmp(&a.size).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())),
            Sort::Date => b.mtime.cmp(&a.mtime),
        })
    });
}

/* Places and both folders, off the UI thread. `focus`: name to keep the
 * cursor on in the active pane. */
fn refresh(ui: &HomeWindow) {
    let (cwds, sort) = STATE.with(|st| {
        let mut st = st.borrow_mut();
        for p in st.panes.iter_mut() {
            p.loading = true;
        }
        ([st.panes[0].cwd.clone(), st.panes[1].cwd.clone()], st.sort)
    });
    let labels = (tr(ui, 610, "Internal Storage"), tr(ui, 64, "SD2"), tr(ui, 622, "{0} free"), tr(ui, 1125, "Connect to open it"),
        tr(ui, 1172, "Add Network Location"), tr(ui, 1128, "Network Location"), tr(ui, 1173, "A folder on a PC or NAS"));
    let weak = ui.as_weak();
    thread::spawn(move || {
        let mut places = Vec::new();
        let mut roots = Vec::new();
        for f in parse_kv_lines(&run_tool(FILESCTL, &["roots"], None), "root") {
            if f.len() < 4 {
                continue;
            }
            let total: i64 = f[2].parse().unwrap_or(0);
            let free: i64 = f[3].parse().unwrap_or(0);
            let (title, icon) = match f[0] {
                "userdata" => (labels.0.clone(), I_DRIVE),
                "sd2" => (labels.1.clone(), I_SD),
                "share" => (base(f[1]).to_owned(), I_SHARE),
                _ => (base(f[1]).to_owned(), I_USB),
            };
            roots.push((f[1].to_owned(), title.clone(), free));
            places.push(Place {
                key: format!("root:{}", f[1]),
                title,
                detail: if f[0] == "share" { format!("{} • {}", labels.5, labels.2.replace("{0}", &human(free))) }
                    else { format!("{} / {}", labels.2.replace("{0}", &human(free)), human(total)) },
                icon,
                usage: if total > 0 { 1.0 - free as f32 / total as f32 } else { -1.0 },
                dim: false,
            });
        }
        let shares: Vec<(String, String, String, bool)> = parse_kv_lines(&run_tool(SHARESCTL, &["list"], None), "share")
            .into_iter()
            .filter(|f| f.len() >= 4)
            .map(|f| (f[0].into(), f[1].into(), f[2].into(), f[3] == "1"))
            .collect();
        for (name, url, _, mounted) in &shares {
            if !mounted {
                places.push(Place { key: format!("share:{name}"), title: name.clone(), detail: format!("{url} • {}", labels.3),
                    icon: I_SHARE, usage: -1.0, dim: true });
            }
        }
        places.push(Place { key: "addshare".into(), title: labels.4.clone(), detail: labels.6.clone(), icon: I_ADD, usage: -1.0,
            dim: false });
        let lists: Vec<Option<Vec<Entry>>> = cwds
            .iter()
            .map(|cwd| {
                if cwd.is_empty() {
                    return Some(Vec::new());
                }
                let out = run_tool(FILESCTL, &["list", cwd], None);
                if out.starts_with("ERR") {
                    return None;
                }
                let mut e: Vec<Entry> = parse_kv_lines(&out, "entry")
                    .into_iter()
                    .filter(|f| f.len() >= 4)
                    .map(|f| Entry { dir: f[0] == "d", size: f[1].parse().unwrap_or(0), mtime: f[2].parse().unwrap_or(0), name: f[3].into() })
                    .collect();
                sort_entries(&mut e, sort);
                Some(e)
            })
            .collect();
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else { return };
            STATE.with(|st| {
                let mut st = st.borrow_mut();
                st.places = places;
                st.roots = roots;
                st.shares = shares;
                let focus = std::mem::take(&mut st.focused);
                let active = st.active;
                let places_len = st.places.len();
                for (p, list) in lists.into_iter().enumerate() {
                    let pane = &mut st.panes[p];
                    if pane.cwd != cwds[p] {
                        continue;
                    }
                    pane.loading = false;
                    match list {
                        /* A removed volume, a dropped share: back to the places. */
                        None => {
                            pane.cwd.clear();
                            pane.entries.clear();
                            pane.index = 0;
                        }
                        Some(entries) => {
                            pane.marks.retain(|m| entries.iter().any(|e| &e.name == m));
                            pane.entries = entries;
                        }
                    }
                    if p == active && !focus.is_empty() {
                        if let Some(i) = pane.entries.iter().position(|e| e.name == focus) {
                            pane.index = i + 1;
                        } else if let Some(i) = st.places.iter().position(|pl| pl.key == focus) {
                            st.panes[p].index = i;
                            continue;
                        }
                    }
                    let pane = &mut st.panes[p];
                    let n = if pane.cwd.is_empty() { places_len } else { pane.entries.len() + 1 };
                    pane.index = pane.index.min(n.saturating_sub(1));
                }
            });
            render_files(&ui);
        });
    });
}

/* ---------------------------------------------------------------- */
/* Open / leave / navigation                                         */
/* ---------------------------------------------------------------- */

/* Home → Applications → Files. */
pub fn open(ui: &HomeWindow) {
    ACTIVE.store(true, Ordering::SeqCst);
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        st.busy = false;
        st.details.clear();
        st.utc_offset = utc_offset();
        if st.panes[0].cwd.is_empty() && st.panes[1].cwd.is_empty() {
            st.active = 0;
        }
    });
    ui.set_files_open(true);
    ui.set_app_kicker(tr(ui, 609, "Files").to_uppercase().into());
    ui.set_details_active(true);
    ui.set_details_shell_title(tr(ui, 609, "Files").into());
    ui.set_details_has_cover(false);
    navigate_settings_view(ui, FILES_VIEW);
    ui.set_settings_open(true);
    write_ui_context("settings");
    render_files(ui);
    refresh(ui);
}

fn leave(ui: &HomeWindow) {
    ACTIVE.store(false, Ordering::SeqCst);
    navigate_settings_view(ui, 0);
    ui.set_settings_selected_index(0);
    ui.set_settings_open(false);
    ui.set_details_active(false);
    ui.set_details_shell_title("".into());
    ui.set_files_open(false);
    ui.set_app_hints(ModelRc::default());
    write_ui_context("home");
}

fn enter(ui: &HomeWindow, path: String) {
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        let a = st.active;
        let pane = &mut st.panes[a];
        pane.cwd = path;
        pane.entries.clear();
        pane.marks.clear();
        pane.index = 0;
        st.details.clear();
    });
    render_files(ui);
    refresh(ui);
}

fn go_up(ui: &HomeWindow) -> bool {
    let target = STATE.with(|st| {
        let mut st = st.borrow_mut();
        let a = st.active;
        let cwd = st.panes[a].cwd.clone();
        if cwd.is_empty() {
            return None;
        }
        let at_root = st.roots.iter().any(|(r, _, _)| *r == cwd);
        st.focused = if at_root { format!("root:{cwd}") } else { base(&cwd).to_owned() };
        Some(if at_root { String::new() } else { parent(&cwd) })
    });
    match target {
        Some(t) => {
            STATE.with(|st| {
                let mut st = st.borrow_mut();
                let a = st.active;
                st.panes[a].cwd = t;
                st.panes[a].marks.clear();
                st.details.clear();
            });
            refresh(ui);
            true
        }
        None => false,
    }
}

fn move_cursor(ui: &HomeWindow, delta: i32) {
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        let a = st.active;
        let n = if st.panes[a].cwd.is_empty() { st.places.len() } else { st.panes[a].entries.len() + 1 } as i32;
        if n == 0 {
            return;
        }
        let i = st.panes[a].index as i32;
        st.panes[a].index = (i + delta).rem_euclid(n) as usize;
        st.details.clear();
    });
    render_cursor(ui);
}

fn switch_pane(ui: &HomeWindow, to: usize) {
    let changed = STATE.with(|st| {
        let mut st = st.borrow_mut();
        let changed = st.active != to;
        st.active = to;
        st.details.clear();
        changed
    });
    if changed {
        render_files(ui);
    }
}

fn confirm(ui: &HomeWindow) {
    enum Act {
        Up,
        Enter(String),
        Mount(String),
        AddShare,
        File,
        None,
    }
    let act = STATE.with(|st| {
        let st = st.borrow();
        let pane = &st.panes[st.active];
        if pane.cwd.is_empty() {
            return match st.places.get(pane.index) {
                Some(p) if p.key == "addshare" => Act::AddShare,
                Some(p) => match p.key.split_once(':') {
                    Some(("root", path)) => Act::Enter(path.to_owned()),
                    Some(("share", name)) => Act::Mount(name.to_owned()),
                    _ => Act::None,
                },
                None => Act::None,
            };
        }
        if pane.index == 0 {
            return Act::Up;
        }
        match focused_entry(&st) {
            Some(e) if e.dir => Act::Enter(format!("{}/{}", pane.cwd, e.name)),
            Some(_) => Act::File,
            None => Act::None,
        }
    });
    match act {
        Act::Up => {
            go_up(ui);
        }
        Act::Enter(p) => enter(ui, p),
        Act::Mount(name) => {
            STATE.with(|st| {
                let mut st = st.borrow_mut();
                st.busy = true;
                st.progress = 0.0;
                st.progress_label = tr(ui, 158, "Connect");
            });
            render_files(ui);
            let weak = ui.as_weak();
            thread::spawn(move || {
                let out = run_tool(SHARESCTL, &["mount", &name], None);
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = weak.upgrade() else { return };
                    STATE.with(|st| st.borrow_mut().busy = false);
                    if out.starts_with("OK") {
                        enter(&ui, format!("{SHARES_ROOT}/{name}"));
                    } else {
                        files_notice(&ui, share_error(&ui, &out, &name));
                        refresh(&ui);
                    }
                });
            });
        }
        Act::AddShare => start_add_share(ui),
        Act::File => actions(ui),
        Act::None => {}
    }
}

fn toggle_mark(ui: &HomeWindow) {
    let ok = STATE.with(|st| {
        let mut st = st.borrow_mut();
        let Some(e) = focused_entry(&st) else { return false };
        let a = st.active;
        let pane = &mut st.panes[a];
        if !pane.marks.remove(&e.name) {
            pane.marks.insert(e.name.clone());
        }
        if pane.index < pane.entries.len() {
            pane.index += 1;
        }
        true
    });
    if ok {
        /* The mark changes a row: rebuild the active pane. */
        render_files(ui);
    }
}

fn cycle_sort(ui: &HomeWindow) {
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        st.sort = match st.sort {
            Sort::Name => Sort::Size,
            Sort::Size => Sort::Date,
            Sort::Date => Sort::Name,
        };
        let sort = st.sort;
        for p in st.panes.iter_mut() {
            let focus = if p.index > 0 { p.entries.get(p.index - 1).map(|e| e.name.clone()) } else { None };
            sort_entries(&mut p.entries, sort);
            if let Some(f) = focus {
                p.index = p.entries.iter().position(|e| e.name == f).map(|i| i + 1).unwrap_or(p.index);
            }
        }
    });
    render_files(ui);
}

/* Input while the Files app is open (and its keyboard and dropdowns). */
pub fn handle_action(ui: &HomeWindow, action: &str, settings_active: &Arc<AtomicBool>) {
    if ui.get_settings_choice_open() || ui.get_settings_view() == 6 {
        handle_settings_action(ui, action, settings_active);
        return;
    }
    if ui.get_settings_view() != FILES_VIEW {
        return;
    }
    play_ui_sound(action);
    match action {
        "menu_up" => move_cursor(ui, -1),
        "menu_down" => move_cursor(ui, 1),
        "menu_left" => switch_pane(ui, 0),
        "menu_right" => switch_pane(ui, 1),
        "menu_confirm" => confirm(ui),
        "menu_back" => {
            if !go_up(ui) {
                leave(ui);
            }
        }
        "face_north" => toggle_mark(ui),
        "face_west" => actions(ui),
        "select" => cycle_sort(ui),
        _ => {}
    }
}

/* ---------------------------------------------------------------- */
/* Actions                                                           */
/* ---------------------------------------------------------------- */

/* The items an action applies to: the marks, else the focused entry. */
fn targets(st: &State) -> Vec<String> {
    let pane = &st.panes[st.active];
    if pane.cwd.is_empty() {
        return Vec::new();
    }
    if !pane.marks.is_empty() {
        return pane.marks.iter().map(|m| format!("{}/{}", pane.cwd, m)).collect();
    }
    focused_entry(st).map(|e| vec![format!("{}/{}", pane.cwd, e.name)]).unwrap_or_default()
}

fn actions(ui: &HomeWindow) {
    let mut opts: Vec<(String, String)> = Vec::new();
    let title = STATE.with(|st| {
        let st = st.borrow();
        let pane = &st.panes[st.active];
        if pane.cwd.is_empty() {
            let Some(p) = st.places.get(pane.index) else { return String::new() };
            if let Some(path) = p.key.strip_prefix("root:") {
                if let Some(name) = path.strip_prefix(&format!("{SHARES_ROOT}/")) {
                    opts.push((format!("share-umount:{name}"), tr(ui, 169, "Disconnect")));
                    opts.push((format!("share-remove:{name}"), tr(ui, 1162, "Remove Location")));
                } else if path.starts_with("/media/") {
                    opts.push((format!("eject:{}", base(path)), tr(ui, 626, "Eject")));
                }
            } else if let Some(name) = p.key.strip_prefix("share:") {
                opts.push((format!("share-mount:{name}"), tr(ui, 158, "Connect")));
                opts.push((format!("share-remove:{name}"), tr(ui, 1162, "Remove Location")));
            }
            opts.push(("add-share".into(), tr(ui, 1172, "Add Network Location")));
            return p.title.clone();
        }
        let n = targets(&st).len();
        let other = &st.panes[1 - st.active];
        if n > 0 && !other.cwd.is_empty() {
            opts.push(("copy".into(), tr(ui, 1139, "Copy to Other Panel")));
            opts.push(("move".into(), tr(ui, 1140, "Move to Other Panel")));
        }
        let focused = focused_entry(&st);
        if pane.marks.is_empty() && focused.is_some() {
            opts.push(("rename".into(), tr(ui, 617, "Rename")));
        }
        /* Play (user request 2026-10-28, personal fork feature): a raw
         * launch, no library entry, no history, no per-game overrides -
         * try a ROM before deciding whether to add it to the library.
         * Shown for any file; nuubos-emud itself rejects an extension no
         * system claims. */
        if pane.marks.is_empty() && focused.as_ref().is_some_and(|e| !e.dir) {
            opts.push(("play".into(), tr(ui, 1419, "Play")));
        }
        if n > 0 {
            opts.push(("add-library".into(), tr(ui, 1275, "Add to Library")));
            opts.push(("delete".into(), tr(ui, 618, "Delete")));
        }
        if focused.is_some() {
            opts.push(("details".into(), tr(ui, 523, "Details")));
        }
        opts.push(("mkdir".into(), tr(ui, 612, "New Folder")));
        if pane.marks.len() < pane.entries.len() {
            opts.push(("select-all".into(), tr(ui, 1143, "Select All")));
        }
        if !pane.marks.is_empty() {
            opts.push(("select-none".into(), tr(ui, 1144, "Clear Selection")));
        }
        opts.push(("same-folder".into(), tr(ui, 1149, "Same Folder in Other Panel")));
        opts.push(("add-share".into(), tr(ui, 1172, "Add Network Location")));
        opts.push(("sort".into(), sort_label(ui, st.sort)));
        if n > 1 {
            tr_arg(ui, 1141, "{0} selected", &n.to_string())
        } else {
            focused.map(|e| e.name).unwrap_or_else(|| base(&pane.cwd).to_owned())
        }
    });
    if !opts.is_empty() {
        open_settings_choice(ui, "files-action", &title, opts, "");
    }
}

/* Run jobs one after the other (jobctl run --progress TYPE ARGS...), with
 * the progress in the status bar: "{0}" of `label` is the job's name.
 * `done` gets every job's @result, or the @error reason of the first one
 * that failed ("failed" without one); later jobs are not started. */
fn run_jobs<F>(ui: &HomeWindow, jobs: Vec<(String, Vec<String>)>, label: String, done: F)
where
    F: FnOnce(&HomeWindow, Result<Vec<String>, String>) + Send + 'static,
{
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        st.busy = true;
        st.progress = 0.0;
        st.progress_label = label.replace("{0}", jobs.first().map(|j| j.0.as_str()).unwrap_or(""));
    });
    render_files(ui);
    let weak = ui.as_weak();
    thread::spawn(move || {
        let total = jobs.len();
        let mut results = Vec::new();
        let mut failure: Option<String> = None;
        for (k, (name, args)) in jobs.iter().enumerate() {
            let child = Command::new(JOBCTL)
                .args(["run", "--progress"])
                .args(args)
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn();
            let Ok(mut child) = child else {
                failure = Some("failed".into());
                break;
            };
            let mut last = String::new();
            let mut result = String::new();
            if let Some(out) = child.stdout.take() {
                for line in BufReader::new(out).lines() {
                    let Ok(line) = line else { break };
                    if let Some(p) = line.strip_prefix("@progress ") {
                        let pct: f32 = p.split_whitespace().next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
                        let overall = (k as f32 + pct.max(0.0) / 100.0) / total as f32;
                        let text = if total > 1 { format!("{} ({}/{})", label.replace("{0}", name), k + 1, total) }
                            else { label.replace("{0}", name) };
                        let weak = weak.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            STATE.with(|st| {
                                let mut st = st.borrow_mut();
                                st.progress = overall;
                                st.progress_label = text;
                            });
                            if let Some(ui) = weak.upgrade() {
                                let st = STATE.with(|st| (st.borrow().progress, st.borrow().progress_label.clone()));
                                ui.set_files_progress(st.0);
                                ui.set_files_progress_label(st.1.into());
                            }
                        });
                    } else if let Some(r) = line.strip_prefix("@result ") {
                        result = r.trim().to_owned();
                    } else {
                        last = line;
                    }
                }
            }
            let ok = child.wait().map(|s| s.success()).unwrap_or(false);
            if !ok || last.starts_with("ERR") || last.starts_with("@error") {
                let reason = last.strip_prefix("@error ").or_else(|| last.strip_prefix("ERR "))
                    .map(|r| r.trim().to_owned()).unwrap_or_default();
                failure = Some(if reason.is_empty() { "failed".into() } else { reason });
                break;
            }
            results.push(result);
        }
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else { return };
            STATE.with(|st| {
                let mut st = st.borrow_mut();
                st.busy = false;
                let a = st.active;
                st.panes[a].marks.clear();
            });
            done(&ui, match failure { Some(f) => Err(f), None => Ok(results) });
            refresh(&ui);
        });
    });
}

/* Copy/move every target into the other pane's folder, one job after the
 * other, with the progress in the status bar. */
fn transfer(ui: &HomeWindow, moving: bool) {
    let (items, dest) = STATE.with(|st| {
        let st = st.borrow();
        (targets(&st), st.panes[1 - st.active].cwd.clone())
    });
    if dest.is_empty() {
        files_notice(ui, tr(ui, 1157, "Open a folder in the other panel first"));
        return;
    }
    if items.iter().any(|src| dest == *src || dest.starts_with(&format!("{src}/"))) {
        files_notice(ui, tr(ui, 1158, "A folder cannot go inside itself"));
        return;
    }
    let label = if moving { tr(ui, 1155, "Moving {0}") } else { tr(ui, 1154, "Copying {0}") };
    let summary = tr(ui, 1161, "Done: {0}").replace("{0}", &items.len().to_string());
    let failed = (tr(ui, 493, "Not enough free space"), tr(ui, 93, "Failed"));
    let kind = if moving { "file-move" } else { "file-copy" };
    let jobs = items.iter().map(|src| (base(src).to_owned(), vec![kind.to_owned(), src.clone(), dest.clone()])).collect();
    run_jobs(ui, jobs, label, move |ui, res| {
        files_notice(ui, match res {
            Ok(_) => summary,
            Err(r) => if r.contains("space") { failed.0 } else { failed.1 },
        });
    });
}

/* ---------------------------------------------------------------- */
/* Add to Library                                                    */
/* ---------------------------------------------------------------- */

/* nuubos-library-import plan: files to import and the questions. */
fn parse_plan(src: &str, out: &str) -> ImportItem {
    let mut item = ImportItem { src: src.to_owned(), ..Default::default() };
    for line in out.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if let Some(head) = f.first().filter(|h| h.starts_with("files=")) {
            item.files = head.split_whitespace().next().and_then(|kv| kv.strip_prefix("files="))
                .and_then(|v| v.parse().ok()).unwrap_or(0);
        } else if f[0] == "ask" && f.len() >= 4 {
            let n = f[1].parse().unwrap_or(0);
            item.asks.push((n, f[2].to_owned(), f[3].parse().unwrap_or(1), Vec::new()));
        } else if f[0] == "cand" && f.len() >= 4 {
            let n: usize = f[1].parse().unwrap_or(usize::MAX);
            if let Some(a) = item.asks.iter_mut().find(|a| a.0 == n) {
                a.3.push((f[2].to_owned(), f[3].to_owned()));
            }
        }
    }
    item
}

/* Plan every target (a worker thread), then ask what the plans could not
 * decide, then import. */
fn start_import(ui: &HomeWindow) {
    let items = STATE.with(|st| {
        let st = st.borrow();
        if st.busy { Vec::new() } else { targets(&st) }
    });
    if items.is_empty() {
        return;
    }
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        st.busy = true;
        st.progress = 0.0;
        st.progress_label = tr(ui, 1277, "Adding {0} to the library").replace("{0}", base(&items[0]));
        st.imports.clear();
    });
    render_files(ui);
    let weak = ui.as_weak();
    thread::spawn(move || {
        let plans: Vec<ImportItem> = items.iter().map(|src| parse_plan(src, &run_tool(LIBRARY_IMPORT, &["plan", src], None))).collect();
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else { return };
            let any = plans.iter().any(|p| p.files > 0);
            STATE.with(|st| {
                let mut st = st.borrow_mut();
                st.busy = false;
                st.imports = plans.into_iter().filter(|p| p.files > 0).collect();
            });
            render_files(&ui);
            if any {
                ask_import(&ui);
            } else {
                files_notice(&ui, tr(&ui, 1280, "No games found here"));
            }
        });
    });
}

/* The next unanswered question, or the import once all are answered. Back
 * on a question cancels the import (nothing has been copied yet). */
fn ask_import(ui: &HomeWindow) {
    let next = STATE.with(|st| {
        let st = st.borrow();
        st.imports.iter().find_map(|it| it.asks.get(it.answers.len()).cloned())
    });
    let Some((_, label, files, candidates)) = next else {
        run_import(ui);
        return;
    };
    let shown = if files > 1 { format!("{label} (+{})", files - 1) } else { label };
    let mut options = candidates;
    options.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
    open_settings_choice(ui, "files-import", &tr_arg(ui, 1276, "Which system is {0}?", &shown), options, "");
}

fn answer_import(ui: &HomeWindow, system: String) {
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        if let Some(it) = st.imports.iter_mut().find(|it| it.answers.len() < it.asks.len()) {
            let n = it.asks[it.answers.len()].0;
            it.answers.push(format!("{n}={system}"));
        }
    });
    ask_import(ui);
}

fn run_import(ui: &HomeWindow) {
    let imports = STATE.with(|st| std::mem::take(&mut st.borrow_mut().imports));
    if imports.is_empty() {
        return;
    }
    let jobs = imports.iter()
        .map(|it| (base(&it.src).to_owned(), vec!["library-import".to_owned(), it.src.clone(), it.answers.join(",")]))
        .collect();
    let added = tr(ui, 1278, "{0} files added to the library");
    let present = tr(ui, 1279, "Already in the library");
    let none = tr(ui, 1280, "No games found here");
    let failed = (tr(ui, 493, "Not enough free space"), tr(ui, 93, "Failed"));
    run_jobs(ui, jobs, tr(ui, 1277, "Adding {0} to the library"), move |ui, res| {
        let text = match res {
            /* @result <copied> <already there> <skipped> */
            Ok(results) => {
                let sum = |i: usize| results.iter().map(|r| r.split_whitespace().nth(i).and_then(|v| v.parse::<usize>().ok()).unwrap_or(0)).sum::<usize>();
                let (copied, there) = (sum(0), sum(1));
                if copied > 0 { added.replace("{0}", &copied.to_string()) } else if there > 0 { present } else { none }
            }
            Err(r) => if r.contains("space") { failed.0 } else { failed.1 },
        };
        files_notice(ui, text);
    });
}

fn delete_targets(ui: &HomeWindow) {
    let items = STATE.with(|st| targets(&st.borrow()));
    let weak = ui.as_weak();
    thread::spawn(move || {
        let mut failed = false;
        for p in &items {
            failed |= run_tool(FILESCTL, &["delete", p], None).starts_with("ERR");
        }
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else { return };
            STATE.with(|st| {
                let mut st = st.borrow_mut();
                let a = st.active;
                st.panes[a].marks.clear();
            });
            if failed {
                files_notice(&ui, tr(&ui, 93, "Failed"));
            }
            refresh(&ui);
        });
    });
}

fn details(ui: &HomeWindow) {
    let Some(path) = STATE.with(|st| {
        let st = st.borrow();
        focused_entry(&st).map(|e| format!("{}/{}", st.panes[st.active].cwd, e.name))
    }) else { return };
    STATE.with(|st| st.borrow_mut().details = tr(ui, 1150, "Calculating…"));
    render_cursor(ui);
    let weak = ui.as_weak();
    thread::spawn(move || {
        let out = run_tool(FILESCTL, &["info", &path], None);
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else { return };
            let get = |k: &str| out.lines().find_map(|l| l.strip_prefix(k)).unwrap_or("").to_owned();
            let size: i64 = get("size=").parse().unwrap_or(0);
            let mtime: i64 = get("mtime=").parse().unwrap_or(0);
            let offset = STATE.with(|st| st.borrow().utc_offset);
            let text = format!("{} • {} • {}", base(&path), human(size), tr_arg(&ui, 1159, "Modified {0}", &date_time(&ui, mtime, offset)));
            STATE.with(|st| st.borrow_mut().details = text);
            render_cursor(&ui);
        });
    });
}

/* What went wrong connecting to a share (nuubos-sharesctl mount). */
fn share_error(ui: &HomeWindow, out: &str, what: &str) -> String {
    if out.contains("auth") {
        tr(ui, 1179, "Wrong user name or password")
    } else if out.contains("share") {
        tr_arg(ui, 1180, "The server has no folder named {0}", what)
    } else {
        tr_arg(ui, 1181, "Cannot reach {0}: check the address and that the computer is on", what)
    }
}

/* Look for PCs and NAS (nuubos-sharesctl discover, ~3 s), then offer
 * them with "Type an Address"; none found = the address keyboard. */
fn start_add_share(ui: &HomeWindow) {
    if STATE.with(|st| st.borrow().busy) {
        return;
    }
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        st.share_draft = Default::default();
        st.share_secret.clear();
        st.share_label.clear();
        st.busy = true;
        st.progress = 0.0;
        st.progress_label = tr(ui, 1281, "Looking for PCs and NAS on the network…");
    });
    render_files(ui);
    let weak = ui.as_weak();
    thread::spawn(move || {
        let out = run_tool(SHARESCTL, &["discover"], None);
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else { return };
            STATE.with(|st| st.borrow_mut().busy = false);
            render_files(&ui);
            let mut options: Vec<(String, String)> = out.lines()
                .filter_map(|l| l.strip_prefix("server="))
                .filter_map(|l| l.split_once('\t'))
                .map(|(name, ip)| (format!("{ip}\t{name}"), if name == ip { ip.to_owned() } else { format!("{name} · {ip}") }))
                .collect();
            if options.is_empty() {
                type_share_server(&ui);
                return;
            }
            options.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
            options.push(("manual".into(), tr(&ui, 1283, "Type an Address")));
            open_settings_choice(&ui, "files-server", &tr(&ui, 1282, "Choose a PC or NAS"), options, "");
        });
    });
}

fn type_share_server(ui: &HomeWindow) {
    open_system_keyboard(ui, &tr(ui, 1174, "Server address, for example 192.168.1.10 or nas.local"),
        KEYBOARD_SHARE_NAME, FILES_VIEW, "text", "");
}

fn type_share_folder(ui: &HomeWindow) {
    let label = STATE.with(|st| st.borrow().share_label.clone());
    open_system_keyboard(ui, &tr_arg(ui, 1175, "Folder on {0}, for example Music", &label),
        KEYBOARD_SHARE_URL, FILES_VIEW, "text", "");
}

/* The shared folders of the chosen server (nuubos-sharesctl shares, with
 * the user and password once given). A server that wants a login asks
 * for it; one that cannot list them (or lists none) gets a typed folder. */
fn browse_shares(ui: &HomeWindow) {
    let (server, user, secret, label) = STATE.with(|st| {
        let mut st = st.borrow_mut();
        st.busy = true;
        st.progress = 0.0;
        let label = st.share_label.clone();
        st.progress_label = tr_arg(ui, 1182, "Connecting to {0}…", &label);
        (st.share_draft.0.clone(), st.share_draft.2.clone(), st.share_secret.clone(), label)
    });
    render_files(ui);
    let weak = ui.as_weak();
    thread::spawn(move || {
        let mut args = vec!["shares", server.as_str()];
        if !user.is_empty() {
            args.push(user.as_str());
        }
        /* The password goes on stdin, never on the command line. */
        let out = run_tool(SHARESCTL, &args, if user.is_empty() { None } else { Some(secret) });
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else { return };
            STATE.with(|st| st.borrow_mut().busy = false);
            render_files(&ui);
            if out.starts_with("ERR auth") {
                if !user.is_empty() {
                    files_notice(&ui, tr(&ui, 1179, "Wrong user name or password"));
                }
                ask_share_user(&ui, &label);
                return;
            }
            let mut options: Vec<(String, String)> = out.lines()
                .filter_map(|l| l.strip_prefix("share="))
                .map(|l| {
                    let (name, comment) = l.split_once('\t').unwrap_or((l, ""));
                    (name.to_owned(), if comment.trim().is_empty() { name.to_owned() } else { format!("{name} · {}", comment.trim()) })
                })
                .collect();
            if options.is_empty() {
                type_share_folder(&ui);
                return;
            }
            options.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
            options.push(("manual".into(), tr(&ui, 1285, "Type a Folder Name")));
            open_settings_choice(&ui, "files-sharefolder", &tr_arg(&ui, 1284, "Choose a Folder on {0}", &label), options, "");
        });
    });
}

fn ask_share_user(ui: &HomeWindow, server: &str) {
    open_system_keyboard(ui, &tr_arg(ui, 1176, "User name on {0} (empty = guest)", server), KEYBOARD_SHARE_USER,
        FILES_VIEW, "text", "");
}

/* A share name for nuubos-sharesctl from the folder: letters, digits,
 * - and _, at most 32, unique among the configured shares. */
fn share_name(folder: &str, taken: &[String]) -> String {
    let mut base: String = folder
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect::<String>()
        .trim_matches(['-', '_'])
        .chars()
        .take(28)
        .collect();
    if base.is_empty() {
        base = "share".into();
    }
    let mut name = base.clone();
    let mut n = 2;
    while taken.iter().any(|t| t.eq_ignore_ascii_case(&name)) {
        name = format!("{base}-{n}");
        n += 1;
    }
    name
}

/* Add, connect and open the share; if it cannot connect it is not kept
 * (most often a typo) and the reason is shown. */
fn finish_add_share(ui: &HomeWindow, password: String) {
    let (server, folder, user, taken) = STATE.with(|st| {
        let st = st.borrow();
        let (s, f, u) = st.share_draft.clone();
        (s, f, u, st.shares.iter().map(|x| x.0.clone()).collect::<Vec<_>>())
    });
    if server.is_empty() || folder.is_empty() {
        return;
    }
    let name = share_name(&folder, &taken);
    let url = format!("//{server}/{folder}");
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        st.busy = true;
        st.progress = 0.0;
        st.progress_label = tr_arg(ui, 1182, "Connecting to {0}…", &format!("{server}/{folder}"));
    });
    render_files(ui);
    let weak = ui.as_weak();
    thread::spawn(move || {
        let mut args = vec!["add", name.as_str(), url.as_str()];
        if !user.is_empty() {
            args.push(user.as_str());
        }
        /* The password goes on stdin, never on the command line. */
        let added = run_tool(SHARESCTL, &args, Some(password));
        let mounted = if added.starts_with("OK") { run_tool(SHARESCTL, &["mount", &name], None) } else { added.clone() };
        if added.starts_with("OK") && !mounted.starts_with("OK") {
            let _ = run_tool(SHARESCTL, &["remove", &name], None);
        }
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else { return };
            let label = STATE.with(|st| {
                let mut st = st.borrow_mut();
                st.busy = false;
                st.share_label.clone()
            });
            /* A guest connection the server refuses: ask for a login and
             * try again with the same folder. */
            if mounted.contains("auth") && user.is_empty() {
                render_files(&ui);
                ask_share_user(&ui, if label.is_empty() { &server } else { &label });
                return;
            }
            STATE.with(|st| {
                let mut st = st.borrow_mut();
                st.share_draft = Default::default();
                st.share_secret.clear();
            });
            if mounted.starts_with("OK") {
                files_notice(&ui, tr_arg(&ui, 1178, "Connected to {0}", &folder));
                enter(&ui, format!("{SHARES_ROOT}/{name}"));
            } else if added.starts_with("ERR") {
                files_notice(&ui, tr(&ui, 93, "Failed"));
                refresh(&ui);
            } else {
                files_notice(&ui, share_error(&ui, &mounted, if mounted.contains("share") { &folder } else { &server }));
                refresh(&ui);
            }
        });
    });
}

/* Dropdown choices opened by this module. */
pub fn apply_choice(ui: &HomeWindow, context: &str, value: String) {
    match context {
        "files-action" => {
            if let Some((verb, name)) = value.split_once(':') {
                let name = name.to_owned();
                match verb {
                    "eject" => tool_then(ui, MEDIACTL, vec!["eject".into(), name], None, |ui, out| {
                        files_notice(ui, if out.starts_with("OK") { tr(ui, 627, "Safe to remove") }
                            else { tr(ui, 628, "In use; close what uses it first") });
                        refresh(ui);
                    }),
                    "share-mount" | "share-umount" | "share-remove" => {
                        let cmd = match verb { "share-mount" => "mount", "share-umount" => "umount", _ => "remove" };
                        let weak = ui.as_weak();
                        thread::spawn(move || {
                            let out = run_tool(SHARESCTL, &[cmd, &name], None);
                            let _ = slint::invoke_from_event_loop(move || {
                                let Some(ui) = weak.upgrade() else { return };
                                if out.starts_with("ERR") && cmd == "mount" {
                                    files_notice(&ui, share_error(&ui, &out, &name));
                                }
                                refresh(&ui);
                            });
                        });
                    }
                    _ => {}
                }
                return;
            }
            match value.as_str() {
                "copy" | "move" => transfer(ui, value == "move"),
                "rename" => {
                    let name = STATE.with(|st| focused_entry(&st.borrow()).map(|e| e.name).unwrap_or_default());
                    open_system_keyboard(ui, &tr(ui, 617, "Rename"), KEYBOARD_RENAME, FILES_VIEW, "text", &name);
                }
                "delete" => {
                    let items = STATE.with(|st| targets(&st.borrow()));
                    let title = if items.len() == 1 { tr_arg(ui, 620, "Delete \"{0}\"?", base(&items[0])) }
                        else { tr_arg(ui, 1151, "Delete {0} items?", &items.len().to_string()) };
                    let options = vec![("delete".to_owned(), tr(ui, 618, "Delete")), ("cancel".to_owned(), tr(ui, 619, "Cancel"))];
                    open_settings_choice(ui, "files-delete", &title, options, "cancel");
                }
                "details" => details(ui),
                "mkdir" => open_system_keyboard(ui, &tr(ui, 613, "Folder name"), KEYBOARD_FOLDER, FILES_VIEW, "text", ""),
                "select-all" | "select-none" => {
                    STATE.with(|st| {
                        let mut st = st.borrow_mut();
                        let a = st.active;
                        let pane = &mut st.panes[a];
                        pane.marks = if value == "select-all" { pane.entries.iter().map(|e| e.name.clone()).collect() } else { BTreeSet::new() };
                    });
                    render_files(ui);
                }
                "same-folder" => {
                    STATE.with(|st| {
                        let mut st = st.borrow_mut();
                        let a = st.active;
                        let cwd = st.panes[a].cwd.clone();
                        let other = &mut st.panes[1 - a];
                        other.cwd = cwd;
                        other.index = 0;
                        other.marks.clear();
                    });
                    refresh(ui);
                }
                "sort" => cycle_sort(ui),
                "add-share" => start_add_share(ui),
                "add-library" => start_import(ui),
                "play" => {
                    let path = STATE.with(|st| targets(&st.borrow()).into_iter().next()).unwrap_or_default();
                    if path.is_empty() {
                        return;
                    }
                    let weak = ui.as_weak();
                    thread::spawn(move || {
                        let reply = emulation(&format!("LAUNCH_FILE\t{path}"), false);
                        if reply.trim() != "OK" {
                            let _ = slint::invoke_from_event_loop(move || {
                                let Some(ui) = weak.upgrade() else { return };
                                files_notice(&ui, tr(&ui, 1420, "Could not play this file"));
                            });
                        }
                    });
                }
                _ => {}
            }
        }
        "files-delete" if value == "delete" => delete_targets(ui),
        "files-server" => {
            if value == "manual" {
                type_share_server(ui);
                return;
            }
            let (ip, name) = value.split_once('\t').unwrap_or((value.as_str(), value.as_str()));
            STATE.with(|st| {
                let mut st = st.borrow_mut();
                st.share_draft = (ip.to_owned(), String::new(), String::new());
                st.share_label = name.to_owned();
            });
            browse_shares(ui);
        }
        "files-sharefolder" => {
            if value == "manual" {
                type_share_folder(ui);
                return;
            }
            let secret = STATE.with(|st| {
                let mut st = st.borrow_mut();
                st.share_draft.1 = value.clone();
                st.share_secret.clone()
            });
            finish_add_share(ui, secret);
        }
        "files-import" => answer_import(ui, value),
        _ => {}
    }
}

/* System keyboard results for this module's purposes. */
pub fn keyboard_done(ui: &HomeWindow, purpose: i32, value: String) {
    let trimmed = value.trim().to_owned();
    match purpose {
        KEYBOARD_FOLDER | KEYBOARD_RENAME => {
            navigate_settings_view(ui, FILES_VIEW);
            render_files(ui);
            if trimmed.is_empty() {
                return;
            }
            let (cwd, focused) = STATE.with(|st| {
                let st = st.borrow();
                (st.panes[st.active].cwd.clone(), focused_entry(&st))
            });
            STATE.with(|st| st.borrow_mut().focused = trimmed.clone());
            let args = if purpose == KEYBOARD_FOLDER {
                vec!["mkdir".to_owned(), format!("{cwd}/{trimmed}")]
            } else {
                let Some(e) = focused else { return };
                vec!["rename".to_owned(), format!("{cwd}/{}", e.name), trimmed]
            };
            tool_then(ui, FILESCTL, args, None, |ui, out| {
                if out.starts_with("ERR") {
                    files_notice(ui, tr(ui, 93, "Failed"));
                }
                refresh(ui);
            });
        }
        KEYBOARD_SHARE_NAME => {
            /* The server; a full address (//server/folder, smb://...,
             * \\server\folder) also gives the folder. */
            let addr = trimmed.trim_start_matches("smb://").trim_start_matches("cifs://")
                .trim_start_matches(['/', '\\']).replace('\\', "/");
            let (server, folder) = match addr.split_once('/') {
                Some((h, f)) => (h.to_owned(), f.trim_matches('/').to_owned()),
                None => (addr.clone(), String::new()),
            };
            if server.is_empty() {
                navigate_settings_view(ui, FILES_VIEW);
                render_files(ui);
                return;
            }
            STATE.with(|st| {
                let mut st = st.borrow_mut();
                st.share_draft = (server.clone(), folder.clone(), String::new());
                st.share_secret.clear();
                st.share_label = server.clone();
            });
            if folder.is_empty() {
                /* List its shared folders instead of asking for one. */
                navigate_settings_view(ui, FILES_VIEW);
                render_files(ui);
                browse_shares(ui);
            } else {
                ask_share_user(ui, &server);
            }
        }
        KEYBOARD_SHARE_URL => {
            if trimmed.is_empty() {
                navigate_settings_view(ui, FILES_VIEW);
                render_files(ui);
                return;
            }
            let (label, user, secret) = STATE.with(|st| {
                let mut st = st.borrow_mut();
                st.share_draft.1 = trimmed.trim_matches(['/', '\\']).to_owned();
                (st.share_label.clone(), st.share_draft.2.clone(), st.share_secret.clone())
            });
            if user.is_empty() {
                ask_share_user(ui, &label);
            } else {
                /* The login was given to list the folders. */
                navigate_settings_view(ui, FILES_VIEW);
                render_files(ui);
                finish_add_share(ui, secret);
            }
        }
        KEYBOARD_SHARE_USER => {
            let server = STATE.with(|st| {
                let mut st = st.borrow_mut();
                st.share_draft.2 = trimmed.clone();
                st.share_draft.0.clone()
            });
            if trimmed.is_empty() {
                /* Guest access: no password. Without a folder yet (the
                 * server did not list them as guest) it is typed. */
                if STATE.with(|st| st.borrow().share_draft.1.is_empty()) {
                    type_share_folder(ui);
                    return;
                }
                navigate_settings_view(ui, FILES_VIEW);
                render_files(ui);
                finish_add_share(ui, String::new());
            } else {
                open_system_keyboard(ui, &tr_arg(ui, 1177, "Password for {0}", &trimmed), KEYBOARD_SHARE_PASSWORD,
                    FILES_VIEW, "password", "");
                let _ = server;
            }
        }
        KEYBOARD_SHARE_PASSWORD => {
            ui.set_keyboard_value("".into());
            navigate_settings_view(ui, FILES_VIEW);
            render_files(ui);
            /* No folder yet: the login lists the server's folders. */
            let listing = STATE.with(|st| {
                let mut st = st.borrow_mut();
                st.share_secret = value.clone();
                st.share_draft.1.is_empty()
            });
            if listing {
                browse_shares(ui);
            } else {
                finish_add_share(ui, value);
            }
        }
        _ => {}
    }
}

/* ---------------------------------------------------------------- */
/* Settings pages: Removable Media, Backup, Recovery                 */
/* ---------------------------------------------------------------- */

fn build_rows(ui: &HomeWindow, st: &State, view: i32) -> Vec<(String, GenRow, bool)> {
    let mut rows: Vec<(String, GenRow, bool)> = Vec::new();
    let armed = armed();
    match view {
        61 => {
            if st.volumes.is_empty() {
                let mut r = row(tr(ui, 629, "No removable media"), String::new(), String::new());
                r.enabled = false;
                rows.push(("info".into(), r, false));
            }
            for (name, _path, total, free) in &st.volumes {
                let key = format!("vol:{name}");
                rows.push((key.clone(), row(volume_label(ui, name), tr_arg(ui, 622, "{0} free", &human(*free)),
                    armed_detail(&armed, &key, tr(ui, 191, "Press again to confirm"),
                        format!("{} • {}", tr(ui, 626, "Eject"), human(*total)))), true));
            }
        }
        BACKUP_VIEW => rows.extend(crate::storageui::backup_rows(ui)),
        RECOVERY_VIEW => {
            rows.extend(crate::storageui::restore_rows(ui));
            rows.extend(crate::recovery_rows(ui));
        }
        _ => {}
    }
    rows
}

/* Backup: one section, the console backup (user request 2026-10-12: no
 * separate profile backup). Recovery: RESTORE, then REPAIR & RESET. */
fn sections(ui: &HomeWindow, view: i32, rows: Vec<(String, GenRow, bool)>) {
    let device = tr(ui, 313, "DEVICE");
    if view == BACKUP_VIEW {
        apply_rows_sections(ui, rows, tr(ui, 1024, "BACKUP"), (0, String::new()), (0, String::new()), &[device]);
        return;
    }
    let reset = rows.iter().position(|r| crate::is_recovery_key(&r.0)).unwrap_or(rows.len()) as i32;
    apply_rows_sections(ui, rows, tr(ui, 1014, "RESTORE"), (reset, tr(ui, 983, "REPAIR & RESET")), (0, String::new()),
        &[device.clone(), device]);
}

pub(crate) fn render(ui: &HomeWindow) {
    let view = ui.get_settings_view();
    if view == FILES_VIEW || view == 60 {
        if ACTIVE.load(Ordering::SeqCst) {
            render_files(ui);
        }
        return;
    }
    let rows = STATE.with(|st| build_rows(ui, &st.borrow(), view));
    /* Keep the focus on the same entry after a refresh. */
    let focus = STATE.with(|st| std::mem::take(&mut st.borrow_mut().focused));
    if !focus.is_empty() {
        if let Some(i) = rows.iter().position(|r| r.0 == focus) {
            ui.set_gen_index(i as i32);
        }
    }
    if view == BACKUP_VIEW || view == RECOVERY_VIEW {
        sections(ui, view, rows);
    } else {
        apply_rows(ui, rows, tr(ui, 19, "Storage"));
    }
}

pub fn open_media(ui: &HomeWindow) {
    show(ui, 61);
    refresh_media(ui);
}

fn refresh_media(ui: &HomeWindow) {
    let weak = ui.as_weak();
    thread::spawn(move || {
        let volumes: Vec<(String, String, i64, i64)> = parse_kv_lines(&run_tool(MEDIACTL, &["list"], None), "volume")
            .into_iter()
            .filter(|f| f.len() >= 6)
            .map(|f| (f[0].into(), f[2].into(), f[3].parse().unwrap_or(0), f[4].parse().unwrap_or(0)))
            .collect();
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else { return };
            STATE.with(|st| st.borrow_mut().volumes = volumes);
            render(&ui);
        });
    });
}

pub fn open_backup(ui: &HomeWindow) {
    open_backup_at(ui, "");
}

/* With the focus on the row `key` (back from Backup Content). */
pub fn open_backup_at(ui: &HomeWindow, key: &str) {
    open_page(ui, BACKUP_VIEW, key);
}

/* Recovery; in safe mode on Exit Safe Mode. */
pub fn open_recovery(ui: &HomeWindow) {
    open_page(ui, RECOVERY_VIEW, if ui.get_safe_mode() { "safe-exit" } else { "" });
}

fn open_page(ui: &HomeWindow, view: i32, key: &str) {
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        st.busy = false;
        st.focused = key.to_owned();
    });
    ui.set_recovery_stage(0);
    show(ui, view);
    render(ui);
}

/* Input on Settings pages 61, 62 and 28 (gameui::handle_page). */
pub fn handle_page(ui: &HomeWindow, view: i32, action: &str) {
    match action {
        "menu_up" | "menu_down" => move_selection(ui, action),
        "menu_back" => if view == 61 { crate::storageui::open(ui) } else { navigate_settings_view(ui, 19) },
        "menu_confirm" => {
            let key = focused_key(ui);
            if (view == BACKUP_VIEW || view == RECOVERY_VIEW)
                && (crate::storageui::backup_confirm(ui, &key) || crate::recovery_confirm(ui, &key)) {
                return;
            }
            if let (61, k) = (view, key.as_str()) {
                if k.starts_with("vol:") && arm_or(ui, k) {
                    tool_then(ui, MEDIACTL, vec!["eject".into(), k[4..].to_owned()], None, |ui, out| {
                        notice(ui, if out.starts_with("OK") { tr(ui, 627, "Safe to remove") }
                            else { tr(ui, 628, "In use; close what uses it first") });
                        refresh_media(ui);
                    });
                }
            }
        }
        _ => {}
    }
}
