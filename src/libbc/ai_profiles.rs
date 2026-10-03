use std::{collections::HashSet, fs, path::Path};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::libbc::{
    ai::{self, AiConfig},
    ai_key,
    args::AiConfigCommand,
};

pub const DEFAULT_PROFILE: &str = "default";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub name: String,
    pub config: AiConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Profiles {
    pub active: Option<String>,
    pub profiles: Vec<Profile>,
}

pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
    {
        bail!("profile name must be 1–64 ASCII letters, digits, '.', '_' or '-'");
    }
    Ok(())
}

impl Profiles {
    /// Only the profile name is shown; never reads the credential store.
    pub fn input_title(&self) -> String {
        let name = self.active.as_deref().unwrap_or("not configured");
        format!("? describe an AI playlist to {name} (Enter: generate, Esc: cancel)")
    }

    fn validate(&self) -> Result<()> {
        let mut names = HashSet::new();
        for profile in &self.profiles {
            validate_name(&profile.name)?;
            profile.config.validate()?;
            if !names.insert(&profile.name) {
                bail!("duplicate AI profile name");
            }
        }
        match &self.active {
            Some(name) if names.contains(name) => {}
            None if self.profiles.is_empty() => {}
            _ => bail!("active AI profile is missing or invalid"),
        }
        Ok(())
    }

    pub fn target_name<'a>(&'a self, name: Option<&'a str>) -> Result<&'a str> {
        let name = name.or(self.active.as_deref()).unwrap_or(DEFAULT_PROFILE);
        validate_name(name)?;
        Ok(name)
    }

    pub fn selected(&self, name: Option<&str>) -> Result<&Profile> {
        let name = self.target_name(name)?;
        self.profiles
            .iter()
            .find(|p| p.name == name)
            .with_context(|| {
                format!("AI profile '{name}' is not configured; use O or `bcradio ai-config set`")
            })
    }

    pub fn set(&mut self, name: &str, config: AiConfig) -> Result<()> {
        validate_name(name)?;
        config.validate()?;
        if let Some(profile) = self.profiles.iter_mut().find(|p| p.name == name) {
            profile.config = config;
        } else {
            self.profiles.push(Profile {
                name: name.into(),
                config,
            });
        }
        if self.active.is_none() {
            self.active = Some(name.into());
        }
        Ok(())
    }

    pub fn activate(&mut self, name: &str) -> Result<()> {
        self.selected(Some(name))?;
        self.active = Some(name.into());
        Ok(())
    }

    pub fn remove(&mut self, name: &str) -> Result<()> {
        self.selected(Some(name))?;
        self.profiles.retain(|p| p.name != name);
        if self.active.as_deref() == Some(name) {
            self.active = self.profiles.first().map(|p| p.name.clone());
        }
        Ok(())
    }

    pub fn save(&self) -> Result<()> {
        self.validate()?;
        ai::write_at(&ai::config_path()?, self)
    }
}

fn read_at(path: &Path) -> Result<Profiles> {
    let data = match fs::read(path) {
        Ok(data) => data,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Profiles::default()),
        Err(e) => return Err(e).context("cannot read AI profiles"),
    };
    // A legacy URL/model pair becomes the default profile in memory. Reading does
    // not create or rewrite files, and its existing keychain account stays valid.
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Stored {
        Profiles(Profiles),
        Legacy(AiConfig),
    }
    let profiles = match serde_json::from_slice(&data).context("invalid AI configuration")? {
        Stored::Profiles(profiles) => profiles,
        Stored::Legacy(config) => Profiles {
            active: Some(DEFAULT_PROFILE.into()),
            profiles: vec![Profile {
                name: DEFAULT_PROFILE.into(),
                config,
            }],
        },
    };
    profiles.validate()?;
    Ok(profiles)
}

pub fn load() -> Result<Profiles> {
    read_at(&ai::config_path()?)
}

pub fn delete(profiles: &mut Profiles, name: &str) -> Result<()> {
    profiles.selected(Some(name))?;
    // Refuse to remove the profile if its secure credential cannot be deleted.
    ai_key::delete_for(name)?;
    profiles.remove(name)?;
    profiles.save()
}

