/*
 * Settings → System → Storage of nuubUI, on the generic rows of home.slint
 * (see gameui.rs), and the games card's moments outside it
 * (docs/dev/platform.md "SD2 = games card"):
 *
 *   64 Storage          SD1, the games card (SD2) and its actions
 *   62 Backup           the console backup (one for everything, user
 *                       request 2026-10-12): automatic, destination
 *                       SD2/SD1/USB, content, Back Up Now
 *   28 Recovery         its RESTORE section: a backup from any place,
 *                       then the whole console or one profile (both
 *                       pages are filesui's: backup_rows/restore_rows/
 *                       backup_confirm)
 *   59 Backup Content   what the backup includes
 *   75 Data on SD2      each category of user data on SD1 or SD2
 *                       (GitHub #24, opt-in): choosing moves it
 *                       (storage-data-move job, progress on the row)
 *
 * The state comes from the nuubos-systemd snapshot (storage.*), pushed on
 * every change; commands go to nuubos-systemd (eject, ignore, use, backup
 * settings) or are jobs (sd2-format, sd2-prepare, sd2-backup, sd2-restore).
 * A card seen for the first time that needs a choice (no games: create the
 * folders?; unreadable: format?) asks once, over the Storage page, never
 * over a game. No storage policy lives here.
 */

use crate::gameui::{
    apply_rows, arm_or, armed, armed_detail, days_ago, focused_key, move_selection, nav, notice, row, run_tool,
    show, toggle,
};
use crate::{navigate_settings_view, open_settings_choice, tr, tr_arg, GenRow, HomeWindow};
use slint::ComponentHandle;
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

pub const STORAGE_VIEW: i32 = 64;
pub const CONTENT_VIEW: i32 = 59;
pub const DATA_VIEW: i32 = 75;

const SYSTEMCTL: &str = "/usr/bin/nuubos-systemctl";
const JOBCTL: &str = "/usr/bin/nuubos-jobctl";
const BACKUPCTL: &str = "/usr/sbin/nuubos-sd2-backup";

/* Backup items, in nuubos-sd2-backup order. */
const ITEMS: [&str; 6] = ["saves", "states", "library", "screenshots", "bios", "covers"];

/* Data on SD2 categories, in nuubos-storagectl order. */
const DATA: [&str; 5] = ["bios", "saves", "states", "screenshots", "library"];

fn data_title(ui: &HomeWindow, category: &str) -> String {
    match category {
        "bios" => tr(ui, 576, "BIOS Files"),
        "saves" => tr(ui, 947, "Saves"),
        "states" => tr(ui, 948, "Save States"),
        "screenshots" => tr(ui, 951, "Screenshots & Recordings"),
        _ => tr(ui, 953, "Covers & Metadata"),
    }
}

/* storage.* of the nuubos-systemd snapshot. */
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Storage {
    pub health: String,
    pub action: String,
    pub sd1_total: u64,
    pub sd1_free: u64,
    pub slot: bool,
    pub state: String,
    pub cid: String,
    pub fs: String,
    pub label: String,
    pub prompt: String,
    pub total: u64,
    pub free: u64,
    pub backup_enabled: bool,
    pub backup_items: String,
    pub backup_last: i64,
    pub backup_result: String,
    pub backup_card: String,
    pub backup_dest: String,
    pub backup_available: bool,
    pub backup_found: bool,
    pub data_on: String,
    pub data_state: String,
    pub data_moving: String,
}

impl Storage {
    pub fn parse(&mut self, key: &str, value: &str) {
        match key {
            "storage.health" => self.health = value.to_owned(),
            "storage.action_required" => self.action = value.to_owned(),
            "storage.userdata_total" => self.sd1_total = value.parse().unwrap_or(0),
            "storage.userdata_free" => self.sd1_free = value.parse().unwrap_or(0),
            "storage.sd2.slot" => self.slot = value == "1",
            "storage.sd2.state" => self.state = value.to_owned(),
            "storage.sd2.cid" => self.cid = value.to_owned(),
            "storage.sd2.fs" => self.fs = value.to_owned(),
            "storage.sd2.label" => self.label = value.to_owned(),
            "storage.sd2.prompt" => self.prompt = value.to_owned(),
            "storage.sd2.total" => self.total = value.parse().unwrap_or(0),
            "storage.sd2.free" => self.free = value.parse().unwrap_or(0),
            "storage.backup.enabled" => self.backup_enabled = value == "1",
            "storage.backup.items" => self.backup_items = value.to_owned(),
            "storage.backup.last" => self.backup_last = value.parse().unwrap_or(0),
            "storage.backup.result" => self.backup_result = value.to_owned(),
            "storage.backup.card" => self.backup_card = value.to_owned(),
            "storage.backup.dest" => self.backup_dest = value.to_owned(),
            "storage.backup.available" => self.backup_available = value == "1",
            "storage.backup.found" => self.backup_found = value == "1",
            "storage.data.on" => self.data_on = value.to_owned(),
            "storage.data.state" => self.data_state = value.to_owned(),
            "storage.data.moving" => self.data_moving = value.to_owned(),
            _ => {}
        }
    }

