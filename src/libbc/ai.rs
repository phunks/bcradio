use std::collections::{HashSet, VecDeque};
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::libbc::ai_key;
use crate::libbc::args::AiConfigCommand;
use crate::libbc::search::find_tracks;
use crate::models::shared_data_models::Track;

const PLAYLIST_PROMPT: &str = "Suggest Bandcamp music matching the user's description. Return ONLY a JSON array of 6 to 8 distinct specific Bandcamp search terms in listening order, preferably artist and track names. Use different artists where possible. Avoid the previously used search terms and recently played songs supplied with the description. For non-English requests, use searchable artist names or genre terms in English where appropriate. Do not invent stream URLs. No markdown or explanatory text.";

const MAX_TRACKS_PER_TERM: usize = 3;
const MAX_PLAYLIST_TRACKS: usize = 15;
const MAX_SEARCH_FALLBACKS: usize = 4;
// Slow LiteLLM/backends can take over a minute before sending any response.
const AI_REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

fn parse_terms(response: &str) -> Result<Vec<String>> {
    let terms: Vec<String> = serde_json::from_str(response.trim())
        .context("AI playlist response must be a JSON array of search terms")?;
    if terms.is_empty()
        || terms.len() > 10
        || terms.iter().any(|s| s.trim().is_empty() || s.len() > 150)
    {
        bail!("AI playlist response contains invalid search terms");
    }
    Ok(terms)
}

/// Resolve AI suggestions against playable Bandcamp tracks before touching the queue.
pub async fn generate_playlist(
    description: &str,
    previous_terms: &[String],
    recent_songs: &[String],
) -> Result<(VecDeque<Track>, Vec<String>)> {
    let request = format!(
        "Description: {description}\nPreviously used search terms: {}\nRecently played songs: {}",
        previous_terms.join("; "),
        recent_songs.join("; ")
    );
    let response = complete(PLAYLIST_PROMPT, &request).await?;
    let terms = parse_terms(&response)?;
    let mut tracks = VecDeque::new();
    let mut seen = HashSet::new();
    let mut first_error = None;
    let mut failed_terms = 0;
    let mut empty_terms = 0;
    for term in &terms {
        // A failed or unavailable search term should not discard matches already found.
        match search_with_fallback(term, |query| async move { find_tracks(&query).await }).await {
            Ok(results) => {
                if results.is_empty() {
                    empty_terms += 1;
                    tracing::debug!(term, "Bandcamp search found no playable tracks");
                }
                add_matches(&mut tracks, &mut seen, shuffled_unique_matches(results));
            }
            Err(error) => {
                failed_terms += 1;
                tracing::warn!(term, error = %format!("{error:#}"), "Bandcamp search failed");
                if first_error.is_none() {
                    first_error = Some(error);
                }
                continue;
            }
        }
        if tracks.len() >= MAX_PLAYLIST_TRACKS {
            break;
        }
    }
    if tracks.is_empty() {
        if let Some(error) = first_error {
            return Err(error).context(format!(
                "No playable Bandcamp tracks found for the AI playlist ({failed_terms} search errors, {empty_terms} terms without playable tracks)"
            ));
        }
        bail!("No playable Bandcamp tracks found for the AI playlist");
    }
    Ok((tracks, terms))
}

// Select from the full playable pool without favoring search rank or album track order.
// Deduplicate before shuffling so reissues cannot occupy multiple lottery slots.
fn shuffled_unique_matches(results: Vec<Track>) -> Vec<Track> {
    let mut unique = HashSet::new();
    let mut results: Vec<_> = results
        .into_iter()
        .filter(|track| !track.url.is_empty() && unique.insert(ai_track_key(track)))
        .collect();
    results.shuffle(&mut rand::thread_rng());
    results
}

/// Try the full AI suggestion first; broaden only when it yields no playable songs.
/// Keep at least two words so a single common first name cannot dominate results.
async fn search_with_fallback<F, Fut>(term: &str, mut search: F) -> Result<Vec<Track>>
where
    F: FnMut(String) -> Fut,
    Fut: Future<Output = Result<Vec<Track>>>,
{
    let words: Vec<&str> = term.split_whitespace().collect();
    let mut query = term.to_owned();
    for fallback in 0..=MAX_SEARCH_FALLBACKS {
        let results = search(query.clone()).await?;
        if !results.is_empty() {
            return Ok(results);
        }
        if fallback == MAX_SEARCH_FALLBACKS || words.len() <= fallback + 2 {
            break;
        }
        query = words[..words.len() - fallback - 1].join(" ");
        tracing::debug!(original_term = term, fallback_term = %query, "Bandcamp search retrying with a broader term");
    }
    Ok(Vec::new())
}

