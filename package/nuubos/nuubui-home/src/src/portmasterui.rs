/*
 * PortMaster (EPIC-058): the nuubUI page of the PortMaster application.
 *
 * nuubos-portsd owns everything: downloading PortMaster on the user's
 * request (it is never part of the image), updating, restoring and removing
 * it, and running its own interface as a session like a game, in the ports
 * sandbox. This page only shows the portsd snapshot (library.rs's ports
 * listener hands it over) and sends commands. PortMaster's own interface
 * browses, installs and removes the ports; they appear under Ports in Home.
 *
 * Application page (AppPage, apppage.rs look) at settings-view 74, rows in
 * gen-index: absent → Download; installing → progress (A cancels);
 * installed → Open, Update / Check for Updates, Automatic Updates (portsd
 * installs a newer release when the page checks), Restore Version (the
 * release before the last update, when kept), Remove (second press; the
 * ports stay). Ports on SD2 (GitHub #21, boards with a card slot, also
 * before the download): portsd moves the ports and PortMaster to the SD2
 * card's ports folder (off: back to the console's card) as a job with
 * progress, A cancels; a port already at the destination keeps that copy.
 */

use crate::{
    move_model_selection, navigate_settings_view, play_ui_sound, tr, tr_arg, write_ui_context, DetailAction,
    DetailChip, HomeWindow,
};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::cmp::Ordering as CmpOrdering;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

pub const VIEW: i32 = 74;
const PORTS_SOCKET: &str = "/run/nuubos/portsd.sock";

/* The PortMaster page owns Home input. */
pub static ACTIVE: AtomicBool = AtomicBool::new(false);
/* The page's Remove is armed (second press removes). */
static REMOVE_ARMED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Default)]
pub struct Snapshot {
    pub installed: bool,
    pub version: String,
    pub latest: String,
    pub job: String,
    pub progress: i32,
    pub error: String,
    pub previous: String,
    pub auto_update: bool,
    /* Ports on SD2: chosen, the folder there (card in), a card slot. */
    pub on_sd2: bool,
    pub ready: bool,
    pub sd2_slot: bool,
    /* Ports the last move left on the other card (-1: no move yet). */
    pub conflicts: i32,
}

pub static SNAPSHOT: Mutex<Option<Snapshot>> = Mutex::new(None);

pub fn parse(block: &str) -> Snapshot {
    let mut s = Snapshot { progress: -1, ..Default::default() };
    for line in block.lines() {
        let Some((key, value)) = line.split_once('=') else { continue };
        match key {
            "portmaster" => s.installed = value == "installed",
            "portmaster_version" => s.version = value.to_owned(),
            "portmaster_latest" => s.latest = value.to_owned(),
            "portmaster_job" => s.job = value.to_owned(),
            "portmaster_progress" => s.progress = value.parse().unwrap_or(-1),
            "portmaster_error" => s.error = value.to_owned(),
            "portmaster_previous" => s.previous = value.to_owned(),
            "portmaster_auto_update" => s.auto_update = value == "1",
            "ports_location" => s.on_sd2 = value == "sd2",
            "ports_ready" => s.ready = value == "1",
            "ports_sd2_slot" => s.sd2_slot = value == "1",
            "ports_conflicts" => s.conflicts = value.parse().unwrap_or(-1),
            _ => {}
        }
    }
    s
}

/* PortMaster versions are dates with a build: "2026.06.23-0015". */
fn compare_versions(a: &str, b: &str) -> CmpOrdering {
    let parse = |v: &str| v.split(['.', '-']).map(|p| p.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    parse(a).cmp(&parse(b))
}

fn update_available(s: &Snapshot) -> bool {
    !s.latest.is_empty() && compare_versions(&s.latest, &s.version) == CmpOrdering::Greater
}

/* Actions of the page, in page order. */
fn page_actions(s: &Snapshot) -> Vec<&'static str> {
    let mut keys = match page_state(s) {
        "installing" | "moving" => return vec!["cancel"],
        "installed" => vec!["open", "update", "auto"],
        _ => vec!["download"],
    };
    if s.sd2_slot {
        keys.push("location");
    }
    if page_state(s) == "installed" {
        if !s.previous.is_empty() {
            keys.push("restore");
        }
        keys.push("remove");
    }
    keys
}