    fn has_item(&self, item: &str) -> bool {
        self.backup_items.split(',').any(|i| i == item)
    }

    fn data_on_sd2(&self, category: &str) -> bool {
        self.data_on.split(',').any(|c| c == category)
    }
}

#[derive(Default)]
struct State {
    storage: Storage,
    /* A job of this page runs (format, prepare, backup, restore). */
    busy: String,
    /* The card whose first-seen question was already asked. */
    asked: String,
    /* Backups found: (dir, label), this console's first. */
    backups: Vec<(String, String)>,
    /* The profiles of the chosen backup: (id, name). */
    backup_users: Vec<(String, String)>,
    /* The card the format confirmation is for. */
    format_cid: String,
    /* The backup being restored and its profile ("" = the whole console;
     * also for Try Again on the restore screen). */
    restore_dir: String,
    restore_user: String,
    /* The UI context before the operation screen (back on Close). */
    op_return_context: String,
    /* Data on SD2: the category being moved and its progress. */
    data_moving: String,
    data_progress: i32,
}

/* The full-screen operation screen (SD2 restore, system update) owns the
 * input while it is open. */
static OP_SCREEN: AtomicBool = AtomicBool::new(false);
pub const OP_RESTORE: i32 = 0;
pub const OP_UPDATE: i32 = 1;

pub fn op_screen_active() -> bool {
    OP_SCREEN.load(Ordering::SeqCst)
}

/* Open the operation screen at 0 %. */
pub fn show_op_screen(ui: &HomeWindow, kind: i32) {
    /* The Quick Menu stays closed and shows no card meanwhile. */
    if !OP_SCREEN.load(Ordering::SeqCst) {
        let current = std::fs::read_to_string(crate::UI_CONTEXT).unwrap_or_default().trim().to_owned();
        STATE.with(|st| st.borrow_mut().op_return_context = current);
        crate::write_ui_context("operation");
    }
    OP_SCREEN.store(true, Ordering::SeqCst);
    ui.set_op_kind(kind);
    ui.set_op_oob(crate::oob_active());
    ui.set_op_failed(false);
    ui.set_op_progress(0);
    ui.set_op_index(0);
    ui.set_op_screen_open(true);
}

