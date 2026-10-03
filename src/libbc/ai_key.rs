use anyhow::{bail, Context, Result};
use inquire::{Password, PasswordDisplayMode};
use keyring::{Entry, Error};

use crate::libbc::ai_profiles::{self, DEFAULT_PROFILE};
use crate::libbc::args::AiKeyCommand;

const SERVICE: &str = "bcradio";
const ACCOUNT: &str = "ai-api-key";

fn account(profile: &str) -> Result<String> {
    ai_profiles::validate_name(profile)?;
    Ok(if profile == DEFAULT_PROFILE {
        ACCOUNT.into()
    } else {
        format!("{ACCOUNT}:profile:{profile}")
    })
}

fn entry(profile: &str) -> Result<Entry> {
    Entry::new(SERVICE, &account(profile)?).context("OS credential store is unavailable")
}

/// Only retrieve the secret when making an AI request. Never log the result.
#[allow(dead_code)]
pub fn load_for(profile: &str) -> Result<Option<String>> {
    match entry(profile)?.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(Error::NoEntry) => Ok(None),
        Err(e) => Err(e).context("could not read AI API key from OS credential store"),
    }
}

pub fn set_for(profile: &str, key: &str) -> Result<()> {
    if key.trim().is_empty() {
        bail!("AI API key cannot be empty");
    }
    entry(profile)?
        .set_password(key)
        .context("could not save AI API key to OS credential store")
}

pub fn delete_for(profile: &str) -> Result<bool> {
    match entry(profile)?.delete_credential() {
        Ok(()) => Ok(true),
        Err(Error::NoEntry) => Ok(false),
        Err(e) => Err(e).context("could not delete AI API key"),
    }
}

pub fn run(command: AiKeyCommand, profile: Option<&str>) -> Result<()> {
    let profiles = ai_profiles::load()?;
    let profile = profiles.target_name(profile)?;
    // Allow the default key to be set before configuring the first endpoint,
    // as in the original CLI. Named keys require a saved profile.
    if profile != DEFAULT_PROFILE {
        profiles.selected(Some(profile))?;
    }
    match command {
        AiKeyCommand::Set => {
            // Prompt rather than accepting a command-line argument: argv and shell history
            // can expose secrets. The key is never written to an application file.
            let key = Password::new("AI API key")
                .without_confirmation()
                .with_display_mode(PasswordDisplayMode::Hidden)
                .prompt()
                .context("could not read AI API key")?;
            set_for(profile, &key)?;
            println!("AI API key for '{profile}' saved in the OS credential store.");
        }
        AiKeyCommand::Status => {
            println!(
                "AI API key for '{profile}': {}",
                if load_for(profile)?.is_some() {
                    "set"
                } else {
                    "not set"
                }
            );
        }
        AiKeyCommand::Delete => {
            println!(
                "AI API key for '{profile}': {}",
                if delete_for(profile)? {
                    "deleted"
                } else {
                    "not set"
                }
            );
        }
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
        assert_eq!(account(DEFAULT_PROFILE).unwrap(), ACCOUNT);
        assert_eq!(account("openai").unwrap(), "ai-api-key:profile:openai");
        assert_ne!(account("openai").unwrap(), account("local").unwrap());
        assert!(account("bad/name").is_err());
    }
}