fn page_state(s: &Snapshot) -> &'static str {
    if s.job == "installing" {
        "installing"
    } else if s.job == "moving" {
        "moving"
    } else if s.installed {
        "installed"
    } else {
        "absent"
    }
}

fn row_count(s: &Snapshot) -> i32 {
    page_actions(s).len() as i32
}

/* The Ports on SD2 switch and its caption. */
fn location_action(ui: &HomeWindow, s: &Snapshot, actions: &mut Vec<DetailAction>, captions: &mut Vec<SharedString>) {
    let mut sd2 = action(27, tr(ui, 1430, "Ports on SD2"), false);
    sd2.on = s.on_sd2;
    actions.push(sd2);
    captions.push(if s.conflicts > 0 {
        tr_arg(ui, 1433, "{0} ports were already there: their copies stayed on the other card", &s.conflicts.to_string()).into()
    } else {
        tr(ui, 1431, "PortMaster and the ports in the ports folder of the SD2 card. The ports of the other card stay there.").into()
    });
}

/* A failed move (or download) as a warning chip. */
fn error_text(ui: &HomeWindow, error: &str) -> String {
    match error {
        "" => String::new(),
        "space" => tr(ui, 493, "Not enough free space"),
        "network" => tr(ui, 494, "Check the network connection"),
        "write" => tr(ui, 1434, "The ports could not be moved"),
        _ => tr(ui, 1361, "PortMaster could not be downloaded"),
    }
}

pub fn send(command: String) {
    thread::spawn(move || {
        let _ = command_reply(&command);
    });
}

fn command_reply(command: &str) -> std::io::Result<String> {
    let mut stream = UnixStream::connect(PORTS_SOCKET)?;
    stream.set_read_timeout(Some(Duration::from_secs(8)))?;
    stream.write_all(command.as_bytes())?;
    stream.write_all(b"\n")?;
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply)?;
    Ok(reply)
}

fn action(icon: i32, label: String, primary: bool) -> DetailAction {
    DetailAction { icon, label: label.into(), primary, on: false, enabled: true, armed: false, busy: false }
}

/* A new snapshot from portsd. */
pub fn apply(ui: &HomeWindow, s: Snapshot) {
    *SNAPSHOT.lock().unwrap() = Some(s.clone());
    if !ACTIVE.load(Ordering::SeqCst) {
        return;
    }
    let count = row_count(&s);
    if ui.get_gen_index() >= count {
        ui.set_gen_index(count - 1);
        REMOVE_ARMED.store(false, Ordering::SeqCst);
    }
    refresh(ui);
}

