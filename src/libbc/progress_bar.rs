use std::fmt::Write;
use std::io::stdout;
use std::ops::Deref;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use anyhow::Result;
use chrono::{DateTime, Local};
use colored_text::Colorize;
use crossterm::{cursor, execute};
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressState, ProgressStyle};
use log::error;
use tokio::task::JoinHandle;

use crate::ceil;
use crate::format_duration;
use crate::models::shared_data_models::CurrentTrack;

static PROGRESS_BAR: Mutex<Option<ProgressBar>> = Mutex::new(None);
static TICK_ENABLED: AtomicBool = AtomicBool::new(true);
static QUIT_PENDING: AtomicBool = AtomicBool::new(false);

const QUIT_MESSAGE_ON: &str = "[Q] Finishing song — Esc: cancel / Ctrl+C: exit";
const QUIT_MESSAGE_OFF: &str = "    Finishing song — Esc: cancel / Ctrl+C: exit";

pub fn set_quit_pending(pending: bool) {
    if let Ok(progress_bar) = PROGRESS_BAR.lock() {
        QUIT_PENDING.store(pending, Ordering::Release);
        if let Some(bar) = progress_bar.as_ref() {
            bar.set_message(if pending { QUIT_MESSAGE_ON } else { "" });
        }
    }
}

#[allow(dead_code)]
fn refresh_song_info_on_screen(local_time: DateTime<Local>, unixtime: u64) {
    let start_date = local_time.timestamp() as u64;
    update_progress_bar(|p| {
        p.set_position(unixtime - start_date);
    });
}

pub fn update_song_info_on_screen(item: &CurrentTrack) -> Result<()> {
    let total_seconds: i32 = ceil!(item.duration, 1.0) as i32; // Note: This may be 0

    update_progress_bar(|p| p.finish_and_clear());

    let dt = item.play_date;
    let dtf = dt.format("%H:%M:%S").to_string();

    println!("{}\r", dtf.rgb(90, 91, 103));
    println!("{:<11} {}\r", "Song:".rgb(146, 49, 176), item.track);
    println!("{:<11} {}\r", "Artist:".rgb(126, 87, 194), item.artist_name);
    println!("{:<11} {}\r", "Album:".rgb(121, 134, 203), item.album_title);

    let progress_bar_len = if total_seconds > 0 {
        total_seconds as u64
    } else {
        u64::MAX
    };

    let progress_bar_style = ProgressStyle::with_template(
        "{prefix}  {wide_bar} {progress_info} {spinner:.dim.bold} {msg}",
    )?
    .tick_chars("⠁⠂⠄⡀⠄⠂ ")
    .with_key(
        "progress_info",
        move |state: &ProgressState, write: &mut dyn Write| {
            let progress_info = get_progress_bar_progress_info(state.pos(), state.len());
            write!(write, "{progress_info}").unwrap();
        },
    );

    let prog_bar = ProgressBar::new(progress_bar_len)
        .with_style(progress_bar_style)
        .with_position(0);

    if let Ok(mut progress_bar) = PROGRESS_BAR.lock() {
        progress_bar.replace(prog_bar);
    }
    Ok(())
}

fn get_progress_bar_progress_info(elapsed_seconds: u64, total_seconds: Option<u64>) -> String {
    let humanized_elapsed_duration = format_duration!(elapsed_seconds);

    if let Some(total_seconds) = total_seconds {
        if total_seconds != u64::MAX {
            let humanized_total_duration = format_duration!(total_seconds);
            return format!("{humanized_elapsed_duration} / {humanized_total_duration}");
        }
    }
    humanized_elapsed_duration
}

pub fn enable_tick() {
    TICK_ENABLED.store(true, Ordering::Release);
}

pub fn disable_tick() {
    TICK_ENABLED.store(false, Ordering::Release);
}

