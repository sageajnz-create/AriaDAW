//! The user's music library.
//!
//! Design rule: **the database is an index, never a cage.** Audio lives as ordinary
//! files in a folder the user can open, back up, or drag into any other program.
//! If Aria is deleted, the music is still there and still playable. A tool that
//! claims you own your output has to mean it on disk.

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: String,
    pub title: String,
    /// What the user actually typed.
    pub prompt: String,
    /// The (often rewritten) caption the model generated from it.
    pub caption: String,
    pub lyrics: String,
    pub bpm: Option<i64>,
    pub keyscale: Option<String>,
    pub timesignature: Option<String>,
    pub vocal_language: Option<String>,
    pub duration: f64,
    pub seed: Option<i64>,
    pub model: String,
    pub audio_path: String,
    pub latent_path: Option<String>,
    pub created_at: i64,
    /// Lineage: which track this was derived from, and how.
    pub parent_id: Option<String>,
    pub operation: Option<String>,
    pub favorite: bool,
    /// Which model wrote the words, when it wasn't the user or ACE-Step itself.
    #[serde(default)]
    pub lyricist: Option<String>,
    /// True when the audio file is no longer where we left it.
    ///
    /// Not stored — computed on read. Aria deliberately keeps music as ordinary
    /// files people are free to rename, move or delete, so the index going stale
    /// is expected behaviour rather than corruption. Saying so plainly beats a
    /// track that silently refuses to play.
    #[serde(default)]
    pub missing: bool,
}

/// A named, ordered set of tracks.
///
/// Membership travels with the playlist rather than the track, so a song can
/// sit in several at once and removing it from one is not a deletion. Ids are
/// returned inline because a library that fits in memory makes a round trip per
/// playlist pure overhead — the UI needs the whole picture to render chips.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub created_at: i64,
    /// Track ids in playing order.
    pub track_ids: Vec<String>,
}

pub struct Library {
    conn: Connection,
    pub audio_dir: PathBuf,
}

impl Library {
    /// Open (creating if needed) the library at `data_dir`, storing audio in `audio_dir`.
    pub fn open(data_dir: &Path, audio_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(data_dir)
            .with_context(|| format!("creating {}", data_dir.display()))?;
        std::fs::create_dir_all(audio_dir)
            .with_context(|| format!("creating {}", audio_dir.display()))?;

        let conn = Connection::open(data_dir.join("library.db"))?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS tracks (
                id              TEXT PRIMARY KEY,
                title           TEXT NOT NULL,
                prompt          TEXT NOT NULL DEFAULT '',
                caption         TEXT NOT NULL DEFAULT '',
                lyrics          TEXT NOT NULL DEFAULT '',
                bpm             INTEGER,
                keyscale        TEXT,
                timesignature   TEXT,
                vocal_language  TEXT,
                duration        REAL NOT NULL DEFAULT 0,
                seed            INTEGER,
                model           TEXT NOT NULL DEFAULT '',
                audio_path      TEXT NOT NULL,
                latent_path     TEXT,
                created_at      INTEGER NOT NULL,
                parent_id       TEXT REFERENCES tracks(id) ON DELETE SET NULL,
                operation       TEXT,
                favorite        INTEGER NOT NULL DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_tracks_created ON tracks(created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_tracks_parent  ON tracks(parent_id);

            CREATE TABLE IF NOT EXISTS playlists (
                id          TEXT PRIMARY KEY,
                name        TEXT NOT NULL,
                created_at  INTEGER NOT NULL
            );

            -- Membership cascades from both sides: deleting a playlist must not
            -- touch the music, and deleting a song must not leave a hole in a
            -- playlist that then fails to play.
            CREATE TABLE IF NOT EXISTS playlist_tracks (
                playlist_id TEXT NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
                track_id    TEXT NOT NULL REFERENCES tracks(id)    ON DELETE CASCADE,
                position    INTEGER NOT NULL,
                PRIMARY KEY (playlist_id, track_id)
            );

            CREATE INDEX IF NOT EXISTS idx_playlist_order
                ON playlist_tracks(playlist_id, position);
            "#,
        )?;

        // Migrations. `CREATE TABLE IF NOT EXISTS` does nothing for a database
        // that already exists, so new columns have to be added explicitly.
        // Adding a column that is already there is not an error worth failing
        // startup over — losing someone's library to a schema tweak would be.
        for stmt in ["ALTER TABLE tracks ADD COLUMN lyricist TEXT"] {
            let _ = conn.execute(stmt, []);
        }

        Ok(Self { conn, audio_dir: audio_dir.to_path_buf() })
    }

    pub fn insert(&self, t: &Track) -> Result<()> {
        self.conn.execute(
            r#"INSERT INTO tracks
               (id,title,prompt,caption,lyrics,bpm,keyscale,timesignature,vocal_language,
                duration,seed,model,audio_path,latent_path,created_at,parent_id,operation,favorite,
                lyricist)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)"#,
            params![
                t.id, t.title, t.prompt, t.caption, t.lyrics, t.bpm, t.keyscale,
                t.timesignature, t.vocal_language, t.duration, t.seed, t.model,
                t.audio_path, t.latent_path, t.created_at, t.parent_id, t.operation,
                t.favorite as i32, t.lyricist
            ],
        )?;
        Ok(())
    }