/* The page from the snapshot (AppPage). */
pub fn refresh(ui: &HomeWindow) {
    if !ACTIVE.load(Ordering::SeqCst) {
        return;
    }
    let s = SNAPSHOT.lock().unwrap().clone().unwrap_or_default();
    let name = tr(ui, 1358, "PortMaster");
    let mut meta = String::new();
    let mut chips: Vec<DetailChip> = Vec::new();
    let mut actions: Vec<DetailAction> = Vec::new();
    let mut captions: Vec<SharedString> = Vec::new();
    let mut progress = -1.0;
    let mut progress_label = String::new();
    let description = tr(
        ui,
        1367,
        "Open-source manager of ports: games and engines made for handhelds, downloaded over Wi-Fi. Some ports need files from a game you own.",
    );
    match page_state(&s) {
        "installing" => {
            let percent = s.progress.max(0);
            progress = percent as f32 / 100.0;
            progress_label = format!("{} • {}%", tr(ui, 1360, "Downloading PortMaster"), percent);
            actions.push(action(7, tr(ui, 482, "Press to cancel"), true));
            captions.push(SharedString::new());
        }
        "moving" => {
            let percent = s.progress.max(0);
            progress = percent as f32 / 100.0;
            progress_label = format!("{} • {}%", tr(ui, 1432, "Moving ports"), percent);
            actions.push(action(7, tr(ui, 482, "Press to cancel"), true));
            captions.push(SharedString::new());
        }
        "installed" => {
            let version = format!("{} {}", tr(ui, 485, "Version"), s.version);
            meta = version.clone();
            if s.job == "checking" {
                chips.push(DetailChip { icon: 6, text: tr(ui, 488, "Checking…").into(), warn: false });
            } else if !s.latest.is_empty() && !update_available(&s) {
                chips.push(DetailChip { icon: 6, text: tr(ui, 489, "Up to date").into(), warn: false });
            }
            /* A move that failed. */
            if !s.error.is_empty() {
                chips.push(DetailChip { icon: 13, text: error_text(ui, &s.error).into(), warn: true });
            }
            actions.push(action(0, tr(ui, 1362, "Open PortMaster"), true));
            captions.push(tr(ui, 1363, "Browse, install and update ports over Wi-Fi").into());
            let update = if update_available(&s) {
                tr_arg(ui, 486, "Update to {0}", &s.latest)
            } else {
                tr(ui, 487, "Check for Updates")
            };
            actions.push(action(5, update, false));
            captions.push(version.into());
            let mut auto = action(4, tr(ui, 852, "Automatic Updates"), false);
            auto.on = s.auto_update;
            actions.push(auto);
            captions.push(tr(ui, 1368, "New versions install when this page checks for updates").into());
            if s.sd2_slot {
                location_action(ui, &s, &mut actions, &mut captions);
            }
            if !s.previous.is_empty() {
                actions.push(action(9, tr_arg(ui, 853, "Restore Version {0}", &s.previous), false));
                captions.push(SharedString::new());
            }
            let armed = REMOVE_ARMED.load(Ordering::SeqCst);
            let mut remove = action(10, tr(ui, 1364, "Remove PortMaster"), false);
            remove.armed = armed;
            actions.push(remove);
            captions.push(if armed {
                tr(ui, 191, "Press again to confirm").into()
            } else {
                tr(ui, 1365, "The installed ports stay, but need PortMaster to start").into()
            });
        }
        _ => {
            let detail = error_text(ui, &s.error);
            if !detail.is_empty() {
                chips.push(DetailChip { icon: 13, text: detail.clone().into(), warn: true });
            }
            /* Ports on SD2 while the card is out. */
            if s.on_sd2 && !s.ready {
                chips.push(DetailChip { icon: 13, text: tr(ui, 81, "Insert SD2").into(), warn: true });
            }
            let mut download = action(4, tr(ui, 1359, "Download PortMaster"), true);
            download.enabled = !s.on_sd2 || s.ready;
            actions.push(download);
            captions.push(detail.into());
            if s.sd2_slot {
                location_action(ui, &s, &mut actions, &mut captions);
            }
        }
    }
    ui.set_app_page_kicker(name.to_uppercase().into());
    ui.set_app_page_title(name.into());
    ui.set_app_page_meta(meta.into());
    ui.set_app_page_description(description.into());
    ui.set_app_page_chips(ModelRc::from(Rc::new(VecModel::from(chips))));
    ui.set_app_page_entries(ModelRc::default());
    ui.set_app_page_actions(ModelRc::from(Rc::new(VecModel::from(actions))));
    ui.set_app_page_captions(ModelRc::from(Rc::new(VecModel::from(captions))));
    ui.set_app_page_pin("".into());
    ui.set_app_page_progress(progress);
    ui.set_app_page_progress_label(progress_label.into());
}