pub fn run(action: &AiConfigCommand) -> Result<()> {
    let mut profiles = load()?;
    match action {
        AiConfigCommand::Set {
            profile,
            url,
            model,
        } => {
            let name = profiles.target_name(profile.as_deref())?.to_owned();
            profiles.set(
                &name,
                AiConfig {
                    url: url.clone(),
                    model: model.clone(),
                },
            )?;
            profiles.save()?;
            println!("AI profile '{name}' saved (API key remains in the OS credential store).");
        }
        AiConfigCommand::Show { profile } => {
            if profiles.profiles.is_empty() && profile.is_none() {
                println!("AI API is not configured.");
            } else {
                let p = profiles.selected(profile.as_deref())?;
                println!(
                    "Profile: {}\nURL: {}\nModel: {}",
                    p.name, p.config.url, p.config.model
                );
            }
        }
        AiConfigCommand::List => {
            if profiles.profiles.is_empty() {
                println!("No AI profiles configured.");
            }
            for p in &profiles.profiles {
                println!(
                    "{} {}  {}  {}",
                    if profiles.active.as_deref() == Some(&p.name) {
                        "*"
                    } else {
                        " "
                    },
                    p.name,
                    p.config.url,
                    p.config.model
                );
            }
        }
        AiConfigCommand::Use { profile } => {
            profiles.activate(profile)?;
            profiles.save()?;
            println!("Active AI profile: {profile}");
        }
        AiConfigCommand::Delete { profile } => {
            delete(&mut profiles, profile)?;
            println!("AI profile '{profile}' and its credential deleted.");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(model: &str) -> AiConfig {
        AiConfig {
            url: "https://example.com/v1".into(),
            model: model.into(),
        }
    }

    #[test]
    fn input_title_tracks_active_profile_and_handles_missing_configuration() {
        let mut p = Profiles::default();
        assert!(p.input_title().contains("to not configured"));
        p.set("first", config("model-A")).unwrap();
        p.set("second", config("model-B")).unwrap();
        assert_eq!(
            p.input_title(),
            "? describe an AI playlist to first (Enter: generate, Esc: cancel)"
        );
        p.activate("second").unwrap();
        assert_eq!(
            p.input_title(),
            "? describe an AI playlist to second (Enter: generate, Esc: cancel)"
        );
        assert!(!p.input_title().contains("model-B"));
        assert!(!p.input_title().contains("https://"));
        p.set("second", config("model-C")).unwrap();
        assert!(p.input_title().contains("to second"));
    }

    #[test]
    fn profiles_switch_update_and_remove() {
        let mut p = Profiles::default();
        p.set("openai", config("one")).unwrap();
        p.set("local", config("two")).unwrap();
        assert_eq!(p.selected(None).unwrap().config.model, "one");
        p.activate("local").unwrap();
        assert_eq!(p.selected(None).unwrap().config.model, "two");
        p.set("local", config("three")).unwrap();
        assert_eq!(p.profiles.len(), 2);
        assert_eq!(p.selected(None).unwrap().config.model, "three");
        assert!(p.activate("missing").is_err());
        p.remove("local").unwrap();
        assert_eq!(p.active.as_deref(), Some("openai"));
        p.remove("openai").unwrap();
        assert_eq!(p, Profiles::default());
        p.validate().unwrap();
    }

    #[test]
    fn rejects_invalid_names_configs_and_stored_profiles() {
        for name in ["", "../name", "a b", "日本語", "a\n"] {
            assert!(validate_name(name).is_err());
        }
        assert!(validate_name(&"a".repeat(65)).is_err());
        let mut p = Profiles::default();
        assert!(p.set("good", config(" ")).is_err());
        assert!(p.profiles.is_empty());
        p.set("good", config("one")).unwrap();
        p.profiles.push(p.profiles[0].clone());
        assert!(p.validate().is_err());
        p.profiles.pop();
        p.active = Some("missing".into());
        assert!(p.validate().is_err());
    }

    #[test]
    fn legacy_migration_and_round_trip_without_secrets_or_read_side_effects() {
        let dir = std::env::temp_dir().join(format!("bcradio-profiles-{}", std::process::id()));
        let path = dir.join("ai.json");
        assert_eq!(read_at(&path).unwrap(), Profiles::default());
        assert!(!dir.exists());
        ai::write_at(&path, &config("legacy")).unwrap();
        let before = fs::read(&path).unwrap();
        let mut p = read_at(&path).unwrap();
        assert_eq!(p.selected(None).unwrap().name, DEFAULT_PROFILE);
        assert_eq!(fs::read(&path).unwrap(), before);
        p.set("second", config("new")).unwrap();
        p.activate("second").unwrap();
        ai::write_at(&path, &p).unwrap();
        assert_eq!(read_at(&path).unwrap(), p);
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("api_key"));
        fs::write(&path, r#"{"active":null,"profiles":[],"api_key":"secret"}"#).unwrap();
        assert!(read_at(&path).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
}