    pub fn list(&self, limit: i64) -> Result<Vec<Track>> {
        let mut stmt = self.conn.prepare(
            "SELECT id,title,prompt,caption,lyrics,bpm,keyscale,timesignature,vocal_language,
                    duration,seed,model,audio_path,latent_path,created_at,parent_id,operation,favorite,
                    lyricist
             FROM tracks ORDER BY created_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], row_to_track)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn get(&self, id: &str) -> Result<Option<Track>> {
        let mut stmt = self.conn.prepare(
            "SELECT id,title,prompt,caption,lyrics,bpm,keyscale,timesignature,vocal_language,
                    duration,seed,model,audio_path,latent_path,created_at,parent_id,operation,favorite,
                    lyricist
             FROM tracks WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map([id], row_to_track)?;
        Ok(rows.next().transpose()?)
    }

    pub fn set_favorite(&self, id: &str, fav: bool) -> Result<()> {
        self.conn
            .execute("UPDATE tracks SET favorite = ?2 WHERE id = ?1", params![id, fav as i32])?;
        Ok(())
    }

    pub fn rename(&self, id: &str, title: &str) -> Result<()> {
        self.conn
            .execute("UPDATE tracks SET title = ?2 WHERE id = ?1", params![id, title])?;
        Ok(())
    }

    /// Remove from the index and delete the audio. Explicit and irreversible, so
    /// the UI must confirm before calling it.
    pub fn delete(&self, id: &str) -> Result<()> {
        if let Some(t) = self.get(id)? {
            let _ = std::fs::remove_file(&t.audio_path);
            let _ = std::fs::remove_file(crate::art::path_beside(&t.audio_path));
            if let Some(l) = &t.latent_path {
                let _ = std::fs::remove_file(l);
            }
        }
        self.conn.execute("DELETE FROM tracks WHERE id = ?1", params![id])?;
        Ok(())
    }

    // -- Playlists ------------------------------------------------------

    pub fn playlists(&self) -> Result<Vec<Playlist>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id,name,created_at FROM playlists ORDER BY created_at")?;
        let heads = stmt
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut members = self.conn.prepare(
            "SELECT track_id FROM playlist_tracks WHERE playlist_id = ?1 ORDER BY position",
        )?;
        heads
            .into_iter()
            .map(|(id, name, created_at)| {
                let track_ids = members
                    .query_map([&id], |r| r.get::<_, String>(0))?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                Ok(Playlist { id, name, created_at, track_ids })
            })
            .collect()
    }