/* The operation failed: Try Again | Start from Scratch / Close. */
pub fn op_failed(ui: &HomeWindow) {
    ui.set_op_failed(true);
    ui.set_op_index(0);
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

pub fn human(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1e9 {
        format!("{:.1} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.0} MB", b / 1e6)
    } else {
        format!("{:.0} KB", b / 1e3)
    }
}

fn space(ui: &HomeWindow, free: u64, total: u64) -> String {
    tr(ui, 914, "{0} free of {1}").replace("{0}", &human(free)).replace("{1}", &human(total))
}

/* Value of the System → Storage row. */
pub fn summary(ui: &HomeWindow, s: &Storage) -> String {
    let sd1 = tr(ui, 912, "SD1");
    match s.state.as_str() {
        "ready" | "readonly" => format!("{sd1} • {}", tr(ui, 64, "SD2")),
        _ => sd1,
    }
}

fn sd2_value(ui: &HomeWindow, s: &Storage) -> (String, String) {
    let name = if s.label.is_empty() { tr(ui, 915, "Games card") } else { s.label.clone() };
    match s.state.as_str() {
        "ready" => (space(ui, s.free, s.total), name),
        "readonly" => (tr(ui, 918, "Read-only"),
            tr(ui, 919, "The card has errors: games can be played, nothing can be written to it")),
        "checking" => (tr(ui, 917, "Checking…"), String::new()),
        "unreadable" => (tr(ui, 920, "Unreadable"), tr(ui, 921, "No supported file system (exFAT, FAT32, ext4)")),
        "ejected" => (tr(ui, 922, "Ejected"), tr(ui, 627, "Safe to remove")),
        "ignored" => (tr(ui, 978, "Not in use"), tr(ui, 980, "nuubOS leaves this card alone until you choose to use it")),
        _ => (tr(ui, 916, "No card"), String::new()),
    }
}

fn last_backup(ui: &HomeWindow, s: &Storage) -> String {
    if s.backup_last > 0 {
        tr_arg(ui, 933, "Last backup: {0}", &days_ago(ui, s.backup_last))
    } else {
        tr(ui, 934, "Never backed up")
    }
}

fn build_rows(ui: &HomeWindow, st: &State, view: i32) -> Vec<(String, GenRow, bool)> {
    let s = &st.storage;
    let mut rows: Vec<(String, GenRow, bool)> = Vec::new();
    let armed = armed();
    if view == CONTENT_VIEW {
        let mut profiles = toggle(tr(ui, 945, "Profiles & Settings"), true,
            tr(ui, 946, "Always included. Passwords and accounts are never copied."));
        profiles.enabled = false;
        rows.push(("profiles".into(), profiles, false));
        for (item, title, detail) in [
            ("saves", tr(ui, 947, "Saves"), String::new()),
            ("states", tr(ui, 948, "Save States"), String::new()),
            ("library", tr(ui, 949, "Game Library"), tr(ui, 950, "Play history and collections")),
            ("screenshots", tr(ui, 951, "Screenshots & Recordings"), String::new()),
            ("bios", tr(ui, 576, "BIOS Files"), String::new()),
            ("covers", tr(ui, 953, "Covers & Metadata"), tr(ui, 954, "Downloaded game information; it can take a lot of space")),
        ] {
            rows.push((format!("item:{item}"), toggle(title, s.has_item(item), detail), true));
        }
        return rows;
    }

    if view == DATA_VIEW {
        let moving = if st.data_moving.is_empty() { s.data_moving.clone() } else { st.data_moving.clone() };
        for category in DATA {
            let on = s.data_on_sd2(category);
            let mut r = row(data_title(ui, category), if on { tr(ui, 64, "SD2") } else { tr(ui, 912, "SD1") },
                if category == "library" { tr(ui, 954, "Downloaded game information; it can take a lot of space") }
                else { String::new() });
            if on && s.data_state == "offline" {
                r.detail = tr(ui, 1437, "Waiting for the SD2 card").into();
            }
            if moving == category {
                r.value = tr(ui, 1436, "Moving…").into();
                r.progress_shown = true;
                r.progress = st.data_progress.clamp(0, 100) as f32 / 100.0;
            }
            r.enabled = moving.is_empty();
            rows.push((format!("data:{category}"), r, moving.is_empty()));
        }
        return rows;
    }

    let busy = !st.busy.is_empty();
    let mut sd1 = row(tr(ui, 912, "SD1"), space(ui, s.sd1_free, s.sd1_total),
        tr(ui, 913, "System and saves"));
    if s.health == "DEGRADED" || s.health == "FAILED" {
        sd1.detail = tr(ui, 976, "Errors found: open Recovery").into();
    }
    rows.push(("sd1".into(), sd1, false));
    if s.slot {
        let (value, detail) = sd2_value(ui, s);
        rows.push(("sd2".into(), row(tr(ui, 64, "SD2"), value, detail), false));
        match s.state.as_str() {
            "ready" | "readonly" => {
                rows.push(("eject".into(), row(tr(ui, 924, "Eject SD2"), String::new(), String::new()), !busy));
            }
            "ejected" => {
                rows.push(("use".into(), row(tr(ui, 925, "Use SD2 Again"), String::new(), String::new()), !busy));
            }
            "ignored" => {
                rows.push(("use".into(), row(tr(ui, 979, "Use SD2"), String::new(), String::new()), !busy));
            }
            _ => {}
        }
        if s.state == "ready" {
            let mut prepare = row(tr(ui, 926, "Create Game Folders"), String::new(),
                tr(ui, 927, "Adds a folder for each system. Nothing is deleted."));
            if st.busy == "prepare" {
                prepare.value = tr(ui, 91, "Running").into();
            }
            rows.push(("prepare".into(), prepare, !busy));
        }
        if matches!(s.state.as_str(), "ready" | "readonly" | "unreadable" | "ignored") {
            let mut format = row(tr(ui, 928, "Format SD2"),
                if st.busy == "format" { tr(ui, 970, "Formatting…") } else { String::new() },
                armed_detail(&armed, "format", tr(ui, 930, "Press again • everything on SD2 will be erased"),
                    tr(ui, 929, "Erases everything on the card")));
            format.enabled = !busy;
            rows.push(("format".into(), format, !busy));
        }
        let on = DATA.iter().filter(|c| s.data_on_sd2(c)).count();
        let mut data = nav(tr(ui, 1435, "Data on SD2"),
            if on == 0 { tr(ui, 54, "Off") }
            else { tr(ui, 938, "{0} of {1}").replace("{0}", &on.to_string()).replace("{1}", &DATA.len().to_string()) });
        data.detail = if s.data_state == "offline" { tr(ui, 1437, "Waiting for the SD2 card") }
            else { tr(ui, 1439, "Saves, states, BIOS and covers") }.into();
        rows.push(("data".into(), data, true));
    }
    rows.push(("media".into(), nav(tr(ui, 625, "Removable Media"), String::new()), true));
    rows
}

fn dest_label(ui: &HomeWindow, dest: &str) -> String {
    match dest {
        "sd2" => tr(ui, 64, "SD2"),
        "usb" => tr(ui, 1016, "USB Drive"),
        _ => tr(ui, 912, "SD1"),
    }
}

/* Where a backup is (nuubos-sd2-backup list): sd2, sd1 or usb:<volume>. */
fn where_label(ui: &HomeWindow, place: &str) -> String {
    match place.strip_prefix("usb:") {
        Some(volume) => crate::filesui::volume_label(ui, volume),
        None => dest_label(ui, place),
    }
}

/* The console backup (System → Backup). */
fn console_backup_rows(ui: &HomeWindow, st: &State) -> Vec<(String, GenRow, bool)> {
    let s = &st.storage;
    let mut rows: Vec<(String, GenRow, bool)> = Vec::new();
    let armed = armed();
    let busy = !st.busy.is_empty();
    let writable = s.backup_available;
    let missing = match s.backup_dest.as_str() {
        "sd2" => tr(ui, 935, "Insert an SD2 card to back up"),
        "usb" => tr(ui, 1021, "Connect a USB drive to back up"),
        _ => String::new(),
    };
    let mut auto = toggle(tr(ui, 69, "Automatic Backup"), s.backup_enabled,
        if writable { last_backup(ui, s) } else { missing });
    if s.backup_result == "failed" {
        auto.detail = tr(ui, 968, "The backup could not be completed").into();
    }
    rows.push(("sd2-auto".into(), auto, true));
    let mut dest = nav(tr(ui, 1015, "Destination"), dest_label(ui, &s.backup_dest));
    if s.backup_dest == "sd1" {
        dest.detail = tr(ui, 1022, "Lost if SD1 is formatted or replaced").into();
    }
    rows.push(("sd2-dest".into(), dest, !busy));
    let on = ITEMS.iter().filter(|i| s.has_item(i)).count();
    rows.push(("sd2-content".into(), nav(tr(ui, 936, "Backup Content"),
        if on == ITEMS.len() { tr(ui, 937, "All") }
        else { tr(ui, 938, "{0} of {1}").replace("{0}", &on.to_string()).replace("{1}", &ITEMS.len().to_string()) }), true));
    if s.backup_card == "conflict" {
        rows.push(("sd2-archive".into(), row(tr(ui, 942, "Backup of a previous SD1"),
            if st.busy == "archive" { tr(ui, 91, "Running") } else { String::new() },
            armed_detail(&armed, "sd2-archive", tr(ui, 191, "Press again to confirm"),
                tr(ui, 943, "Restore it, or keep it aside and start a new backup"))), writable && !busy));
    } else {
        let mut now = row(tr(ui, 88, "Back Up Now"),
            if st.busy == "backup" { tr(ui, 966, "Backing up…") } else { String::new() }, String::new());
        now.enabled = writable;
        rows.push(("sd2-backup".into(), now, writable && !busy));
    }
    rows
}

pub fn backup_rows(ui: &HomeWindow) -> Vec<(String, GenRow, bool)> {
    STATE.with(|st| console_backup_rows(ui, &st.borrow()))
}

/* Recovery: Restore from Backup while a backup can be seen anywhere. */
pub fn restore_rows(ui: &HomeWindow) -> Vec<(String, GenRow, bool)> {
    STATE.with(|st| {
        let st = st.borrow();
        if !st.storage.backup_found {
            let mut none = row(tr(ui, 634, "No backups found"), String::new(), String::new());
            none.enabled = false;
            return vec![("sd2-restore-none".into(), none, false)];
        }
        let mut restore = nav(tr(ui, 1017, "Restore from Backup"),
            if st.busy == "restore" { tr(ui, 965, "Restoring…") } else { String::new() });
        restore.detail = tr(ui, 1020, "The whole console or a single profile").into();
        vec![("sd2-restore".into(), restore, st.busy.is_empty())]
    })
}

pub fn render(ui: &HomeWindow) {
    let view = ui.get_settings_view();
    if view == crate::filesui::BACKUP_VIEW || view == crate::filesui::RECOVERY_VIEW {
        crate::filesui::render(ui);
        return;
    }
    if view != STORAGE_VIEW && view != CONTENT_VIEW && view != DATA_VIEW {
        return;
    }
    let rows = STATE.with(|st| build_rows(ui, &st.borrow(), view));
    let section = match view {
        CONTENT_VIEW => tr(ui, 936, "Backup Content"),
        DATA_VIEW => tr(ui, 1435, "Data on SD2"),
        _ => tr(ui, 19, "Storage"),
    };
    apply_rows(ui, rows, section.to_uppercase());
}

/* System → Storage. */
pub fn open(ui: &HomeWindow) {
    show(ui, STORAGE_VIEW);
    render(ui);
}

/* A new nuubos-systemd snapshot. */
pub fn update(ui: &HomeWindow, s: &Storage) {
    let changed = STATE.with(|st| {
        let mut st = st.borrow_mut();
        let changed = st.storage != *s;
        st.storage = s.clone();
        changed
    });
    if changed {
        render(ui);
    }
    maybe_ask(ui);
}

/* Nothing in front that a question would interrupt. */
fn can_ask(ui: &HomeWindow) -> bool {
    !crate::library::GAME_RUNNING.load(Ordering::SeqCst)
        && !crate::oob_active()
        && !crate::mediaui::PLAYING.load(Ordering::SeqCst)
        && !crate::moonlight::STREAM_RUNNING.load(Ordering::SeqCst)
        && !crate::webui::RUNNING.load(Ordering::SeqCst)
        && !crate::terminalui::RUNNING.load(Ordering::SeqCst)
        && !ui.get_settings_choice_open()
}

/* The first-seen question for a card: once per card and session, on the
 * Storage page. Called on every snapshot and when a game ends. */
pub fn maybe_ask(ui: &HomeWindow) {
    let (prompt, cid, label) = STATE.with(|st| {
        let st = st.borrow();
        (st.storage.prompt.clone(), st.storage.cid.clone(), st.storage.label.clone())
    });
    if prompt == "none" || prompt.is_empty() || cid.is_empty() || !can_ask(ui) {
        return;
    }
    if STATE.with(|st| st.borrow().asked == cid) {
        return;
    }
    STATE.with(|st| st.borrow_mut().asked = cid.clone());
    if ui.get_settings_view() != STORAGE_VIEW {
        crate::open_settings_at(ui, STORAGE_VIEW);
        open(ui);
    }
    let name = if label.is_empty() { tr(ui, 64, "SD2") } else { label };
    if prompt == "prepare" {
        open_settings_choice(ui, "sd2-prompt", &format!("{} • {}", tr(ui, 955, "New SD2 card: no games found"), name),
            vec![("prepare".to_owned(), tr(ui, 926, "Create Game Folders")),
                 ("ignore".to_owned(), tr(ui, 956, "Don't Use"))], "prepare");
    } else {
        open_settings_choice(ui, "sd2-prompt", &tr(ui, 957, "SD2 can't be read"),
            vec![("format".to_owned(), tr(ui, 928, "Format SD2")),
                 ("ignore".to_owned(), tr(ui, 956, "Don't Use"))], "ignore");
    }
}

fn system(ui: &HomeWindow, args: Vec<String>, done: fn(&HomeWindow, String)) {
    let weak = ui.as_weak();
    thread::spawn(move || {
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let out = run_tool(SYSTEMCTL, &refs, None);
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                done(&ui, out);
            }
        });
    });
}

