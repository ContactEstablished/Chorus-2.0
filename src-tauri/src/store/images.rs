//! Images belong to either the saved draft or one immutable queued submission.
use super::Store;
use chorus_core::attachments::{MAX_IMAGES, MAX_TOTAL_BYTES};
use rusqlite::params;
use serde::Serialize;
use specta::Type;

#[derive(Debug, Clone, Serialize, Type)]
pub struct ComposerImage {
    pub id: u32,
    pub name: String,
    pub bytes: u32,
}

pub fn list(store: &Store, session: u32, queue: Option<u32>) -> Result<Vec<ComposerImage>, String> {
    store.with(|c| {
        c.prepare("SELECT id,name,length(png) FROM composer_images WHERE session_id=?1 AND queue_id IS ?2 ORDER BY id")?
            .query_map(params![session, queue], |r| Ok(ComposerImage { id: r.get(0)?, name: r.get(1)?, bytes: r.get(2)? }))?
            .collect()
    })
}
pub fn bytes(store: &Store, session: u32, id: u32) -> Result<Vec<u8>, String> {
    store.with(|c| c.query_row("SELECT png FROM composer_images WHERE session_id=?1 AND id=?2", params![session, id], |r| r.get(0)))
}
pub fn add(store: &Store, session: u32, revision: u32, name: &str, png: &[u8]) -> Result<(), String> {
    chorus_core::attachments::validate_png(png)?;
    store.with(|c| {
        let tx = c.unchecked_transaction()?;
        let (count, bytes): (u32, u32) = tx.query_row(
            "SELECT count(*),COALESCE(sum(length(png)),0) FROM composer_images WHERE session_id=?1 AND queue_id IS NULL",
            [session],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if count as usize >= MAX_IMAGES || bytes as usize + png.len() > MAX_TOTAL_BYTES {
            return Err(rusqlite::Error::InvalidParameterName("Maximum 8 images and 20 MiB per message.".into()));
        }
        tx.execute("INSERT OR IGNORE INTO composer_drafts(session_id) VALUES (?1)", [session])?;
        if tx.execute("UPDATE composer_drafts SET revision=revision+1 WHERE session_id=?1 AND revision=?2", params![session, revision])?
            != 1
        {
            return Err(rusqlite::Error::InvalidParameterName("Draft changed. Reload before attaching an image.".into()));
        }
        tx.execute("INSERT INTO composer_images(session_id,name,png) VALUES (?1,?2,?3)", params![session, name, png])?;
        tx.commit()
    })
}
pub fn remove(store: &Store, session: u32, revision: u32, id: u32) -> Result<(), String> {
    store.with(|c| {
        let tx = c.unchecked_transaction()?;
        if tx.execute("UPDATE composer_drafts SET revision=revision+1 WHERE session_id=?1 AND revision=?2", params![session, revision])?
            != 1
        {
            return Err(rusqlite::Error::InvalidParameterName("Draft changed. Reload before removing an image.".into()));
        }
        if tx.execute("DELETE FROM composer_images WHERE session_id=?1 AND id=?2 AND queue_id IS NULL", params![session, id])? != 1 {
            return Err(rusqlite::Error::InvalidParameterName("Only an image in this draft can be removed.".into()));
        }
        tx.commit()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn revisions_ownership_and_queue_snapshot_are_atomic() {
        let s = Store::in_memory();
        s.with(|c| c.execute_batch("INSERT INTO sessions(id,name,agent,cwd,state,cols,rows,created_at) VALUES(1,'test','codex','scratch','running',100,30,0),(2,'other','codex','scratch','running',100,30,0); INSERT INTO runs(id,session_id,program,args,started_at) VALUES(1,1,'test','[]',0);")).unwrap();
        let png = include_bytes!("../../icons/32x32.png");
        add(&s, 1, 0, "test.png", png).unwrap();
        let image = list(&s, 1, None).unwrap().remove(0);
        assert!(bytes(&s, 2, image.id).is_err());
        assert!(remove(&s, 1, 0, image.id).is_err());
        assert!(add(&s, 1, 0, "stale.png", png).is_err());
        let draft = super::super::composer::save(&s, 1, 1, "look").unwrap();
        assert_eq!(draft.images.len(), 1);
        let q = super::super::composer::enqueue(&s, 1, 1, "request", 2, "look", true).unwrap();
        assert!(list(&s, 1, None).unwrap().is_empty());
        assert_eq!(list(&s, 1, Some(q)).unwrap()[0].id, image.id);
        assert!(remove(&s, 1, 3, image.id).is_err());
        assert_eq!(super::super::composer::draft(&s, 1).unwrap().revision, 3);
        super::super::composer::recover(&s).unwrap();
        assert_eq!(super::super::composer::oldest(&s, 1).unwrap().unwrap().images.len(), 1);
        assert_eq!(bytes(&s, 1, image.id).unwrap(), png);
    }
}
