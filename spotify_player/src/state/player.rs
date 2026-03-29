use super::model::{
    AlbumId, ArtistId, ContextId, Device, Id, PlayableId, PlaybackMetadata, PlaylistId, ShowId,
    TracksId,
};

/// Player state
#[derive(Default, Debug)]
pub struct PlayerState {
    pub devices: Vec<Device>,

    pub playback: Option<rspotify::model::CurrentPlaybackContext>,
    pub playback_last_updated_time: Option<std::time::Instant>,
    /// A buffered state to speedup the feedback of playback metadata update to user
    // Related issue: https://github.com/aome510/spotify-player/issues/109
    pub buffered_playback: Option<PlaybackMetadata>,

    pub queue: Option<rspotify::model::CurrentUserQueue>,

    /// The currently playing Tracks context (for contexts not tracked by Spotify's playback, e.g. liked/top tracks)
    pub currently_playing_tracks_id: Option<TracksId>,

    /// State for batched sorted playback -- tracks the full sorted list and current batch window
    pub sorted_playback: Option<SortedPlaybackState>,
}

/// Tracks the full sorted playlist for batched playback continuation.
/// Keeps all track IDs and the current window position so we can
/// navigate forward (auto-advance + skip) and backward (previous track).
#[derive(Debug)]
pub struct SortedPlaybackState {
    /// Complete sorted list of track IDs (entire sorted playlist from play-start to end).
    pub tracks: Vec<PlayableId<'static>>,
    /// Index into `tracks` where the current batch starts.
    pub batch_start: usize,
    /// Index into `tracks` where the current batch ends (exclusive).
    pub batch_end: usize,
}

impl SortedPlaybackState {
    /// Returns the next batch of track IDs, advancing the window. Returns None if at end.
    pub fn next_batch(&mut self, limit: usize) -> Option<Vec<PlayableId<'static>>> {
        if self.batch_end >= self.tracks.len() {
            return None;
        }
        self.batch_start = self.batch_end;
        self.batch_end = std::cmp::min(self.batch_start + limit, self.tracks.len());
        Some(self.tracks[self.batch_start..self.batch_end].to_vec())
    }

    /// Returns the previous batch of track IDs, moving the window back. Returns None if at start.
    pub fn prev_batch(&mut self, limit: usize) -> Option<Vec<PlayableId<'static>>> {
        if self.batch_start == 0 {
            return None;
        }
        self.batch_end = self.batch_start;
        self.batch_start = self.batch_end.saturating_sub(limit);
        Some(self.tracks[self.batch_start..self.batch_end].to_vec())
    }

    /// True if the given track URI is the last track in the current batch.
    pub fn is_last_in_batch(&self, uri: &str) -> bool {
        self.batch_end > 0
            && self.batch_end <= self.tracks.len()
            && self.tracks[self.batch_end - 1].uri() == uri
    }

    /// True if the given track URI is the first track in the current batch.
    pub fn is_first_in_batch(&self, uri: &str) -> bool {
        self.batch_start < self.tracks.len() && self.tracks[self.batch_start].uri() == uri
    }

    /// True if there are more tracks after the current batch.
    pub fn has_next(&self) -> bool {
        self.batch_end < self.tracks.len()
    }

    /// True if there are tracks before the current batch.
    pub fn has_prev(&self) -> bool {
        self.batch_start > 0
    }
}

impl PlayerState {
    /// Get the current playback
    ///
    /// # Note
    /// Because playback metadata stored inside the player state is buffered,
    /// the returned playback is estimated based on the available data.
    pub fn current_playback(&self) -> Option<rspotify::model::CurrentPlaybackContext> {
        let mut playback = self.playback.clone()?;

        // update the playback's progress based on the `playback_last_updated_time`
        playback.progress = playback.progress.map(|d| {
            d + if playback.is_playing {
                chrono::Duration::from_std(self.playback_last_updated_time.unwrap().elapsed())
                    .unwrap()
            } else {
                chrono::Duration::zero()
            }
        });

        // update the playback's metadata based on the `buffered_playback` metadata
        if let Some(ref p) = self.buffered_playback {
            playback.device.name.clone_from(&p.device_name);
            playback.device.id.clone_from(&p.device_id);
            playback.is_playing = p.is_playing;
            playback.device.volume_percent = p.volume;
            playback.repeat_state = p.repeat_state;
            playback.shuffle_state = p.shuffle_state;
        }

        Some(playback)
    }

    pub fn currently_playing(&self) -> Option<&rspotify::model::PlayableItem> {
        self.playback.as_ref().and_then(|p| p.item.as_ref())
    }

    pub fn playback_progress(&self) -> Option<chrono::Duration> {
        match self.playback {
            None => None,
            Some(ref playback) => {
                let progress = playback.progress.unwrap()
                    + if playback.is_playing {
                        chrono::Duration::from_std(
                            self.playback_last_updated_time.unwrap().elapsed(),
                        )
                        .ok()?
                    } else {
                        chrono::Duration::zero()
                    };
                Some(progress)
            }
        }
    }

    pub fn playing_context_id(&self) -> Option<ContextId> {
        match self.playback {
            Some(ref playback) => match playback.context {
                Some(ref context) => {
                    let uri = crate::utils::parse_uri(&context.uri);
                    match context._type {
                        rspotify::model::Type::Playlist => Some(ContextId::Playlist(
                            PlaylistId::from_uri(&uri).ok()?.into_static(),
                        )),
                        rspotify::model::Type::Album => Some(ContextId::Album(
                            AlbumId::from_uri(&uri).ok()?.into_static(),
                        )),
                        rspotify::model::Type::Artist => Some(ContextId::Artist(
                            ArtistId::from_uri(&uri).ok()?.into_static(),
                        )),
                        rspotify::model::Type::Show => {
                            Some(ContextId::Show(ShowId::from_uri(&uri).ok()?.into_static()))
                        }
                        _ => None,
                    }
                }
                None => self
                    .currently_playing_tracks_id
                    .clone()
                    .map(ContextId::Tracks),
            },
            None => None,
        }
    }
}