/* A job of this page: busy while it runs, then its outcome as a notice. */
fn job(ui: &HomeWindow, busy: &'static str, args: Vec<String>, done: fn(&HomeWindow, bool, String)) {
    STATE.with(|st| st.borrow_mut().busy = busy.to_owned());
    render(ui);
    let weak = ui.as_weak();
    thread::spawn(move || {
        let mut all = vec!["run".to_owned()];
        all.extend(args);
        let refs: Vec<&str> = all.iter().map(String::as_str).collect();
        let out = run_tool(JOBCTL, &refs, None);
        let ok = !out.starts_with("ERR") && !out.is_empty();
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else { return };
            STATE.with(|st| st.borrow_mut().busy.clear());
            done(&ui, ok, out.trim().to_owned());
            render(&ui);
        });
    });
}

fn start_format(ui: &HomeWindow, cid: String) {
    job(ui, "format", vec!["sd2-format".into(), cid], |ui, ok, out| {
        notice(ui, if ok { tr(ui, 971, "SD2 formatted") }
            /* The card with the user's data (Data on SD2). */
            else if out.ends_with(" data") { tr(ui, 1442, "SD2 holds your data: move it to SD1 first") }
            else { tr(ui, 972, "The card could not be formatted") });
    });
}

/* Data on SD2: a category to the other card, progress on its row. */
fn start_data_move(ui: &HomeWindow, category: &str, to: &str) {
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        st.data_moving = category.to_owned();
        st.data_progress = 0;
    });
    render(ui);
    let args = vec!["storage-data-move".to_owned(), category.to_owned(), to.to_owned()];
    crate::gameui::job_with_progress(ui, args, |ui, p| {
        STATE.with(|st| st.borrow_mut().data_progress = p);
        render(ui);
    }, |ui, result| {
        STATE.with(|st| st.borrow_mut().data_moving.clear());
        match result.strip_prefix("ERR ") {
            None => {
                let conflicts: u32 = result.trim().parse().unwrap_or(0);
                notice(ui, if conflicts > 0 {
                    tr_arg(ui, 1438, "{0} files were already there: the other copies are in the conflicts folder",
                        &conflicts.to_string())
                } else {
                    tr(ui, 92, "Completed")
                });
            }
            Some(error) if error.starts_with("cancelled") => {}
            Some(error) => notice(ui, match error.rsplit(' ').next().unwrap_or("") {
                "space" => tr(ui, 493, "Not enough free space"),
                "card" => tr(ui, 1443, "Insert the SD2 card with your data"),
                "game" => tr(ui, 628, "In use; close what uses it first"),
                _ => tr(ui, 1444, "The data could not be moved"),
            }),
        }
        render(ui);
    });
}

