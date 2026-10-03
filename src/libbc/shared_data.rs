use crate::libbc::ai::ai_track_key;
use crate::libbc::input_gate::InputGate;
#[cfg(test)]
use crate::libbc::trailing_silence::EndMarker;
use crate::models::bc_discover_index::{Element, PostData};
use crate::models::shared_data_models::{CurrentTrack, State, Track};
use anyhow::Result;
use chrono::{DateTime, Local, TimeDelta};
use log::info;
use std::clone::Clone;
use std::collections::{HashSet, VecDeque};
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;

#[derive(Default, Debug)]
pub struct SharedState {
    pub state: Arc<Mutex<State>>,
    pub changed: Arc<Notify>,
    pub input_gate: InputGate,
    phantom: PhantomData<&'static ()>,
}

impl Clone for SharedState {
    fn clone(&self) -> Self {
        SharedState {
            state: Arc::clone(&self.state),
            changed: self.changed.clone(),
            input_gate: self.input_gate.clone(),
            phantom: Default::default(),
        }
    }
}

impl SharedState {
    pub fn recent_songs(&self) -> Vec<String> {
        let lock = self.state.lock().unwrap();
        let now = Local::now();
        lock.player
            .history
            .iter()
            .rev()
            .filter(|song| now.signed_duration_since(song.play_date) < TimeDelta::minutes(60))
            .take(30)
            .map(|song| format!("{} - {}", song.artist_name, song.track))
            .collect()
    }

    pub fn history(&self) -> Vec<CurrentTrack> {
        self.state
            .lock()
            .unwrap()
            .player
            .history
            .iter()
            .rev()
            .cloned()
            .collect()
    }

    /// Claim a refill before spawning it, so the playback loop cannot start duplicates.
    pub fn claim_ai_refill(&self) -> Option<(String, Vec<String>, u64)> {
        let mut lock = self.state.lock().unwrap();
        if lock.player.tracks.len() > 1
            || lock.player.ai_refill_in_progress
            || lock
                .player
                .ai_retry_after
                .is_some_and(|until| until > Local::now())
        {
            return None;
        }
        let description = lock.player.ai_description.clone()?;
        lock.player.ai_refill_in_progress = true;
        Some((
            description,
            lock.player.ai_terms.clone(),
            lock.player.ai_generation,
        ))
    }

    /// Ignore results from requests started before a manual playlist change.
    pub fn finish_ai_refill(
        &self,
        generation: u64,
        result: Result<(VecDeque<Track>, Vec<String>)>,
    ) {
        let mut lock = self.state.lock().unwrap();
        if lock.player.ai_generation != generation || lock.player.ai_description.is_none() {
            return;
        }
        lock.player.ai_refill_in_progress = false;
        self.changed.notify_one();
        match result {
            Ok((tracks, terms)) => {
                lock.player.ai_terms.extend(terms);
                if lock.player.ai_terms.len() > 40 {
                    let excess = lock.player.ai_terms.len() - 40;
                    lock.player.ai_terms.drain(..excess);
                }
                let mut available = filter_ai_candidates(
                    &lock.player.tracks,
                    &lock.player.history,
                    tracks,
                    Local::now(),
                );
                if available.is_empty() {
                    crate::libbc::terminal::print_error(
                        "No new tracks found; retrying AI playlist in 5 minutes",
                    );
                    lock.player.ai_retry_after = Some(Local::now() + TimeDelta::minutes(5));
                } else {
                    lock.player.tracks.append(&mut available);
                    if lock.player.tracks.len() < 2 {
                        lock.player.ai_retry_after = Some(Local::now() + TimeDelta::seconds(30));
                    }
                }
            }
            Err(e) => {
                crate::libbc::terminal::print_error(format!(
                    "AI refill failed: {e:#}; retrying in 5 minutes"
                ));
                lock.player.ai_retry_after = Some(Local::now() + TimeDelta::minutes(5));
            }
        }
    }