fn add_matches(
    tracks: &mut VecDeque<Track>,
    seen: &mut HashSet<(String, String)>,
    results: Vec<Track>,
) {
    let mut added = 0;
    for track in results {
        if tracks.len() >= MAX_PLAYLIST_TRACKS || added >= MAX_TRACKS_PER_TERM {
            break;
        }
        if track.url.is_empty() || !seen.insert(ai_track_key(&track)) {
            continue;
        }
        tracks.push_back(track);
        added += 1;
    }
}

// Bandcamp can list the same artist's song under multiple releases/band IDs.
// Do not collapse different artists' recordings of a song (or untitled tracks).
pub(crate) fn ai_track_key(track: &Track) -> (String, String) {
    let title = normalize_track_text(&track.track);
    if title.is_empty() {
        return (format!("stream:{}", track.url), title);
    }
    let artist = normalize_track_text(&track.artist_name);
    if artist.is_empty() {
        (format!("band:{}", track.band_id), title)
    } else {
        (format!("artist:{artist}"), title)
    }
}

fn normalize_track_text(text: &str) -> String {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AiConfig {
    /// OpenAI-compatible API base URL, including /v1 when required.
    pub url: String,
    pub model: String,
}

impl AiConfig {
    fn validate(&self) -> Result<Url> {
        let url = Url::parse(&self.url).context("invalid AI API base URL")?;
        if url.host_str().is_none()
            || url.username() != ""
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || !matches!(url.scheme(), "http" | "https")
        {
            bail!("AI API URL must be an HTTP(S) base URL without credentials, query or fragment");
        }
        if self.model.trim().is_empty() {
            bail!("AI model cannot be empty");
        }
        Ok(url)
    }

    fn chat_url(&self) -> Result<Url> {
        let mut url = self.validate()?;
        let path = url.path().trim_end_matches('/');
        url.set_path(&format!("{path}/chat/completions"));
        Ok(url)
    }
}

fn config_path() -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    let base = PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?)
        .join("Library/Application Support");
    #[cfg(target_os = "linux")]
    let base = match std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        Some(path) if Path::new(&path).is_absolute() => PathBuf::from(path),
        _ => PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?).join(".config"),
    };
    #[cfg(target_os = "windows")]
    let base = PathBuf::from(std::env::var_os("APPDATA").context("APPDATA is not set")?);
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    bail!("unsupported platform for AI configuration");
    Ok(base.join("bcradio").join("ai.json"))
}

fn read_at(path: &Path) -> Result<Option<AiConfig>> {
    let contents = match fs::read(path) {
        Ok(contents) => contents,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
    };
    let config: AiConfig = serde_json::from_slice(&contents).context("invalid AI configuration")?;
    config.validate()?;
    Ok(Some(config))
}

pub fn load_config() -> Result<Option<AiConfig>> {
    read_at(&config_path()?)
}

fn write_at(path: &Path, config: &AiConfig) -> Result<()> {
    config.validate()?;
    let parent = path.parent().context("invalid AI config path")?;
    fs::create_dir_all(parent)?;
    let contents = serde_json::to_vec_pretty(config)?;
    // Windows does not support replacing an existing file with rename. The
    // config contains only public values; overwrite it directly on Windows.
    #[cfg(windows)]
    return fs::write(path, contents).context("cannot save AI configuration");

    #[cfg(not(windows))]
    {
        // Write atomically so an interrupted save does not leave truncated JSON.
        let temp = path.with_extension(format!("{}.tmp", std::process::id()));
        let result = (|| -> Result<()> {
            let file = {
                use std::os::unix::fs::OpenOptionsExt;
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&temp)?
            };
            use std::io::Write;
            let mut file = file;
            file.write_all(&contents)?;
            file.sync_all()?;
            fs::rename(&temp, path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }
}

pub fn run_config(action: &AiConfigCommand) -> Result<()> {
    match action {
        AiConfigCommand::Set { url, model } => {
            let config = AiConfig {
                url: url.clone(),
                model: model.clone(),
            };
            write_at(&config_path()?, &config)?;
            println!("AI API configuration saved (API key remains in the OS credential store).");
        }
        AiConfigCommand::Show => match load_config()? {
            Some(config) => println!("URL: {}\nModel: {}", config.url, config.model),
            None => println!("AI API is not configured."),
        },
    }
    Ok(())
}

#[derive(Debug, Serialize)]
struct Message<'a> {
    role: &'static str,
    content: &'a str,
}

#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: [Message<'a>; 2],
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    content: Option<String>,
}