fn start_prepare(ui: &HomeWindow) {
    job(ui, "prepare", vec!["sd2-prepare".into()], |ui, ok, _| {
        notice(ui, if ok { tr(ui, 92, "Completed") } else { tr(ui, 93, "Failed") });
    });
}

fn start_backup(ui: &HomeWindow) {
    job(ui, "backup", vec!["sd2-backup".into()], |ui, ok, _| {
        notice(ui, if ok { tr(ui, 73, "Backup completed") } else { tr(ui, 968, "The backup could not be completed") });
    });
}

/* Restore, from Settings or the OOB, on the full-screen restore screen
 * (user request 2026-10-12): it follows the job's progress (jobctl
 * --progress: jobd's own events, no polling) and owns the input; every
 * service reloads the restored settings through the central restart path,
 * which takes over from the screen. */
pub fn start_restore(ui: &HomeWindow, dir: String, user: String) {
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        st.busy = "restore".to_owned();
        st.restore_dir = dir.clone();
        st.restore_user = user.clone();
    });
    show_op_screen(ui, OP_RESTORE);
    render(ui);
    let mut args = vec!["sd2-restore".to_owned(), dir];
    if !user.is_empty() {
        args.push(user);
    }
    crate::gameui::job_with_progress(ui, args, |ui, p| ui.set_op_progress(p), |ui, result| {
        STATE.with(|st| st.borrow_mut().busy.clear());
        if result == "restart-required" {
            ui.set_op_progress(100);
            thread::spawn(|| { let _ = run_tool(SYSTEMCTL, &["restart"], None); });
        } else {
            op_failed(ui);
        }
        render(ui);
    });
}