    pub fn is_ai_playlist(&self) -> bool {
        self.state.lock().unwrap().player.ai_description.is_some()
    }

    pub fn queue_length_from_truck_list(&self) -> usize {
        let lock = self.state.lock().unwrap();
        lock.player.tracks.len()
    }

    /// Discard recently played AI entries before choosing a preparation target.
    pub fn preparation_track(&self) -> Option<Track> {
        let mut lock = self.state.lock().unwrap();
        while lock.player.tracks.front().is_some_and(|track| {
            track.ai_generated
                && recently_played(
                    &lock.player.history,
                    track.band_id,
                    &track.track,
                    Local::now(),
                )
        }) {
            lock.player.tracks.pop_front();
        }
        lock.player.tracks.front().cloned()
    }

    /// Commit only the front track the player actually prepared.
    pub fn activate_prepared_track(&self, url: &str, duration: Duration) -> bool {
        let mut lock = self.state.lock().unwrap();
        if !lock
            .player
            .tracks
            .front()
            .is_some_and(|track| track.url == url)
        {
            return false;
        }
        let track = lock.player.tracks.pop_front().unwrap();
        lock.player.current_track = CurrentTrack {
            duration: duration.as_secs_f32(),
            track: track.track,
            album_title: track.album_title,
            art_id: track.art_id,
            band_id: track.band_id,
            artist_name: track.artist_name,
            play_date: Local::now(),
            results: track.results,
            genre: track.genre,
            subgenre: track.subgenre,
            ..Default::default()
        };
        true
    }

    pub fn skip_pending_track(&self) {
        self.state.lock().unwrap().player.tracks.pop_front();
    }

    pub fn append_tracklist(&self, playlist: VecDeque<Track>) {
        let mut lock = self.state.lock().unwrap();
        let mut playlist = filter_candidates(&lock.player.tracks, playlist);
        lock.player.tracks.append(&mut playlist);
    }

    pub fn push_front_tracklist(&self, mut playlist: Track) {
        let mut lock = self.state.lock().unwrap();
        playlist.ai_generated = false;
        // Explicit search selections may move a queued song to the front.
        lock.player.tracks.retain(|song| {
            playlist.track.is_empty()
                || song.band_id != playlist.band_id
                || song.track != playlist.track
        });
        lock.player.tracks.push_front(playlist);
    }

    #[allow(dead_code)]
    pub fn insert_tracklist(&self, n: usize, playlist: Track) {
        let mut lock = self.state.lock().unwrap();
        lock.player.tracks.insert(n, playlist);
    }

    pub fn clear_all_tracklist(&self) {
        info!("clear_all_tracklist\r");
        let mut lock = self.state.lock().unwrap();
        lock.player.tracks.clear();
        lock.player.ai_description = None;
        lock.player.ai_terms.clear();
        lock.player.ai_retry_after = None;
        lock.player.ai_refill_in_progress = false;
        lock.player.ai_generation = lock.player.ai_generation.wrapping_add(1);
    }

    pub fn start_ai_playlist(
        &self,
        description: String,
        terms: Vec<String>,
        tracks: VecDeque<Track>,
    ) -> bool {
        let mut lock = self.state.lock().unwrap();
        let tracks =
            filter_ai_candidates(&VecDeque::new(), &lock.player.history, tracks, Local::now());
        if tracks.is_empty() {
            return false;
        }
        lock.player.tracks = tracks;
        lock.player.post_data.cursor = None;
        lock.player.ai_description = Some(description);
        lock.player.ai_terms = terms;
        lock.player.ai_retry_after = None;
        lock.player.ai_refill_in_progress = false;
        lock.player.ai_generation = lock.player.ai_generation.wrapping_add(1);
        true
    }

    pub fn drain_tracklist(&self, l: usize) {
        let mut lock = self.state.lock().unwrap();
        lock.player.tracks.drain(..l - 1);
    }