/// Provider-independent chat-completions interface. Do not log the key,
/// prompts, response body or authorization header.
#[allow(dead_code)]
pub async fn complete(system: &str, user: &str) -> Result<String> {
    let config = load_config()?.context("configure AI with `bcradio ai-config set` first")?;
    let key = ai_key::load()?.context("set AI API key with `bcradio ai-key set` first")?;
    complete_with(&config, &key, system, user).await
}

async fn complete_with(config: &AiConfig, key: &str, system: &str, user: &str) -> Result<String> {
    let url = config.chat_url()?;
    let client = reqwest::Client::builder()
        .timeout(AI_REQUEST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let body = serde_json::to_vec(&ChatRequest {
        model: &config.model,
        messages: [
            Message {
                role: "system",
                content: system,
            },
            Message {
                role: "user",
                content: user,
            },
        ],
    })?;
    let started = Instant::now();
    let response = client
        .post(url)
        .bearer_auth(key)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body)
        .send()
        .await
        .map_err(|e| ai_transport_error("request", started, e))?;
    let status = response.status();
    tracing::debug!(elapsed_secs = started.elapsed().as_secs_f64(), %status, "AI API response headers received");
    if !status.is_success() {
        bail!("AI API returned HTTP {status}");
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|e| ai_transport_error("response body", started, e))?;
    tracing::debug!(
        elapsed_secs = started.elapsed().as_secs_f64(),
        "AI API response body received"
    );
    let result: ChatResponse = serde_json::from_slice(&bytes).context("invalid AI API response")?;
    result
        .choices
        .into_iter()
        .next()
        .and_then(|choice| choice.message.content)
        .filter(|content| !content.trim().is_empty())
        .context("AI API returned no text")
}

