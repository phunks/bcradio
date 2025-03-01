use anyhow::{Error, Result};
use crossterm::event::KeyEventKind;
use crossterm::event::{self, poll, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use libbc::player::{PARK, RXTX};
use std::ops::Deref;
use std::time::Duration;

use crate::libbc::args::init_args;
use crate::libbc::player;
use crate::libbc::player::park_lock;
use crate::libbc::shared_data::SharedState;
use crate::libbc::terminal;
use crate::models::bc_error::BcradioError;

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
    let _exit = terminal::Quit;
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
                        modifiers: KeyModifiers::CONTROL,
                        ..
                    }) => {
                        terminal::quit(Error::from(BcradioError::OperationInterrupted));
                    }
                    Event::Key(e) => {
                        if e.kind == KeyEventKind::Press {
                            if let KeyCode::Char(c) = e.code {
                                match c {
                                    's' | 'h' | 'm' | 'i' | 'l' => {
                                        park_lock();
                                        RXTX.deref().0.send(c).await?
                                    }
                                    'a'..='z' | '0'..='9' => RXTX.deref().0.send(c).await?,
                                    'Q' => {
                                        RXTX.deref().0.send(c).await?;
                                        break;
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    _ => {}
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