/* Input on the operation screen: nothing while it runs; after a failure
 * Try Again | Start from Scratch (OOB restore) / Close. */
pub fn handle_op_action(ui: &HomeWindow, action: &str) {
    if !ui.get_op_failed() {
        return;
    }
    match action {
        "menu_left" | "menu_up" => ui.set_op_index(0),
        "menu_right" | "menu_down" => ui.set_op_index(1),
        "menu_confirm" if ui.get_op_index() == 0 && ui.get_op_kind() == OP_UPDATE => crate::gameui::start_install(ui),
        "menu_confirm" if ui.get_op_index() == 0 => {
            let (dir, user) = STATE.with(|st| {
                let st = st.borrow();
                (st.restore_dir.clone(), st.restore_user.clone())
            });
            start_restore(ui, dir, user);
        }
        "menu_confirm" => {
            /* The OOB goes on as a new setup; Settings is back where it was. */
            OP_SCREEN.store(false, Ordering::SeqCst);
            let context = STATE.with(|st| std::mem::take(&mut st.borrow_mut().op_return_context));
            crate::write_ui_context(if context.is_empty() { "settings" } else { &context });
            ui.set_op_screen_open(false);
        }
        _ => {}
    }
}

/* Backups on SD2, SD1 and USB drives, this console's first, then the
 * newest. */
fn load_backups(ui: &HomeWindow, then: fn(&HomeWindow)) {
    let weak = ui.as_weak();
    thread::spawn(move || {
        let out = run_tool(BACKUPCTL, &["list"], None);
        let mut list: Vec<(bool, i64, String, String, String, String)> = out.lines()
            .filter_map(|l| l.strip_prefix("backup="))
            .map(|l| l.split('\t').collect::<Vec<_>>())
            .filter(|f| f.len() >= 7)
            .map(|f| (f[5] == "1", f[3].parse().unwrap_or(0), f[0].to_owned(), f[2].to_owned(), f[4].replace(',', ", "),
                f[6].to_owned()))
            .collect();
        list.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else { return };
            let backups = list.into_iter().map(|(this, created, dir, model, users, place)| {
                let who = if this { tr(&ui, 962, "This console") } else { model };
                let mut label = format!("{who} • {} • {}", days_ago(&ui, created), where_label(&ui, &place));
                if !users.is_empty() {
                    label.push_str(&format!(" • {users}"));
                }
                (dir, label)
            }).collect();
            STATE.with(|st| st.borrow_mut().backups = backups);
            then(&ui);
        });
    });
}

fn choose_backup(ui: &HomeWindow) {
    let options = STATE.with(|st| st.borrow().backups.clone());
    if options.is_empty() {
        return;
    }
    let first = options[0].0.clone();
    open_settings_choice(ui, "sd2-restore", &tr(ui, 1017, "Restore from Backup"), options, &first);
}

/* The chosen backup: the whole console or one of its profiles. */
fn choose_scope(ui: &HomeWindow, dir: String) {
    STATE.with(|st| st.borrow_mut().restore_dir = dir.clone());
    let weak = ui.as_weak();
    thread::spawn(move || {
        let out = run_tool(BACKUPCTL, &["users", &dir], None);
        let users: Vec<(String, String)> = out.lines()
            .filter_map(|l| l.strip_prefix("user="))
            .filter_map(|l| l.split_once('\t'))
            .map(|(id, name)| (id.to_owned(), name.to_owned()))
            .collect();
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else { return };
            let mut options = vec![("all".to_owned(), tr(&ui, 1018, "Entire Console"))];
            options.extend(users.iter().cloned());
            STATE.with(|st| st.borrow_mut().backup_users = users);
            open_settings_choice(&ui, "sd2-restore-scope", &tr(&ui, 1019, "What do you want to restore?"), options, "all");
        });
    });
}

