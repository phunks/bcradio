
use std::ops::Deref;
use anyhow::{Error, Result};
use clap::Parser;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::event::KeyEventKind;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use libbc::player::{PARK, RXTX};

use crate::models::bc_error::BcradioError;
use crate::libbc::args::Args;
use crate::libbc::player;
use crate::libbc::shared_data::SharedState;
use crate::libbc::terminal;

mod libbc;
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
    Args::parse();

    terminal::init();
    println!("{}", LOGO);

    if let Err(e) = start_playing().await {
        disable_raw_mode()?;
        terminal::print_error(e);
    }
    Ok(())
}

async fn start_playing() -> Result<()> {
    let hdl = tokio::spawn(<SharedState as player::Player>::player_thread());
    loop {
        if *PARK.lock().unwrap() {

            enable_raw_mode()?;
            match event::read()? {
                Event::Key(KeyEvent {
                               code: KeyCode::Char('c'),
                               modifiers: KeyModifiers::CONTROL, ..
                           }) => {
                    terminal::quit(Error::from(BcradioError::OperationInterrupted));
                },
                Event::Key(e) => {
                    if e.kind == KeyEventKind::Press {
                        if let KeyCode::Char(c) = e.code {
                            match c {
                                's' | 'h' | 'm' | 'i' => {
                                    *PARK.lock().unwrap() = false;
                                    RXTX.deref().0.send(c).await?
                                },
                                'a'..='z' | '0'..='9' => RXTX.deref().0.send(c).await?,
                                'Q' => {
                                    RXTX.deref().0.send(c).await?;
                                    break;
                                },
                                _ => {},
                            }
                        }
                    }
                },
                _ => {},
            }
        }
    }
    hdl.await?.expect("player thread");
    disable_raw_mode()?;
    Ok(())
}