pub fn enable_tick_on_screen() {
    if let Ok(a) = PROGRESS_BAR.lock() {
        if let Some(b) = a.deref() {
            // Resetting a visible draw target loses indicatif's tracked row count.
            // AI generation restores the bar before the request and again on exit;
            // the second reset would leave the previous bar on the terminal.
            if b.is_hidden() {
                b.set_draw_target(ProgressDrawTarget::stdout());
                b.tick();
            }
        }
    }
}

pub fn disable_tick_on_screen() {
    match PROGRESS_BAR.lock() {
        Ok(mut a) => {
            if let Some(bar) = a.as_ref() {
                // Clear using indicatif's tracked row count before discarding its draw target.
                // Replacing the target directly leaves the old bar on the normal screen.
                let hidden =
                    ProgressBar::with_draw_target(bar.length(), ProgressDrawTarget::hidden())
                        .with_style(bar.style())
                        .with_position(bar.position());
                bar.finish_and_clear();
                *a = Some(hidden);
                execute!(stdout(), cursor::Hide).unwrap();
            }
        }
        Err(e) => {
            error!("{}", e);
        }
    }
}

pub fn destroy() {
    match PROGRESS_BAR.lock() {
        Ok(a) => {
            if let Some(a) = a.to_owned() {
                a.finish_and_clear()
            }
        }
        Err(e) => error!("Error: {}", e),
    }
}

/// Increase elapsed seconds in progress bar by 1 every second.
async fn tick_progress_bar_progress() {
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    let mut blink_on = false;
    loop {
        interval.tick().await;
        if TICK_ENABLED.load(Ordering::Acquire) {
            update_progress_bar(|p| p.inc(1));
        }
        update_progress_bar(|p| {
            if QUIT_PENDING.load(Ordering::Acquire) {
                blink_on = !blink_on;
                p.set_message(if blink_on {
                    QUIT_MESSAGE_ON
                } else {
                    QUIT_MESSAGE_OFF
                });
            } else {
                blink_on = false;
            }
        });
    }
}

pub async fn run() -> JoinHandle<()> {
    tokio::spawn(tick_progress_bar_progress())
}

pub fn enable_spinner() {
    if TICK_ENABLED.load(Ordering::Acquire) {
        match PROGRESS_BAR.lock() {
            Ok(a) => {
                if let Some(a) = a.to_owned() {
                    a.enable_steady_tick(Duration::from_millis(100))
                }
            }
            Err(e) => println!("Error: {}", e),
        }
    }
}

pub fn disable_spinner() {
    if TICK_ENABLED.load(Ordering::Acquire) {
        match PROGRESS_BAR.lock() {
            Ok(a) => {
                if let Some(a) = a.to_owned() {
                    a.disable_steady_tick()
                }
            }
            Err(e) => println!("Error: {}", e),
        }
    }
}

/// A visible status for the AI request and Bandcamp resolution, including startup.
pub struct GenerationStatus {
    bar: ProgressBar,
    standalone: bool,
}

impl GenerationStatus {
    pub fn new(during_playback: bool) -> Self {
        let existing = if during_playback {
            PROGRESS_BAR.lock().ok().and_then(|bar| bar.clone())
        } else {
            None
        };
        let standalone = existing.is_none();
        let bar = existing.unwrap_or_else(ProgressBar::new_spinner);
        bar.set_message("Creating AI playlist (searching Bandcamp)...");
        bar.enable_steady_tick(Duration::from_millis(100));
        Self { bar, standalone }
    }
}

impl Drop for GenerationStatus {
    fn drop(&mut self) {
        self.bar.disable_steady_tick();
        if self.standalone {
            self.bar.finish_and_clear();
        } else {
            self.bar.set_message("");
        }
    }
}

fn update_progress_bar<T>(action: T)
where
    T: FnOnce(&ProgressBar),
{
    if let Ok(progress_bar) = PROGRESS_BAR.lock() {
        if let Some(progress_bar) = progress_bar.as_ref() {
            action(progress_bar);
        }
    }
}