    pub fn get_tracklist(&self) -> VecDeque<Track> {
        let lock = self.state.lock().unwrap();
        lock.player.tracks.clone()
    }

    pub fn set_next_postdata(&self, post_data: &PostData) {
        let mut lock = self.state.lock().unwrap();
        lock.player.post_data = post_data.clone();
    }

    pub fn save_genres(&self, genres: Vec<Element>, subgenres: Vec<Element>) {
        let mut lock = self.state.lock().unwrap();
        lock.player.genres = genres;
        lock.player.subgenres = subgenres;
    }

    pub fn get_genres(&self) -> (Vec<Element>, Vec<Element>) {
        let lock = self.state.lock().unwrap();
        (
            lock.player.genres.to_owned(),
            lock.player.subgenres.to_owned(),
        )
    }

    pub fn set_genre(&self, genre: &str) {
        let mut lock = self.state.lock().unwrap();
        lock.player.genre = genre.to_owned();
    }
    #[allow(dead_code)]
    pub fn get_genre(&self) -> String {
        let lock = self.state.lock().unwrap();
        lock.player.genre.to_owned()
    }

    pub fn set_subgenre(&self, subgenre: &str) {
        let mut lock = self.state.lock().unwrap();
        lock.player.subgenre = subgenre.to_owned();
    }
    #[allow(dead_code)]
    pub fn get_subgenre(&self) -> String {
        let lock = self.state.lock().unwrap();
        lock.player.subgenre.to_owned()
    }

    pub fn next_post(&self) -> PostData {
        self.state.lock().unwrap().player.post_data.to_owned()
    }

    #[cfg(test)]
    pub fn set_track_buffer(&self, url: &str, buf: Vec<u8>, duration: Duration) {
        self.set_analyzed_track_buffer(url, buf, duration, None);
    }
    #[cfg(test)]
    pub fn set_analyzed_track_buffer(
        &self,
        url: &str,
        buf: Vec<u8>,
        duration: Duration,
        marker: Option<EndMarker>,
    ) {
        let mut lock = self.state.lock().unwrap();
        if let Some(track) = lock.player.tracks.front_mut() {
            if track.url == url && track.buffer.is_empty() {
                track.buffer = buf;
                track.duration = duration.as_secs_f32();
                track.end_marker = marker;
            }
        }
    }
    #[cfg(test)]
    pub fn take_ready_track(&self) -> Option<Vec<u8>> {
        self.take_ready_audio().map(|(buffer, _)| buffer)
    }
    #[cfg(test)]
    pub fn take_ready_audio(&self) -> Option<(Vec<u8>, Option<EndMarker>)> {
        let mut lock = self.state.lock().unwrap();
        while lock.player.tracks.front().is_some_and(|track| {
            track.ai_generated
                && recently_played(
                    &lock.player.history,
                    track.band_id,
                    &track.track,
                    Local::now(),
                )
        }) {
            lock.player.tracks.pop_front();
        }
        if !lock
            .player
            .tracks
            .front()
            .is_some_and(|track| !track.buffer.is_empty())
        {
            return None;
        }
        let mut track = lock.player.tracks.pop_front()?;
        let buffer = std::mem::take(&mut track.buffer);
        lock.player.current_track.duration = track.duration;
        lock.player.current_track.track = track.track;
        lock.player.current_track.album_title = track.album_title;
        lock.player.current_track.art_id = track.art_id;
        lock.player.current_track.band_id = track.band_id;
        lock.player.current_track.artist_name = track.artist_name;
        lock.player.current_track.play_date = Local::now();
        lock.player.current_track.results = track.results;
        lock.player.current_track.genre = track.genre;
        lock.player.current_track.subgenre = track.subgenre;
        Some((buffer, track.end_marker))
    }

