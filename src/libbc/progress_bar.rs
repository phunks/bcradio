use std::fmt::Write;
use std::io::stdout;
use std::ops::Deref;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::Result;
use chrono::{DateTime, Local};
use colored_text::Colorize;
use crossterm::{cursor, execute};
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressState, ProgressStyle};
use log::{error, warn};
use tokio::task::JoinHandle;

use crate::ceil;
use crate::format_duration;
use crate::libbc::player::PROG;
use crate::models::shared_data_models::CurrentTrack;

static PROGRESS_BAR: Mutex<Option<ProgressBar>> = Mutex::new(None);

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
    println!(
        "{:<11} {}\r",
        "Artist:".rgb(126, 87, 194),
        item.artist_name
    );
    println!(
        "{:<11} {}\r",
        "Album:".rgb(121, 134, 203),
        item.album_title
    );

    let progress_bar_len = if total_seconds > 0 {
        total_seconds as u64
    } else {
        u64::MAX
    };

    let progress_bar_style = ProgressStyle::with_template(
        "{prefix}  {wide_bar} {progress_info} {spinner:.dim.bold} ",
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

    PROGRESS_BAR.lock().unwrap().replace(prog_bar);
    Ok(())
}

fn get_progress_bar_progress_info(
    elapsed_seconds: u64,
    total_seconds: Option<u64>,
) -> String {
    let humanized_elapsed_duration = format_duration!(elapsed_seconds);

    if let Some(total_seconds) = total_seconds {
        if total_seconds != u64::MAX {
            let humanized_total_duration = format_duration!(total_seconds);
            return format!("{humanized_elapsed_duration} / {humanized_total_duration}");
        }
    }
    humanized_elapsed_duration
}

pub fn get_progress_bar_current_position() -> Duration {
    PROGRESS_BAR.lock().unwrap().to_owned().unwrap().eta()
}

pub fn enable_tick() {
    *PROG.lock().unwrap() = true;
}

pub fn disable_tick() {
    *PROG.lock().unwrap() = false;
}

pub fn enable_tick_on_screen() {
    if let Ok(a) = PROGRESS_BAR.lock() {
        if let Some(b) = a.deref() {
            b.set_draw_target(ProgressDrawTarget::stdout());
        }
    }
    execute!(stdout(), cursor::MoveToColumn(0), cursor::Hide).unwrap();
}

pub fn disable_tick_on_screen() {
    match PROGRESS_BAR.lock() {
        Ok(a) => {
            while a.as_ref().is_some() {
                match a.as_ref() {
                    Some(b) => {
                        b.set_draw_target(ProgressDrawTarget::hidden());
                        execute!(stdout(), cursor::Hide).unwrap();
                        break;
                    }
                    None => {
                        warn!("retry lock progress bar");
                        continue;
                    }
                }
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
    loop {
        interval.tick().await;
        if *PROG.lock().unwrap() {
            update_progress_bar(|p| p.inc(1));
        }
    }
}

pub async fn run() -> JoinHandle<()> {
    tokio::spawn(tick_progress_bar_progress())
}

pub fn enable_spinner() {
    if *PROG.lock().unwrap() {
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
    if *PROG.lock().unwrap() {
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

fn update_progress_bar<T>(action: T)
where
    T: FnOnce(&ProgressBar),
{
    if let Some(progress_bar) = PROGRESS_BAR.lock().unwrap().as_ref() {
        action(progress_bar);
    }
}
