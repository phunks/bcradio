use anyhow::{Error, Result};
use async_channel::unbounded;
use crossterm::event::{self, poll};
use crossterm::terminal::disable_raw_mode;
use std::time::Duration;

use crate::libbc::args::{args_command, args_verbose_log, init_args, ConfigCommand};
use crate::libbc::command::{from_event, Command};
use crate::libbc::player;
use crate::libbc::shared_data::SharedState;
use crate::libbc::terminal;
use crate::logger::Logger;
use crate::models::bc_error::BcradioError;

mod libbc;
mod logger;
mod models;

const LOGO: &str = r#"
▄▄▄▄·  ▄▄· ▄▄▄   ▄▄▄· ·▄▄▄▄  ▪
▐█ ▀█▪▐█ ▌▪▀▄ █·▐█ ▀█ ██▪ ██ ██ ▪
▐█▀▀█▄██ ▄▄▐▀▀▄ ▄█▀▀█ ▐█· ▐█▌▐█· ▄█▀▄
██▄▪▐█▐███▌▐█•█▌▐█ ▪▐▌██. ██ ▐█▌▐█▌.▐▌
·▀▀▀▀ ·▀▀▀ .▀  ▀ ▀  ▀ ▀▀▀▀▀• ▀▀▀ ▀█▄▀▪
"#;

#[tokio::main]
async fn main() -> Result<()> {
    init_args();
    match args_command() {
        Some(ConfigCommand::AiKey { action }) => return libbc::ai_key::run(*action),
        Some(ConfigCommand::AiConfig { action }) => return libbc::ai::run_config(action),
        None => {}
    }
    let _exit = terminal::Quit;
    let _logger = Logger::build(args_verbose_log());
    terminal::init();

    println!("{}", LOGO);

    if let Err(e) = start_playing().await {
        disable_raw_mode()?;
        terminal::print_error(format!("{e:#}"));
    }
    Ok(())
}

async fn start_playing() -> Result<()> {
    let state = SharedState::default();
    let gate = state.input_gate.clone();
    let (sender, receiver) = unbounded();
    let mut hdl = tokio::spawn(<SharedState as player::Player>::player_thread(
        state, receiver,
    ));
    // Only this loop reads the terminal while the player screen owns it.
    // The modal screens (including inquire) use their own blocking readers.
    let input_result: Result<()> = async {
        loop {
            if hdl.is_finished() {
                return (&mut hdl).await?;
            }
            if !gate.is_playback_active() {
                tokio::time::sleep(Duration::from_millis(250)).await;
                continue;
            }
            terminal::ensure_raw_mode()?;
            if !poll(Duration::from_millis(250))? {
                continue;
            }
            if let Some(command) = from_event(event::read()?) {
                if command == Command::Interrupt {
                    return Err(Error::from(BcradioError::OperationInterrupted));
                }
                if command.opens_screen() {
                    gate.hand_off_to_screen();
                }
                sender.send(command).await?;
            }
        }
    }
    .await;
    if input_result.is_err() {
        hdl.abort();
    }
    input_result
}