    pub fn record_playback_start(&self) {
        let mut lock = self.state.lock().unwrap();
        lock.player.current_track.play_date = Local::now();
        let current = lock.player.current_track.clone();
        lock.player.history.push_back(current);
        while lock.player.history.len() > 500
            && lock.player.history.front().is_some_and(|song| {
                Local::now().signed_duration_since(song.play_date) >= TimeDelta::minutes(60)
            })
        {
            lock.player.history.pop_front();
        }
    }

    pub fn get_current_track_info(&self) -> CurrentTrack {
        let lock = self.state.lock().unwrap();
        lock.player.current_track.to_owned()
    }

    pub fn get_current_art_id(&self) -> Option<i64> {
        let lock = self.state.lock().unwrap();
        lock.player.current_track.art_id
    }
}

fn recently_played(
    history: &VecDeque<CurrentTrack>,
    band_id: i64,
    title: &str,
    now: DateTime<Local>,
) -> bool {
    !title.is_empty()
        && history.iter().any(|song| {
            song.band_id == band_id
                && song.track == title
                && now.signed_duration_since(song.play_date) < TimeDelta::minutes(60)
        })
}

fn filter_candidates(queued: &VecDeque<Track>, candidates: VecDeque<Track>) -> VecDeque<Track> {
    let mut seen: HashSet<(i64, String)> = queued
        .iter()
        .map(|song| (song.band_id, song.track.clone()))
        .collect();
    candidates
        .into_iter()
        .filter(|song| song.track.is_empty() || seen.insert((song.band_id, song.track.clone())))
        .collect()
}