pub fn open(ui: &HomeWindow) {
    ACTIVE.store(true, Ordering::SeqCst);
    REMOVE_ARMED.store(false, Ordering::SeqCst);
    ui.set_portmaster_active(true);
    ui.set_gen_index(0);
    ui.set_gen_notice("".into());
    navigate_settings_view(ui, VIEW);
    ui.set_settings_open(true);
    write_ui_context("portmaster");
    refresh(ui);
    /* The ports folder now: an SD2 card may have come or gone. */
    send("REFRESH".into());
    /* One request to PortMaster's releases: is there a newer one? */
    let s = SNAPSHOT.lock().unwrap().clone();
    if let Some(s) = s {
        if s.installed && s.job == "idle" {
            send("PM_CHECK".into());
        }
    }
}

/* Back to Home (also when the user picker takes over). */
pub fn leave(ui: &HomeWindow) {
    ACTIVE.store(false, Ordering::SeqCst);
    navigate_settings_view(ui, 0);
    ui.set_settings_selected_index(0);
    ui.set_settings_open(false);
    ui.set_portmaster_active(false);
    write_ui_context("home");
}

/* Ports on SD2 switched: portsd moves them; a card that is out says so. */
fn move_ports(ui: &HomeWindow, to_sd2: bool) {
    let weak = ui.as_weak();
    thread::spawn(move || {
        let reply = command_reply(&format!("LOCATION\t{}", if to_sd2 { "sd2" } else { "console" })).unwrap_or_default();
        if reply.trim() != "ERR card" {
            return;
        }
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                play_ui_sound("error");
                ui.set_gen_notice(tr(&ui, 81, "Insert SD2").into());
            }
        });
    });
}

fn launch(ui: &HomeWindow) {
    let weak = ui.as_weak();
    ui.set_gen_notice("".into());
    thread::spawn(move || {
        let reply = command_reply("OPEN").unwrap_or_default();
        if reply.trim() == "OK" || reply.trim() == "ERR busy" {
            /* Started (Home hides under it), or a double press. */
            return;
        }
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                play_ui_sound("error");
                ui.set_gen_notice(tr(&ui, 1366, "PortMaster could not be started").into());
            }
        });
    });
}

pub fn handle_action(ui: &HomeWindow, action: &str) {
    let Some(s) = SNAPSHOT.lock().unwrap().clone() else {
        if action == "menu_back" {
            play_ui_sound(action);
            leave(ui);
        }
        return;
    };
    let index = ui.get_gen_index();
    let count = row_count(&s);
    play_ui_sound(action);
    match action {
        "menu_up" | "menu_down" | "menu_left" | "menu_right" => {
            let back = action == "menu_up" || action == "menu_left";
            ui.set_gen_index(move_model_selection(index, count, if back { -1 } else { 1 }));
            REMOVE_ARMED.store(false, Ordering::SeqCst);
            ui.set_gen_notice("".into());
            refresh(ui);
            return;
        }
        "menu_back" => {
            leave(ui);
            return;
        }
        "menu_confirm" => {}
        _ => return,
    }
    ui.set_gen_notice("".into());
    let key = page_actions(&s).get(index.max(0) as usize).copied().unwrap_or("");
    match (page_state(&s), key) {
        (_, "location") => move_ports(ui, !s.on_sd2),
        ("absent", _) if s.on_sd2 && !s.ready => {}
        ("absent", _) => send("PM_INSTALL".into()),
        ("installing" | "moving", _) => send("PM_CANCEL".into()),
        (_, "open") => launch(ui),
        (_, "update") if update_available(&s) => send("PM_INSTALL".into()),
        (_, "update") => send("PM_CHECK".into()),
        (_, "auto") => send(format!("PM_AUTO_UPDATE\t{}", if s.auto_update { 0 } else { 1 })),
        (_, "restore") => send("PM_ROLLBACK".into()),
        (_, "remove") if REMOVE_ARMED.load(Ordering::SeqCst) => {
            REMOVE_ARMED.store(false, Ordering::SeqCst);
            ui.set_gen_index(0);
            send("PM_REMOVE".into());
        }
        (_, "remove") => REMOVE_ARMED.store(true, Ordering::SeqCst),
        _ => {}
    }
    refresh(ui);
}
