
mod models;
mod libbc;

use clap::Parser;
use std::ops::Deref;
use anyhow::Result;
use libbc::player::{PARK, RXTX};
use crate::libbc::args::Args;
use crate::libbc::terminal;
use crate::libbc::player;
use crate::libbc::shared_data::SharedState;

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
        println!("{:?}", e);
        terminal::print_error(e);
    }
    Ok(())
}

async fn start_playing() -> Result<()> {
    let hdl = tokio::spawn(<SharedState as player::Player>::player_thread());
    loop {
        if *PARK.lock().unwrap() {
            if let Ok(event) = terminal::read_char() {
                match event {
                    's' | 'h' => {
                        *PARK.lock().unwrap() = false;
                        RXTX.deref().0.send(event).await?
                    }
                    'a'..='z' | '0'..='9' => {
                        RXTX.deref().0.send(event).await?
                    }
                    'Q' => {
                        RXTX.deref().0.send(event).await?;
                        break
                    }
                    _ => {}
                };
            }
        }
    }
    hdl.await?.expect("player thread");
    Ok(())
}