fn ai_transport_error(stage: &str, started: Instant, error: reqwest::Error) -> anyhow::Error {
    let timed_out = error.is_timeout();
    tracing::warn!(
        stage,
        elapsed_secs = started.elapsed().as_secs_f64(),
        timed_out,
        "AI API transport failed"
    );
    anyhow::Error::new(error).context(format!(
        "AI API {stage} failed after {:.1}s (timed out: {timed_out}; request limit: {}s)",
        started.elapsed().as_secs_f64(),
        AI_REQUEST_TIMEOUT.as_secs()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn broadens_empty_artist_track_search_to_artist() {
        let queries = Arc::new(Mutex::new(Vec::new()));
        let seen = queries.clone();
        let tracks = search_with_fallback("Gerry Mulligan Night Lights", move |query| {
            seen.lock().unwrap().push(query.clone());
            async move {
                if query == "Gerry Mulligan" {
                    Ok(vec![Track {
                        track: "found".into(),
                        ..Default::default()
                    }])
                } else {
                    Ok(vec![])
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(tracks[0].track, "found");
        assert_eq!(
            *queries.lock().unwrap(),
            [
                "Gerry Mulligan Night Lights",
                "Gerry Mulligan Night",
                "Gerry Mulligan"
            ]
        );
    }

    #[tokio::test]
    async fn paul_desmond_fallback_never_searches_paul_alone() {
        let mut queries = Vec::new();
        search_with_fallback("Paul Desmond Take Ten", |query| {
            queries.push(query);
            async { Ok(vec![]) }
        })
        .await
        .unwrap();
        assert_eq!(
            queries,
            ["Paul Desmond Take Ten", "Paul Desmond Take", "Paul Desmond"]
        );
    }

    #[test]
    fn ai_deduplicates_same_song_across_band_ids_but_keeps_other_artists() {
        let make = |artist: &str, name: &str, band_id: i64| Track {
            artist_name: artist.into(),
            track: name.into(),
            band_id,
            url: format!("https://example.com/{band_id}/{artist}"),
            ..Default::default()
        };
        let mut queue = VecDeque::new();
        let mut seen = HashSet::new();
        add_matches(
            &mut queue,
            &mut seen,
            vec![
                make("Alabaster DePlume", "Visit Croatia", 1),
                make("alabaster deplume", "VISIT CROATIA!", 2),
                make("Another Artist", "Visit Croatia", 3),
                make("Alabaster DePlume", "Visit Japan", 4),
            ],
        );
        assert_eq!(queue.len(), 3);
        assert_eq!(queue[0].track, "Visit Croatia");
        assert_eq!(queue[1].artist_name, "Another Artist");
        assert_eq!(queue[2].track, "Visit Japan");
        // The same song suggested by another term is still a duplicate.
        add_matches(
            &mut queue,
            &mut seen,
            vec![make("Alabaster DePlume", "Visit Croatia", 5)],
        );
        assert_eq!(queue.len(), 3);
    }

    #[test]
    fn shuffle_pool_keeps_late_matches_and_removes_duplicates_before_selection() {
        let make = |index: usize| Track {
            artist_name: "Artist".into(),
            track: format!("song {index}"),
            url: format!("https://example.com/{index}"),
            ..Default::default()
        };
        let mut results = (0..8).map(make).collect::<Vec<_>>();
        results.push(Track {
            band_id: 123,
            url: "https://example.com/reissue".into(),
            ..make(0)
        });
        results.push(Track {
            url: String::new(),
            ..make(99)
        });
        let pool = shuffled_unique_matches(results);
        assert_eq!(pool.len(), 8);
        let mut names: Vec<_> = pool.iter().map(|track| track.track.clone()).collect();
        names.sort();
        assert_eq!(
            names,
            (0..8).map(|i| format!("song {i}")).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn fallback_does_not_run_on_matches_or_errors() {
        let mut calls = 0;
        let found = search_with_fallback("Stan Getz Moonlight in Vermont", |_| {
            calls += 1;
            async {
                Ok(vec![Track {
                    track: "match".into(),
                    ..Default::default()
                }])
            }
        })
        .await
        .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(calls, 1);

        let error = search_with_fallback("Gerry Mulligan Night Lights", |_| {
            calls += 1;
            async { Err(anyhow::anyhow!("Bandcamp offline")) }
        })
        .await
        .unwrap_err();
        assert!(error.to_string().contains("Bandcamp offline"));
        assert_eq!(calls, 2);
    }

    #[tokio::test]
    async fn fallback_keeps_two_words_and_limits_requests() {
        let mut queries = Vec::new();
        let result = search_with_fallback("one two three four five six seven eight", |query| {
            queries.push(query);
            async { Ok(vec![]) }
        })
        .await
        .unwrap();
        assert!(result.is_empty());
        assert_eq!(queries.len(), MAX_SEARCH_FALLBACKS + 1);
        assert_eq!(queries.last().unwrap(), "one two three four");

        let mut queries = Vec::new();
        search_with_fallback("Gerry Mulligan", |query| {
            queries.push(query);
            async { Ok(vec![]) }
        })
        .await
        .unwrap();
        assert_eq!(queries, ["Gerry Mulligan"]);
    }

    fn config(url: &str) -> AiConfig {
        AiConfig {
            url: url.into(),
            model: "test-model".into(),
        }
    }

    #[test]
    fn parses_only_bounded_search_terms() {
        assert_eq!(
            parse_terms("[\"artist song\",\"other\"]").unwrap(),
            vec!["artist song", "other"]
        );
        for text in [
            "```json\n[\"a\"]\n```",
            "[]",
            "[\"\"]",
            "{\"tracks\":[]}",
            "[\"a\", 1]",
        ] {
            assert!(parse_terms(text).is_err());
        }
    }

    #[test]
    fn takes_multiple_distinct_tracks_per_term_without_overfilling() {
        let make = |name: &str, url: &str| Track {
            band_id: 42,
            track: name.into(),
            url: url.into(),
            ..Default::default()
        };
        let mut queue = VecDeque::new();
        let mut seen = HashSet::new();
        add_matches(
            &mut queue,
            &mut seen,
            vec![
                make("one", "1"),
                make("one", "another stream for same song"),
                make("unavailable", ""),
                make("two", "2"),
                make("three", "3"),
                make("four", "4"),
            ],
        );
        assert_eq!(
            queue.iter().map(|t| t.track.as_str()).collect::<Vec<_>>(),
            vec!["one", "two", "three"]
        );
        add_matches(
            &mut queue,
            &mut seen,
            vec![make("two", "new"), make("four", "4")],
        );
        assert_eq!(queue.len(), 4);
        assert_eq!(queue.back().unwrap().track, "four");

        let many: Vec<Track> = (0..30)
            .map(|i| make(&format!("song {i}"), &format!("{i}")))
            .collect();
        for _ in 0..20 {
            add_matches(&mut queue, &mut seen, many.clone());
        }
        assert_eq!(queue.len(), MAX_PLAYLIST_TRACKS);
    }

    #[test]
    fn validates_and_builds_chat_endpoint() {
        assert_eq!(
            config("https://example.com/v1/")
                .chat_url()
                .unwrap()
                .as_str(),
            "https://example.com/v1/chat/completions"
        );
        assert_eq!(
            config("http://localhost:1234/v1")
                .chat_url()
                .unwrap()
                .as_str(),
            "http://localhost:1234/v1/chat/completions"
        );
        assert_eq!(
            config("http://litellm.internal:4000/v1")
                .chat_url()
                .unwrap()
                .as_str(),
            "http://litellm.internal:4000/v1/chat/completions"
        );
        for url in [
            "https://user:pass@example.com/v1",
            "http://user:pass@litellm.internal:4000/v1",
            "https://example.com/v1?token=x",
            "file:///tmp/x",
            "https://example.com/v1#x",
        ] {
            assert!(
                config(url).validate().is_err(),
                "unexpectedly accepted {url}"
            );
        }
        assert!(AiConfig {
            model: " ".into(),
            ..config("https://example.com/v1")
        }
        .validate()
        .is_err());
    }

    #[test]
    fn config_round_trip_without_key_or_side_effects_on_read() {
        let path = std::env::temp_dir()
            .join(format!(
                "bcradio-ai-test-{}-{}",
                std::process::id(),
                std::thread::current().name().unwrap_or("test")
            ))
            .join("ai.json");
        assert!(read_at(&path).unwrap().is_none());
        assert!(!path.exists());
        let expected = config("https://example.com/v1");
        write_at(&path, &expected).unwrap();
        assert_eq!(read_at(&path).unwrap(), Some(expected));
        let data = fs::read_to_string(&path).unwrap();
        assert!(!data.contains("api_key"));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn sends_compatible_request_and_parses_response() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = vec![0; 8192];
            let n = socket.read(&mut request).await.unwrap();
            let text = String::from_utf8_lossy(&request[..n]);
            assert!(text.starts_with("POST /v1/chat/completions HTTP/1.1"));
            assert!(text
                .to_ascii_lowercase()
                .contains("authorization: bearer test-key"));
            let body = r#"{"choices":[{"message":{"content":"ok"}}]}"#;
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        });
        let result = complete_with(
            &config(&format!("http://127.0.0.1:{}/v1", address.port())),
            "test-key",
            "system",
            "user",
        )
        .await
        .unwrap();
        assert_eq!(result, "ok");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn waits_for_slow_ai_response() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        assert!(AI_REQUEST_TIMEOUT > Duration::from_secs(60));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 8192];
            socket.read(&mut request).await.unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;
            let body = r#"{"choices":[{"message":{"content":"ok"}}]}"#;
            socket
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });
        assert_eq!(
            complete_with(
                &config(&format!("http://127.0.0.1:{port}/v1")),
                "test-key",
                "system",
                "user"
            )
            .await
            .unwrap(),
            "ok"
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn failed_request_reports_elapsed_time_and_cause_without_key() {
        use tokio::io::AsyncReadExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 8192];
            socket.read(&mut request).await.unwrap();
            // Close before sending response headers.
        });
        let error = complete_with(
            &config(&format!("http://127.0.0.1:{port}/v1")),
            "secret-test-key",
            "system",
            "user",
        )
        .await
        .unwrap_err();
        let text = format!("{error:#}");
        assert!(text.contains("AI API request failed after"), "{text}");
        assert!(text.contains("timed out: false"), "{text}");
        assert!(text.contains("request limit: 300s"), "{text}");
        assert!(!text.contains("secret-test-key"));
        server.await.unwrap();
    }
}