    pub fn create_playlist(&self, name: &str) -> Result<Playlist> {
        let id = uuid::Uuid::new_v4().to_string();
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        self.conn.execute(
            "INSERT INTO playlists (id,name,created_at) VALUES (?1,?2,?3)",
            params![id, name, created_at],
        )?;
        Ok(Playlist { id, name: name.to_string(), created_at, track_ids: Vec::new() })
    }

    pub fn rename_playlist(&self, id: &str, name: &str) -> Result<()> {
        self.conn
            .execute("UPDATE playlists SET name = ?2 WHERE id = ?1", params![id, name])?;
        Ok(())
    }

    /// Removes the playlist only. The songs in it are untouched — a playlist is
    /// a view of the library, never a container that owns what it lists.
    pub fn delete_playlist(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM playlists WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Append to the end. Adding a track that's already there is a no-op rather
    /// than an error, so a double click can't produce a duplicate row.
    pub fn add_to_playlist(&self, playlist_id: &str, track_id: &str) -> Result<()> {
        let next: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM playlist_tracks WHERE playlist_id = ?1",
            params![playlist_id],
            |r| r.get(0),
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO playlist_tracks (playlist_id,track_id,position)
             VALUES (?1,?2,?3)",
            params![playlist_id, track_id, next],
        )?;
        Ok(())
    }

    pub fn remove_from_playlist(&self, playlist_id: &str, track_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM playlist_tracks WHERE playlist_id = ?1 AND track_id = ?2",
            params![playlist_id, track_id],
        )?;
        Ok(())
    }

    /// Move a track one step earlier (`-1`) or later (`1`) in the playlist.
    ///
    /// Swaps the two positions rather than renumbering the list, so the cost
    /// doesn't grow with the playlist and concurrent readers never see a gap.
    pub fn move_in_playlist(&self, playlist_id: &str, track_id: &str, delta: i64) -> Result<()> {
        let here: i64 = match self.conn.query_row(
            "SELECT position FROM playlist_tracks WHERE playlist_id = ?1 AND track_id = ?2",
            params![playlist_id, track_id],
            |r| r.get(0),
        ) {
            Ok(p) => p,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(()),
            Err(e) => return Err(e.into()),
        };

        // The neighbour in that direction, whatever its position number is.
        let neighbour: Option<(String, i64)> = self
            .conn
            .query_row(
                if delta < 0 {
                    "SELECT track_id, position FROM playlist_tracks
                     WHERE playlist_id = ?1 AND position < ?2
                     ORDER BY position DESC LIMIT 1"
                } else {
                    "SELECT track_id, position FROM playlist_tracks
                     WHERE playlist_id = ?1 AND position > ?2
                     ORDER BY position ASC LIMIT 1"
                },
                params![playlist_id, here],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok();

        // Already at the end it was asked to move towards.
        let Some((other_id, there)) = neighbour else { return Ok(()) };

        self.conn.execute(
            "UPDATE playlist_tracks SET position = ?3 WHERE playlist_id = ?1 AND track_id = ?2",
            params![playlist_id, track_id, there],
        )?;
        self.conn.execute(
            "UPDATE playlist_tracks SET position = ?3 WHERE playlist_id = ?1 AND track_id = ?2",
            params![playlist_id, other_id, here],
        )?;
        Ok(())
    }

    pub fn count(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))?)
    }
}