/* OOB, after the language: a backup (SD2, USB drive) can bring everything
 * back instead of a new setup. */
pub fn offer_restore(ui: &HomeWindow) -> bool {
    if !STATE.with(|st| st.borrow().storage.backup_found) {
        return false;
    }
    load_backups(ui, |ui| {
        let mut options = STATE.with(|st| st.borrow().backups.clone());
        if options.is_empty() {
            return;
        }
        let first = options[0].0.clone();
        options.push(("scratch".to_owned(), tr(ui, 964, "Start from Scratch")));
        open_settings_choice(ui, "sd2-oob-restore", &tr(ui, 1023, "Backup found"), options, &first);
    });
    true
}

fn set_item(ui: &HomeWindow, item: &str, on: bool) {
    /* Shown at once; the snapshot confirms. */
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        let mut items: Vec<String> = st.storage.backup_items.split(',').filter(|i| !i.is_empty() && *i != item)
            .map(str::to_owned).collect();
        if on {
            items.push(item.to_owned());
        }
        st.storage.backup_items = ITEMS.iter().filter(|i| items.iter().any(|x| x == *i)).copied()
            .collect::<Vec<_>>().join(",");
    });
    render(ui);
    system(ui, vec!["set-storage-backup-item".into(), item.to_owned(), if on { "1" } else { "0" }.into()], |_, _| {});
}

fn confirm(ui: &HomeWindow, view: i32, key: &str) {
    let s = STATE.with(|st| st.borrow().storage.clone());
    if view == CONTENT_VIEW {
        if let Some(item) = key.strip_prefix("item:") {
            set_item(ui, item, !s.has_item(item));
        }
        return;
    }
    match key {
        "eject" => system(ui, vec!["sd2".into(), "eject".into()], |ui, out| {
            notice(ui, if out.starts_with("OK") { tr(ui, 627, "Safe to remove") }
                else { tr(ui, 628, "In use; close what uses it first") });
        }),
        "use" => system(ui, vec!["sd2".into(), "use".into()], |_, _| {}),
        "prepare" => start_prepare(ui),
        "format" => {
            if arm_or(ui, "format") {
                STATE.with(|st| st.borrow_mut().format_cid = s.cid.clone());
                start_format(ui, s.cid.clone());
            } else {
                render(ui);
            }
        }
        "media" => crate::filesui::open_media(ui),
        "data" => show(ui, DATA_VIEW),
        _ => {}
    }
}

/* A row of Data on SD2: SD1 or SD2. */
fn data_confirm(ui: &HomeWindow, key: &str) {
    let Some(category) = key.strip_prefix("data:") else { return };
    let (s, moving) = STATE.with(|st| {
        let st = st.borrow();
        (st.storage.clone(), !st.data_moving.is_empty())
    });
    if moving || !s.data_moving.is_empty() {
        return;
    }
    let current = if s.data_on_sd2(category) { "sd2" } else { "sd1" };
    open_settings_choice(ui, &format!("sd2-data:{category}"), &data_title(ui, category),
        vec![("sd1".to_owned(), tr(ui, 912, "SD1")), ("sd2".to_owned(), tr(ui, 64, "SD2"))], current);
}

/* Confirm on a row of Backup or on Restore from Backup of Recovery; false
 * when the key is not one of them. */
pub fn backup_confirm(ui: &HomeWindow, key: &str) -> bool {
    let s = STATE.with(|st| st.borrow().storage.clone());
    match key {
        "sd2-auto" => {
            STATE.with(|st| st.borrow_mut().storage.backup_enabled = !s.backup_enabled);
            render(ui);
            system(ui, vec!["set-storage-backup".into(), if s.backup_enabled { "0" } else { "1" }.into()], |_, _| {});
        }
        "sd2-dest" => {
            let mut options = Vec::new();
            if s.slot {
                options.push(("sd2".to_owned(), dest_label(ui, "sd2")));
            }
            options.push(("sd1".to_owned(), dest_label(ui, "sd1")));
            options.push(("usb".to_owned(), dest_label(ui, "usb")));
            open_settings_choice(ui, "sd2-dest", &tr(ui, 1015, "Destination"), options, &s.backup_dest);
        }
        "sd2-content" => show(ui, CONTENT_VIEW),
        "sd2-backup" => start_backup(ui),
        "sd2-archive" => {
            if arm_or(ui, "sd2-archive") {
                STATE.with(|st| st.borrow_mut().busy = "archive".into());
                system(ui, vec!["storage-backup-archive".into()], |ui, out| {
                    STATE.with(|st| st.borrow_mut().busy.clear());
                    if !out.starts_with("OK") {
                        notice(ui, tr(ui, 93, "Failed"));
                    }
                    render(ui);
                });
            }
            render(ui);
        }
        "sd2-restore" => load_backups(ui, choose_backup),
        _ => return false,
    }
    true
}

