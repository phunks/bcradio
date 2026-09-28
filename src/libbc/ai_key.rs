use anyhow::{bail, Context, Result};
use inquire::{Password, PasswordDisplayMode};
use keyring::{Entry, Error};

use crate::libbc::args::AiKeyCommand;

const SERVICE: &str = "bcradio";
const ACCOUNT: &str = "ai-api-key";

fn entry() -> Result<Entry> {
    Entry::new(SERVICE, ACCOUNT).context("OS credential store is unavailable")
}

/// Only retrieve the secret when making an AI request. Never log the result.
#[allow(dead_code)]
pub fn load() -> Result<Option<String>> {
    match entry()?.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(Error::NoEntry) => Ok(None),
        Err(e) => Err(e).context("could not read AI API key from OS credential store"),
    }
}

pub fn run(command: AiKeyCommand) -> Result<()> {
    match command {
        AiKeyCommand::Set => {
            // Prompt rather than accepting a command-line argument: argv and shell history
            // can expose secrets. The key is never written to an application file.
            let key = Password::new("AI API key")
                .without_confirmation()
                .with_display_mode(PasswordDisplayMode::Hidden)
                .prompt()
                .context("could not read AI API key")?;
            if key.trim().is_empty() {
                bail!("AI API key cannot be empty");
            }
            entry()?
                .set_password(&key)
                .context("could not save AI API key to OS credential store")?;
            println!("AI API key saved in the OS credential store.");
        }
        AiKeyCommand::Status => {
            println!(
                "AI API key: {}",
                if load()?.is_some() { "set" } else { "not set" }
            );
        }
        AiKeyCommand::Delete => match entry()?.delete_credential() {
            Ok(()) => println!("AI API key deleted."),
            Err(Error::NoEntry) => println!("AI API key is not set."),
            Err(e) => return Err(e).context("could not delete AI API key"),
        },
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_identifiers_are_stable() {
        assert_eq!(SERVICE, "bcradio");
        assert_eq!(ACCOUNT, "ai-api-key");
    }
}