fn row_to_track(r: &rusqlite::Row) -> rusqlite::Result<Track> {
    let audio_path: String = r.get(12)?;
    Ok(Track {
        id: r.get(0)?,
        title: r.get(1)?,
        prompt: r.get(2)?,
        caption: r.get(3)?,
        lyrics: r.get(4)?,
        bpm: r.get(5)?,
        keyscale: r.get(6)?,
        timesignature: r.get(7)?,
        vocal_language: r.get(8)?,
        duration: r.get(9)?,
        seed: r.get(10)?,
        model: r.get(11)?,
        audio_path: audio_path.clone(),
        latent_path: r.get(13)?,
        created_at: r.get(14)?,
        parent_id: r.get(15)?,
        operation: r.get(16)?,
        favorite: r.get::<_, i32>(17)? != 0,
        lyricist: r.get(18)?,
        missing: !std::path::Path::new(&audio_path).exists(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(id: &str) -> Track {
        Track {
            id: id.into(),
            title: "Test".into(),
            prompt: "warm folk".into(),
            caption: "warm folk, fingerpicked".into(),
            lyrics: "[Verse]".into(),
            bpm: Some(120),
            keyscale: Some("C major".into()),
            timesignature: Some("4".into()),
            vocal_language: Some("en".into()),
            duration: 60.0,
            seed: Some(42),
            model: "turbo".into(),
            audio_path: "/tmp/none.mp3".into(),
            latent_path: None,
            created_at: 1,
            parent_id: None,
            operation: None,
            favorite: false,
            lyricist: None,
            missing: false,
        }
    }

    #[test]
    fn round_trips_a_track() {
        let dir = std::env::temp_dir().join(format!("aria-test-{}", uuid::Uuid::new_v4()));
        let lib = Library::open(&dir, &dir.join("audio")).unwrap();
        lib.insert(&sample("a")).unwrap();

        let got = lib.get("a").unwrap().unwrap();
        assert_eq!(got.title, "Test");
        assert_eq!(got.bpm, Some(120));
        assert_eq!(lib.count().unwrap(), 1);

        lib.set_favorite("a", true).unwrap();
        assert!(lib.get("a").unwrap().unwrap().favorite);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn playlists_hold_an_order_and_release_deleted_tracks() {
        let dir = std::env::temp_dir().join(format!("aria-test-{}", uuid::Uuid::new_v4()));
        let lib = Library::open(&dir, &dir.join("audio")).unwrap();
        for id in ["a", "b", "c"] {
            lib.insert(&sample(id)).unwrap();
        }

        let pl = lib.create_playlist("Late night").unwrap();
        for id in ["a", "b", "c"] {
            lib.add_to_playlist(&pl.id, id).unwrap();
        }
        // Adding twice must not duplicate.
        lib.add_to_playlist(&pl.id, "b").unwrap();
        let ids = |l: &Library| l.playlists().unwrap()[0].track_ids.clone();
        assert_eq!(ids(&lib), vec!["a", "b", "c"]);

        lib.move_in_playlist(&pl.id, "c", -1).unwrap();
        assert_eq!(ids(&lib), vec!["a", "c", "b"]);
        // Moving past either end is a no-op, not an error or a wrap-around.
        lib.move_in_playlist(&pl.id, "a", -1).unwrap();
        assert_eq!(ids(&lib), vec!["a", "c", "b"]);

        lib.remove_from_playlist(&pl.id, "c").unwrap();
        assert_eq!(ids(&lib), vec!["a", "b"]);
        // Removing from a playlist is not a deletion.
        assert!(lib.get("c").unwrap().is_some());

        // Deleting the song itself does take it out of the playlist.
        lib.delete("a").unwrap();
        assert_eq!(ids(&lib), vec!["b"]);

        // And deleting the playlist leaves the music alone.
        lib.delete_playlist(&pl.id).unwrap();
        assert!(lib.playlists().unwrap().is_empty());
        assert!(lib.get("b").unwrap().is_some());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn lineage_survives_parent_deletion() {
        let dir = std::env::temp_dir().join(format!("aria-test-{}", uuid::Uuid::new_v4()));
        let lib = Library::open(&dir, &dir.join("audio")).unwrap();
        lib.insert(&sample("parent")).unwrap();

        let mut child = sample("child");
        child.parent_id = Some("parent".into());
        child.operation = Some("repaint".into());
        lib.insert(&child).unwrap();

        // Deleting a parent must not cascade away the derived track.
        lib.delete("parent").unwrap();
        let got = lib.get("child").unwrap().unwrap();
        assert_eq!(got.parent_id, None);
        assert_eq!(got.operation.as_deref(), Some("repaint"));

        std::fs::remove_dir_all(&dir).ok();
    }
}