fn filter_ai_candidates(
    queued: &VecDeque<Track>,
    history: &VecDeque<CurrentTrack>,
    candidates: VecDeque<Track>,
    now: DateTime<Local>,
) -> VecDeque<Track> {
    let mut seen: HashSet<(String, String)> = queued.iter().map(ai_track_key).collect();
    let recently_played_keys: HashSet<(String, String)> = history
        .iter()
        .filter(|song| now.signed_duration_since(song.play_date) < TimeDelta::minutes(60))
        .map(|song| {
            ai_track_key(&Track {
                band_id: song.band_id,
                artist_name: song.artist_name.clone(),
                track: song.track.clone(),
                ..Default::default()
            })
        })
        .collect();
    filter_candidates(queued, candidates)
        .into_iter()
        .filter(|song| {
            let key = ai_track_key(song);
            !recently_played(history, song.band_id, &song.track, now)
                && !recently_played_keys.contains(&key)
                && seen.insert(key)
        })
        .map(|mut song| {
            song.ai_generated = true;
            song
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(name: &str) -> Track {
        Track {
            band_id: 42,
            track: name.into(),
            url: format!("https://example.com/{name}"),
            ..Default::default()
        }
    }

    #[test]
    fn ai_filters_queue_duplicates_and_only_recent_history() {
        let now = Local::now();
        let queue = VecDeque::from([song("queued")]);
        let history = VecDeque::from([
            CurrentTrack {
                band_id: 42,
                track: "recent".into(),
                play_date: now - TimeDelta::minutes(59),
                ..Default::default()
            },
            CurrentTrack {
                band_id: 42,
                track: "expired".into(),
                play_date: now - TimeDelta::minutes(60),
                ..Default::default()
            },
        ]);
        let candidates = VecDeque::from([
            song("queued"),
            song("recent"),
            song("expired"),
            song("new"),
            song("new"),
        ]);
        let filtered = filter_ai_candidates(&queue, &history, candidates, now);
        assert_eq!(
            filtered
                .iter()
                .map(|s| s.track.as_str())
                .collect::<Vec<_>>(),
            vec!["expired", "new"]
        );
    }

    #[test]
    fn ai_queue_and_recent_history_deduplicate_across_band_ids() {
        let now = Local::now();
        let make = |id, artist: &str, title: &str| Track {
            band_id: id,
            artist_name: artist.into(),
            track: title.into(),
            url: format!("https://example.com/{id}"),
            ..Default::default()
        };
        let queued = VecDeque::from([make(1, "Alabaster DePlume", "Visit Croatia")]);
        let history = VecDeque::from([CurrentTrack {
            band_id: 4,
            artist_name: "Alabaster DePlume".into(),
            track: "Quiet Fire".into(),
            play_date: now,
            ..Default::default()
        }]);
        let candidates = VecDeque::from([
            make(2, "alabaster deplume", "VISIT CROATIA!"),
            make(5, "Alabaster DePlume", "Quiet Fire"),
            make(3, "Other Artist", "Visit Croatia"),
        ]);
        let filtered = filter_ai_candidates(&queued, &history, candidates, now);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].artist_name, "Other Artist");
    }

    #[test]
    fn records_start_once_and_allows_non_ai_replay() {
        let state = SharedState::default();
        let mut first = song("first");
        first.buffer = vec![1];
        state.append_tracklist(VecDeque::from([first]));
        assert_eq!(state.take_ready_track(), Some(vec![1]));
        assert!(state.history().is_empty());
        state.record_playback_start();
        assert_eq!(state.history().len(), 1);
        assert_eq!(state.history()[0].track, "first");
        state.append_tracklist(VecDeque::from([song("first"), song("next")]));
        assert_eq!(state.get_tracklist().len(), 2);
        assert_eq!(state.preparation_track().unwrap().track, "first");
    }

    #[test]
    fn end_marker_travels_with_its_buffer_and_keeps_original_duration() {
        let state = SharedState::default();
        state.push_front_tracklist(Track {
            url: "marked".into(),
            ..Default::default()
        });
        let marker = EndMarker {
            frame: 48_000,
            sample_rate: 48_000,
        };
        state.set_analyzed_track_buffer("stale", vec![9], Duration::from_secs(9), Some(marker));
        assert!(state.take_ready_audio().is_none());
        state.set_analyzed_track_buffer("marked", vec![1, 2], Duration::from_secs(3), Some(marker));
        assert_eq!(state.take_ready_audio(), Some((vec![1, 2], Some(marker))));
        assert_eq!(state.get_current_track_info().duration, 3.0);
    }

    #[test]
    fn manual_search_can_replay_recent_songs_even_during_ai_playback() {
        for ai_active in [false, true] {
            let state = SharedState::default();
            if ai_active {
                assert!(state.start_ai_playlist(
                    "jazz".into(),
                    vec![],
                    VecDeque::from([song("next")]),
                ));
            }
            state
                .state
                .lock()
                .unwrap()
                .player
                .history
                .push_back(CurrentTrack {
                    band_id: 42,
                    track: "recent".into(),
                    play_date: Local::now(),
                    ..Default::default()
                });
            let mut selected = song("recent");
            selected.ai_generated = true;
            state.push_front_tracklist(selected.clone());
            state.push_front_tracklist(selected);
            assert_eq!(
                state
                    .get_tracklist()
                    .iter()
                    .filter(|s| s.track == "recent")
                    .count(),
                1
            );
            let track = state
                .preparation_track()
                .expect("manual song must not be discarded");
            assert_eq!(track.track, "recent");
            assert!(!track.ai_generated);
            assert_eq!(state.is_ai_playlist(), ai_active);
            assert!(state.activate_prepared_track(&track.url, Duration::from_secs(2)));
            assert_eq!(state.get_current_track_info().track, "recent");
        }
    }

    #[test]
    fn ai_preparation_still_discards_songs_played_since_generation() {
        let state = SharedState::default();
        assert!(state.start_ai_playlist(
            "jazz".into(),
            vec![],
            VecDeque::from([song("recent"), song("next")]),
        ));
        assert!(state.get_tracklist().iter().all(|s| s.ai_generated));
        state
            .state
            .lock()
            .unwrap()
            .player
            .history
            .push_back(CurrentTrack {
                band_id: 42,
                track: "recent".into(),
                play_date: Local::now(),
                ..Default::default()
            });
        assert_eq!(state.preparation_track().unwrap().track, "next");
        assert_eq!(state.queue_length_from_truck_list(), 1);
    }

    #[test]
    fn ai_context_survives_refill_and_failed_new_selection() {
        let state = SharedState::default();
        assert!(state.start_ai_playlist(
            "jazz".into(),
            vec!["term".into()],
            VecDeque::from([song("one")])
        ));
        state
            .state
            .lock()
            .unwrap()
            .player
            .history
            .push_back(CurrentTrack {
                band_id: 42,
                track: "one".into(),
                play_date: Local::now(),
                ..Default::default()
            });
        assert!(!state.start_ai_playlist("another".into(), vec![], VecDeque::from([song("one")])));
        let (description, terms, generation) = state.claim_ai_refill().unwrap();
        assert_eq!(description, "jazz");
        assert_eq!(terms, vec!["term"]);
        state.finish_ai_refill(
            generation,
            Ok((VecDeque::from([song("two")]), vec!["new term".into()])),
        );
        assert_eq!(
            state.state.lock().unwrap().player.ai_terms,
            vec!["term", "new term"]
        );
        assert!(state.claim_ai_refill().is_none());
        state.clear_all_tracklist();
        assert!(!state.is_ai_playlist());
    }

    #[test]
    fn selecting_ai_replaces_queue_but_preserves_current_song() {
        let state = SharedState::default();
        state.append_tracklist(VecDeque::from([song("queued")]));
        {
            let mut lock = state.state.lock().unwrap();
            lock.player.current_track.track = "playing".into();
            lock.player.post_data.cursor = Some("old cursor".into());
        }
        assert!(state.start_ai_playlist(
            "ambient jazz".into(),
            vec!["new term".into()],
            VecDeque::from([song("new"), song("new"), song("more")]),
        ));
        assert_eq!(state.get_current_track_info().track, "playing");
        assert_eq!(
            state
                .get_tracklist()
                .iter()
                .map(|s| s.track.as_str())
                .collect::<Vec<_>>(),
            vec!["new", "more"]
        );
        assert_eq!(
            state.state.lock().unwrap().player.ai_description.as_deref(),
            Some("ambient jazz")
        );
        assert_eq!(
            state.state.lock().unwrap().player.ai_terms,
            vec!["new term"]
        );
        assert!(state.next_post().cursor.is_none());
    }

    #[test]
    fn failed_ai_selection_does_not_change_queue_or_refill() {
        let state = SharedState::default();
        state.append_tracklist(VecDeque::from([song("queued")]));
        state
            .state
            .lock()
            .unwrap()
            .player
            .history
            .push_back(CurrentTrack {
                band_id: 42,
                track: "played".into(),
                play_date: Local::now(),
                ..Default::default()
            });
        assert!(!state.start_ai_playlist(
            "no matches".into(),
            vec![],
            VecDeque::from([song("played")])
        ));
        assert_eq!(state.get_tracklist()[0].track, "queued");
        assert!(!state.is_ai_playlist());
        assert!(state.next_post().cursor.is_some());
    }

    #[test]
    fn refill_claims_last_queued_song_once_and_keeps_it_until_search_finishes() {
        let state = SharedState::default();
        assert!(state.start_ai_playlist(
            "jazz".into(),
            vec![],
            VecDeque::from([song("one"), song("two")])
        ));
        assert!(state.claim_ai_refill().is_none());
        state.state.lock().unwrap().player.tracks.pop_front();
        let (_, _, generation) = state.claim_ai_refill().unwrap();
        assert!(state.claim_ai_refill().is_none());
        assert_eq!(state.get_tracklist()[0].track, "two");
        state.finish_ai_refill(
            generation,
            Ok((
                VecDeque::from([song("two"), song("three")]),
                vec!["term".into()],
            )),
        );
        assert_eq!(
            state
                .get_tracklist()
                .iter()
                .map(|s| s.track.as_str())
                .collect::<Vec<_>>(),
            vec!["two", "three"]
        );
        assert!(state.claim_ai_refill().is_none());
    }

    #[test]
    fn old_refill_cannot_modify_a_new_playlist() {
        let state = SharedState::default();
        assert!(state.start_ai_playlist("first".into(), vec![], VecDeque::from([song("one")])));
        let (_, _, generation) = state.claim_ai_refill().unwrap();
        assert!(state.start_ai_playlist("second".into(), vec![], VecDeque::from([song("two")])));
        state.finish_ai_refill(
            generation,
            Ok((VecDeque::from([song("old")]), vec!["stale".into()])),
        );
        assert_eq!(state.get_tracklist()[0].track, "two");
        assert_eq!(
            state.state.lock().unwrap().player.ai_description.as_deref(),
            Some("second")
        );
        assert!(state.state.lock().unwrap().player.ai_terms.is_empty());
    }

    #[test]
    fn ai_playlist_replaces_queue_and_disables_old_discover_cursor() {
        let state = SharedState::default();
        state.push_front_tracklist(Track {
            track: "old".into(),
            ..Default::default()
        });
        assert!(state.start_ai_playlist(
            "new playlist".into(),
            vec![],
            VecDeque::from([Track {
                track: "new".into(),
                ..Default::default()
            }])
        ));
        assert_eq!(state.get_tracklist().len(), 1);
        assert_eq!(state.get_tracklist()[0].track, "new");
        assert!(state.next_post().cursor.is_none());
    }

    #[test]
    fn only_takes_buffered_front_track() {
        let state = SharedState::default();
        state.append_tracklist(VecDeque::from([
            Track {
                url: "first-url".into(),
                track: "first".into(),
                ..Default::default()
            },
            Track {
                url: "second-url".into(),
                track: "second".into(),
                buffer: vec![1, 2, 3],
                ..Default::default()
            },
        ]));
        assert!(state.take_ready_track().is_none());
        assert_eq!(state.queue_length_from_truck_list(), 2);
        assert!(state.get_current_track_info().track.is_empty());

        state.set_track_buffer("first-url", vec![4], Duration::from_secs(2));
        assert_eq!(state.take_ready_track(), Some(vec![4]));
        assert_eq!(state.get_current_track_info().track, "first");
        assert_eq!(state.queue_length_from_truck_list(), 1);
    }

    #[test]
    fn stale_download_does_not_buffer_a_later_matching_track() {
        let state = SharedState::default();
        state.append_tracklist(VecDeque::from([
            Track {
                url: "first".into(),
                ..Default::default()
            },
            Track {
                url: "old".into(),
                ..Default::default()
            },
        ]));
        state.set_track_buffer("old", vec![9], Duration::from_secs(1));
        assert!(state.take_ready_track().is_none());
        state.push_front_tracklist(Track {
            url: "new".into(),
            ..Default::default()
        });
        state.set_track_buffer("first", vec![9], Duration::from_secs(1));
        assert!(state.take_ready_track().is_none());
    }

    #[test]
    fn stale_preparation_cannot_activate_a_different_front_track() {
        let state = SharedState::default();
        state.append_tracklist(VecDeque::from([song("first"), song("second")]));
        let first = state.preparation_track().unwrap();
        state.skip_pending_track();
        assert!(!state.activate_prepared_track(&first.url, Duration::from_secs(1)));
        let second = state.preparation_track().unwrap();
        assert!(state.activate_prepared_track(&second.url, Duration::from_secs(2)));
        assert_eq!(state.get_current_track_info().track, "second");
        assert_eq!(state.get_current_track_info().duration, 2.0);
        assert_eq!(state.queue_length_from_truck_list(), 0);
    }
}
