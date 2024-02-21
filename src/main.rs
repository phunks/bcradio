
use std::ops::Deref;
use std::time::Duration;
use anyhow::{Error, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers, poll};
use crossterm::event::KeyEventKind;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use libbc::player::{PARK, RXTX};

use crate::models::bc_error::BcradioError;
use crate::libbc::args::init_args;
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
    init_args();
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
            if poll(Duration::from_millis(200))? {
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
        } else {
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
    hdl.await?.expect("player thread");
    disable_raw_mode()?;
    Ok(())
}

