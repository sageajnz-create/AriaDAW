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
            if let Some(l) = &t.latent_path {
                let _ = std::fs::remove_file(l);
            }
        }
        self.conn.execute("DELETE FROM tracks WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn count(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))?)
    }
}

fn row_to_track(r: &rusqlite::Row) -> rusqlite::Result<Track> {
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
        audio_path: r.get(12)?,
        latent_path: r.get(13)?,
        created_at: r.get(14)?,
        parent_id: r.get(15)?,
        operation: r.get(16)?,
        favorite: r.get::<_, i32>(17)? != 0,
        lyricist: r.get(18)?,
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