/* Input on pages 64, 59 and 75 (gameui::handle_page); 62 is filesui's. */
pub fn handle_page(ui: &HomeWindow, view: i32, action: &str) {
    match action {
        "menu_up" | "menu_down" => {
            move_selection(ui, action);
            render(ui);
        }
        "menu_back" => {
            if view == CONTENT_VIEW {
                crate::filesui::open_backup_at(ui, "sd2-content");
            } else if view == DATA_VIEW {
                open(ui);
                crate::gameui::focus(ui, "data");
                render(ui);
            } else {
                navigate_settings_view(ui, 19);
            }
        }
        "menu_confirm" if view == DATA_VIEW => data_confirm(ui, &focused_key(ui)),
        "menu_confirm" => confirm(ui, view, &focused_key(ui)),
        _ => {}
    }
}

/* Dropdown choices opened by this module. */
pub fn apply_choice(ui: &HomeWindow, context: &str, value: String) {
    match context {
        "sd2-prompt" => match value.as_str() {
            "prepare" => start_prepare(ui),
            "format" => {
                let (cid, size) = STATE.with(|st| {
                    let st = st.borrow();
                    (st.storage.cid.clone(), human(st.storage.total))
                });
                STATE.with(|st| st.borrow_mut().format_cid = cid);
                open_settings_choice(ui, "sd2-format", &tr_arg(ui, 958, "Erase everything on SD2 ({0})?", &size),
                    vec![("format".to_owned(), tr(ui, 959, "Format")), ("cancel".to_owned(), tr(ui, 619, "Cancel"))],
                    "cancel");
            }
            _ => system(ui, vec!["sd2".into(), "ignore".into()], |_, _| {}),
        },
        "sd2-format" if value == "format" => {
            let cid = STATE.with(|st| st.borrow().format_cid.clone());
            /* The card must still be the one the user confirmed. */
            if !cid.is_empty() && STATE.with(|st| st.borrow().storage.cid == cid) {
                start_format(ui, cid);
            }
        }
        "sd2-dest" => {
            /* Shown at once; the snapshot confirms. */
            STATE.with(|st| st.borrow_mut().storage.backup_dest = value.clone());
            render(ui);
            system(ui, vec!["set-storage-backup-dest".into(), value], |_, _| {});
        }
        "sd2-restore" => choose_scope(ui, value),
        "sd2-restore-scope" => {
            let label = STATE.with(|st| {
                let st = st.borrow();
                if value == "all" {
                    let backup = st.backups.iter().find(|b| b.0 == st.restore_dir).map(|b| b.1.clone()).unwrap_or_default();
                    format!("{} • {backup}", tr(ui, 1018, "Entire Console"))
                } else {
                    st.backup_users.iter().find(|u| u.0 == value).map(|u| u.1.clone()).unwrap_or_default()
                }
            });
            STATE.with(|st| st.borrow_mut().restore_user = if value == "all" { String::new() } else { value });
            open_settings_choice(ui, "sd2-restore-confirm", &tr_arg(ui, 960, "Restore \"{0}\"?", &label),
                vec![("restore".to_owned(), tr(ui, 635, "Restore")), ("cancel".to_owned(), tr(ui, 619, "Cancel"))],
                "cancel");
        }
        "sd2-restore-confirm" if value == "restore" => {
            let (dir, user) = STATE.with(|st| {
                let st = st.borrow();
                (st.restore_dir.clone(), st.restore_user.clone())
            });
            if !dir.is_empty() {
                start_restore(ui, dir, user);
            }
        }
        "sd2-oob-restore" if value != "scratch" => start_restore(ui, value, String::new()),
        _ if context.starts_with("sd2-data:") => {
            let category = &context["sd2-data:".len()..];
            let s = STATE.with(|st| st.borrow().storage.clone());
            let on = s.data_on_sd2(category);
            if (value == "sd2") == on {
                return;
            }
            if value == "sd2" && s.state != "ready" {
                notice(ui, tr(ui, 81, "Insert SD2"));
            } else if value == "sd1" && s.data_state != "online" {
                /* The card is gone: back on SD1 without what it holds. */
                open_settings_choice(ui, &format!("sd2-data-forget:{category}"),
                    &tr(ui, 1440, "Use SD1 without the card's data?"),
                    vec![("forget".to_owned(), tr(ui, 1441, "Use SD1")), ("cancel".to_owned(), tr(ui, 619, "Cancel"))],
                    "cancel");
            } else {
                start_data_move(ui, category, &value);
            }
        }
        _ if context.starts_with("sd2-data-forget:") && value == "forget" => {
            let category = context["sd2-data-forget:".len()..].to_owned();
            system(ui, vec!["storage-data-forget".into(), category], |_, _| {});
        }
        _ => {}
    }
}

/* Whether a dropdown context belongs to this module. */
pub fn owns_choice(context: &str) -> bool {
    context.starts_with("sd2-")
}
