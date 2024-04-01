use std::fmt::Write;
use std::io::stdout;
use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Local};
use colored::Colorize;
use crossterm::{cursor, execute};
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressState, ProgressStyle};
use tokio::task::JoinHandle;

use crate::format_duration;
use crate::ceil;
use crate::libbc::player::PROG;
use crate::models::shared_data_models::CurrentTrack;

#[async_trait]
pub(crate) trait Progress<'a> {
    fn new() -> Self;
    fn refresh_song_info_on_screen(&self, local_time: DateTime<Local>, unixtime: u64);
    fn update_song_info_on_screen(&self, item: CurrentTrack) -> Result<()>;
    fn get_progress_bar_progress_info(
        &self,
        elapsed_seconds: u64,
        total_seconds: Option<u64>,
    ) -> String;
    fn get_progress_bar_current_position(&self) -> Duration;
    fn enable_tick(&self);
    fn disable_tick(&self);
    fn enable_tick_on_screen(&self);
    fn disable_tick_on_screen(&self);
    fn destroy(&self);
    async fn tick_progress_bar_progress(mut self);
    async fn run(mut self) -> JoinHandle<()>;
    fn enable_spinner(&self);
    fn disable_spinner(&self);
}

#[derive(Default, Debug)]
pub struct Bar<'a> {
    mutex_progress_bar: Arc<Mutex<Option<ProgressBar>>>,
    phantom: PhantomData<&'a ()>,
}

impl Clone for Bar<'_> {
    fn clone(&self) -> Self {
        Self {
            mutex_progress_bar: Arc::new(Mutex::from(
                self.mutex_progress_bar.lock().unwrap().clone(),
            )),
            phantom: PhantomData,
        }
    }
}

static PROGRESS_BAR: Mutex<Option<ProgressBar>> = Mutex::new(None);

#[async_trait]
impl Progress<'static> for Bar<'static> {
    fn new() -> Self {
        Default::default()
    }

    fn refresh_song_info_on_screen(&self, local_time: DateTime<Local>, unixtime: u64) {
        let start_date = local_time.timestamp() as u64;
        self.update_progress_bar(|p| {
            p.set_position(unixtime - start_date);
        });
    }

    fn update_song_info_on_screen(&self, item: CurrentTrack) -> Result<()> {
        let total_seconds: i32 = ceil!(item.duration, 1.0) as i32; // Note: This may be 0
        // New song
        self.update_progress_bar(|p| p.finish_and_clear());

        let dt = item.play_date;
        let dtf = dt.format("%H:%M:%S").to_string();

        println!("{}\r", dtf.truecolor(90, 91, 103));
        println!("{:<11} {}\r", "Song:".truecolor(146, 49, 176), item.track);
        println!("{:<11} {}\r", "Artist:".truecolor(126, 87, 194), item.artist_name);
        println!("{:<11} {}\r", "Album:".truecolor(121, 134, 203), item.album_title);

        let progress_bar_len = if total_seconds > 0 {
            total_seconds as u64
        } else {
            u64::MAX
        };
        let b = self.clone();
        let progress_bar_style = ProgressStyle::with_template(
            "  {wide_bar} {progress_info} {spinner:.dim.bold} ",
        ).unwrap()
            .tick_chars("⠁⠂⠄⡀⠄⠂ ")
            .with_key(
                "progress_info",
                move |state: &ProgressState, write: &mut dyn Write| {
                    let progress_info = b.get_progress_bar_progress_info(state.pos(), state.len());
                    write!(write, "{progress_info}").unwrap();
                },
            );

        let prog_bar = ProgressBar::new(progress_bar_len)
            .with_style(progress_bar_style)
            .with_position(0);

        // self.mutex_progress_bar.lock().unwrap().replace(prog_bar);
        PROGRESS_BAR.lock().unwrap().replace(prog_bar);
        Ok(())
    }

    fn get_progress_bar_progress_info(
        &self,
        elapsed_seconds: u64,
        total_seconds: Option<u64>,
    ) -> String {
        let humanized_elapsed_duration =
            format_duration!(elapsed_seconds);

        if let Some(total_seconds) = total_seconds {
            if total_seconds != u64::MAX {
                let humanized_total_duration =
                    format_duration!(total_seconds);
                return format!("{humanized_elapsed_duration} / {humanized_total_duration}");
            }
        }
        humanized_elapsed_duration
    }

    fn get_progress_bar_current_position(&self) -> Duration {
        PROGRESS_BAR.lock().unwrap().to_owned().unwrap().eta()
    }

    fn enable_tick(&self) {
        *PROG.lock().unwrap() = true;
    }

    fn disable_tick(&self) {
        *PROG.lock().unwrap() = false;
    }

    fn enable_tick_on_screen(&self) {
        PROGRESS_BAR
            .lock().unwrap()
            .to_owned().unwrap()
            .set_draw_target(ProgressDrawTarget::stdout());

        execute!(stdout(),
            cursor::MoveUp(1)
        ).unwrap();
    }

    fn disable_tick_on_screen(&self) {
        PROGRESS_BAR
            .lock().unwrap()
            .to_owned().unwrap()
            .set_draw_target(ProgressDrawTarget::hidden());
    }

    fn destroy(&self) {
        match PROGRESS_BAR.lock() {
            Ok(a)
                => if let Some(a) = a.to_owned() { a.finish_and_clear() },
            Err(e)
                => println!("Error: {}", e),
        }
    }


    /// Increase elapsed seconds in progress bar by 1 every second.
    async fn tick_progress_bar_progress(mut self) {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            if *PROG.lock().unwrap() {
                self.update_progress_bar(|p| p.inc(1));
            }
        }
    }

    async fn run(mut self) -> JoinHandle<()> {
        let s = self.clone();
        tokio::spawn(s.tick_progress_bar_progress())
    }

    fn enable_spinner(&self) {
        if *PROG.lock().unwrap() {
            match PROGRESS_BAR.lock() {
                Ok(a)
                    => if let Some(a) = a.to_owned() {
                        a.enable_steady_tick(Duration::from_millis(100))
                },
                Err(e)
                    => println!("Error: {}", e),
            }
        }
    }

    fn disable_spinner(&self) {
        if *PROG.lock().unwrap() {
            match PROGRESS_BAR.lock() {
                Ok(a)
                    => if let Some(a) = a.to_owned() {
                        a.disable_steady_tick()
                },
                Err(e)
                    => println!("Error: {}", e),
            }
        }
    }
}

trait UpdateProgressBar {
    fn update_progress_bar<T>(&self, action: T)
        where
            T: FnOnce(&ProgressBar);
}

impl UpdateProgressBar for Bar<'_> {
    fn update_progress_bar<T>(&self, action: T)
        where
            T: FnOnce(&ProgressBar),
    {
        if let Some(progress_bar) = PROGRESS_BAR.lock().unwrap().as_ref() {
            action(progress_bar);
        }
    }
}
