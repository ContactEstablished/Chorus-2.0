//! Providers, their model catalogs and shortlists, and the ledger of capped keys (migration 0003).
//! Keys stay in `credentials`; a provider only points at them.

use chorus_core::providers::{CatalogModel, ProviderKind};
use rusqlite::{OptionalExtension, Row, params};

use super::{Store, now_ms};

#[derive(Debug, Clone)]
pub struct ProviderRow {
    pub id: u32,
    pub kind: ProviderKind,
    pub name: String,
    pub base_url: Option<String>,
    pub credential_id: Option<u32>,
    pub management_credential_id: Option<u32>,
    pub cap_usd: Option<f64>,
    pub tested_at: Option<i64>,
    pub test_ok: Option<bool>,
    pub test_note: Option<String>,
    pub catalog_refreshed_at: Option<i64>,
}

/// What the user sets on a provider.
pub struct ProviderInput<'a> {
    pub kind: ProviderKind,
    pub name: &'a str,
    pub base_url: Option<&'a str>,
    pub credential_id: Option<u32>,
    pub management_credential_id: Option<u32>,
    pub cap_usd: Option<f64>,
}

const COLUMNS: &str = "id, kind, name, base_url, credential_id, management_credential_id, cap_usd, tested_at, test_ok, \
                       test_note, catalog_refreshed_at";

fn provider_from(r: &Row<'_>) -> rusqlite::Result<ProviderRow> {
    let kind: String = r.get(1)?;
    Ok(ProviderRow {
        id: r.get(0)?,
        kind: ProviderKind::parse(&kind).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, format!("unknown provider kind {kind:?}").into())
        })?,
        name: r.get(2)?,
        base_url: r.get(3)?,
        credential_id: r.get(4)?,
        management_credential_id: r.get(5)?,
        cap_usd: r.get(6)?,
        tested_at: r.get(7)?,
        test_ok: r.get(8)?,
        test_note: r.get(9)?,
        catalog_refreshed_at: r.get(10)?,
    })
}

fn unique(e: String, name: &str) -> String {
    if e.contains("UNIQUE") { format!("a provider named {name:?} already exists") } else { e }
}

pub fn insert(store: &Store, p: &ProviderInput<'_>) -> Result<u32, String> {
    store
        .with(|c| {
            c.execute(
                "INSERT INTO providers (kind, name, base_url, credential_id, management_credential_id, cap_usd, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![p.kind.as_str(), p.name, p.base_url, p.credential_id, p.management_credential_id, p.cap_usd, now_ms()],
            )?;
            Ok(c.last_insert_rowid() as u32)
        })
        .map_err(|e| unique(e, p.name))
}

/// Changing the keys or address clears the last test, which no longer says anything.
pub fn update(store: &Store, id: u32, p: &ProviderInput<'_>) -> Result<(), String> {
    store
        .with(|c| {
            c.execute(
                "UPDATE providers SET kind = ?2, name = ?3, base_url = ?4, credential_id = ?5, management_credential_id = ?6,
                        cap_usd = ?7, tested_at = NULL, test_ok = NULL, test_note = NULL
                 WHERE id = ?1",
                params![id, p.kind.as_str(), p.name, p.base_url, p.credential_id, p.management_credential_id, p.cap_usd],
            )
            .map(|_| ())
        })
        .map_err(|e| unique(e, p.name))
}

pub fn get(store: &Store, id: u32) -> Result<Option<ProviderRow>, String> {
    store.with(|c| c.query_row(&format!("SELECT {COLUMNS} FROM providers WHERE id = ?1"), [id], provider_from).optional())
}

pub fn list(store: &Store) -> Result<Vec<ProviderRow>, String> {
    store.with(|c| {
        let mut stmt = c.prepare(&format!("SELECT {COLUMNS} FROM providers ORDER BY name"))?;
        stmt.query_map([], provider_from)?.collect()
    })
}

/// Sessions on it keep running; their next launch says the provider is gone.
pub fn delete(store: &Store, id: u32) -> Result<bool, String> {
    store.with(|c| c.execute("DELETE FROM providers WHERE id = ?1", [id]).map(|n| n > 0))
}

pub fn set_test(store: &Store, id: u32, ok: bool, note: &str) -> Result<(), String> {
    store.with(|c| {
        c.execute("UPDATE providers SET tested_at = ?2, test_ok = ?3, test_note = ?4 WHERE id = ?1", params![id, now_ms(), ok, note])
            .map(|_| ())
    })
}

/// Records a fresh model list: new and returning models are stored, and models it no longer has
/// are marked missing, not deleted (a shortlisted model may come back). Returns how many it has.
pub fn replace_catalog(store: &Store, provider_id: u32, models: &[CatalogModel]) -> Result<usize, String> {
    let now = now_ms();
    store.with(|c| {
        let tx = c.unchecked_transaction()?;
        tx.execute("UPDATE model_catalog SET missing_since = COALESCE(missing_since, ?2) WHERE provider_id = ?1", params![provider_id, now])?;
        for m in models {
            let data = serde_json::to_string(m).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
            tx.execute(
                "INSERT INTO model_catalog (provider_id, model_id, data, first_seen_at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (provider_id, model_id) DO UPDATE SET data = excluded.data, missing_since = NULL",
                params![provider_id, m.id, data, now],
            )?;
        }
        tx.execute("UPDATE providers SET catalog_refreshed_at = ?2 WHERE id = ?1", params![provider_id, now])?;
        tx.commit()?;
        Ok(models.len())
    })
}

#[derive(Debug, Clone)]
pub struct CatalogRow {
    pub model: CatalogModel,
    pub missing_since: Option<i64>,
}

pub fn catalog(store: &Store, provider_id: u32) -> Result<Vec<CatalogRow>, String> {
    store.with(|c| {
        let mut stmt = c.prepare("SELECT data, missing_since FROM model_catalog WHERE provider_id = ?1 ORDER BY model_id")?;
        let rows = stmt.query_map([provider_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?)))?;
        Ok(rows
            .flatten()
            .filter_map(|(data, missing_since)| Some(CatalogRow { model: serde_json::from_str(&data).ok()?, missing_since }))
            .collect())
    })
}

pub fn catalog_model(store: &Store, provider_id: u32, model_id: &str) -> Result<Option<CatalogModel>, String> {
    let data: Option<String> = store.with(|c| {
        c.query_row(
            "SELECT data FROM model_catalog WHERE provider_id = ?1 AND model_id = ?2",
            params![provider_id, model_id],
            |r| r.get(0),
        )
        .optional()
    })?;
    Ok(data.and_then(|d| serde_json::from_str(&d).ok()))
}

pub fn shortlist(store: &Store, provider_id: u32) -> Result<Vec<String>, String> {
    store.with(|c| {
        let mut stmt = c.prepare("SELECT model_id FROM shortlist WHERE provider_id = ?1 ORDER BY added_at, model_id")?;
        stmt.query_map([provider_id], |r| r.get(0))?.collect()
    })
}

/// Replaces the shortlist, keeping when each model that stays was first added.
pub fn set_shortlist(store: &Store, provider_id: u32, model_ids: &[String]) -> Result<(), String> {
    let now = now_ms();
    store.with(|c| {
        let tx = c.unchecked_transaction()?;
        let keep = serde_json::to_string(model_ids).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        tx.execute(
            "DELETE FROM shortlist WHERE provider_id = ?1 AND model_id NOT IN (SELECT value FROM json_each(?2))",
            params![provider_id, keep],
        )?;
        for id in model_ids {
            tx.execute(
                "INSERT INTO shortlist (provider_id, model_id, added_at) VALUES (?1, ?2, ?3) ON CONFLICT DO NOTHING",
                params![provider_id, id, now],
            )?;
        }
        tx.commit()
    })
}

/// A ledger row for a key about to be minted; its id names the key.
pub fn open_capped_key(store: &Store, provider_id: u32, session_id: u32, limit_usd: f64) -> Result<u32, String> {
    store.with(|c| {
        c.execute(
            "INSERT INTO capped_keys (provider_id, session_id, limit_usd, minted_at) VALUES (?1, ?2, ?3, ?4)",
            params![provider_id, session_id, limit_usd, now_ms()],
        )?;
        Ok(c.last_insert_rowid() as u32)
    })
}

pub fn set_capped_hash(store: &Store, id: u32, hash: &str) -> Result<(), String> {
    store.with(|c| c.execute("UPDATE capped_keys SET hash = ?2 WHERE id = ?1", params![id, hash]).map(|_| ()))
}

pub fn set_capped_run(store: &Store, id: u32, run_id: u32) -> Result<(), String> {
    store.with(|c| c.execute("UPDATE capped_keys SET run_id = ?2 WHERE id = ?1", params![id, run_id]).map(|_| ()))
}

/// The key is gone at OpenRouter (or was never minted), with why when there is something to say.
pub fn close_capped_key(store: &Store, id: u32, note: Option<&str>) -> Result<(), String> {
    store.with(|c| {
        c.execute(
            "UPDATE capped_keys SET revoked_at = COALESCE(revoked_at, ?2), cost_note = COALESCE(?3, cost_note) WHERE id = ?1",
            params![id, now_ms(), note],
        )
        .map(|_| ())
    })
}

pub fn set_capped_cost(store: &Store, id: u32, cost_usd: Option<f64>, note: &str) -> Result<(), String> {
    store.with(|c| {
        c.execute("UPDATE capped_keys SET cost_usd = ?2, cost_note = ?3 WHERE id = ?1", params![id, cost_usd, note]).map(|_| ())
    })
}

#[derive(Debug, Clone)]
pub struct CappedKeyRow {
    pub id: u32,
    pub provider_id: Option<u32>,
    #[cfg_attr(not(test), allow(dead_code, reason = "read by tests"))]
    pub run_id: Option<u32>,
    pub hash: Option<String>,
    pub limit_usd: f64,
    pub minted_at: i64,
    pub revoked_at: Option<i64>,
    pub cost_usd: Option<f64>,
    pub cost_note: Option<String>,
}

fn capped_from(r: &Row<'_>) -> rusqlite::Result<CappedKeyRow> {
    Ok(CappedKeyRow {
        id: r.get(0)?,
        provider_id: r.get(1)?,
        run_id: r.get(2)?,
        hash: r.get(3)?,
        limit_usd: r.get(4)?,
        minted_at: r.get(5)?,
        revoked_at: r.get(6)?,
        cost_usd: r.get(7)?,
        cost_note: r.get(8)?,
    })
}

const CAPPED_COLUMNS: &str = "id, provider_id, run_id, hash, limit_usd, minted_at, revoked_at, cost_usd, cost_note";

pub fn capped_key(store: &Store, id: u32) -> Result<Option<CappedKeyRow>, String> {
    store.with(|c| c.query_row(&format!("SELECT {CAPPED_COLUMNS} FROM capped_keys WHERE id = ?1"), [id], capped_from).optional())
}

/// Keys that may still be live: minted, and not revoked.
pub fn open_capped_keys(store: &Store) -> Result<Vec<CappedKeyRow>, String> {
    store.with(|c| {
        let mut stmt = c.prepare(&format!("SELECT {CAPPED_COLUMNS} FROM capped_keys WHERE revoked_at IS NULL ORDER BY id"))?;
        stmt.query_map([], capped_from)?.collect()
    })
}

/// The newest capped key of a session's runs, for its summary.
pub fn last_capped_key(store: &Store, session_id: u32) -> Result<Option<CappedKeyRow>, String> {
    store.with(|c| {
        c.query_row(
            &format!("SELECT {CAPPED_COLUMNS} FROM capped_keys WHERE session_id = ?1 ORDER BY id DESC LIMIT 1"),
            [session_id],
            capped_from,
        )
        .optional()
    })
}

pub fn max_capped_id(store: &Store) -> Result<u32, String> {
    store.with(|c| c.query_row("SELECT COALESCE(MAX(id), 0) FROM capped_keys", [], |r| r.get(0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(_name: &str) -> Store {
        Store::in_memory()
    }

    fn model(id: &str) -> CatalogModel {
        CatalogModel {
            id: id.into(),
            name: id.into(),
            prompt_per_mtok: Some(0.0),
            completion_per_mtok: Some(0.0),
            context_length: None,
            max_output: None,
            efforts: vec![],
            tools: Some(true),
            free: true,
        }
    }

    #[test]
    fn providers_catalogs_and_shortlists_are_kept() {
        let s = store("catalog");
        let input = ProviderInput {
            kind: ProviderKind::Openrouter,
            name: "OpenRouter",
            base_url: None,
            credential_id: None,
            management_credential_id: None,
            cap_usd: Some(1.0),
        };
        let id = insert(&s, &input).unwrap();
        assert!(insert(&s, &input).unwrap_err().contains("already exists"));
        set_test(&s, id, true, "ok").unwrap();
        assert_eq!(get(&s, id).unwrap().unwrap().test_ok, Some(true));
        update(&s, id, &input).unwrap();
        assert_eq!(get(&s, id).unwrap().unwrap().test_ok, None, "a change clears the test");

        replace_catalog(&s, id, &[model("a/x:free"), model("b/y")]).unwrap();
        set_shortlist(&s, id, &["b/y".into(), "c/z".into()]).unwrap();
        replace_catalog(&s, id, &[model("a/x:free")]).unwrap();
        let rows = catalog(&s, id).unwrap();
        assert_eq!(rows.len(), 2, "a model that disappears is kept");
        assert!(rows.iter().find(|r| r.model.id == "b/y").unwrap().missing_since.is_some());
        assert!(rows.iter().find(|r| r.model.id == "a/x:free").unwrap().missing_since.is_none());
        assert_eq!(shortlist(&s, id).unwrap(), ["b/y", "c/z"]);
        set_shortlist(&s, id, &["c/z".into()]).unwrap();
        assert_eq!(shortlist(&s, id).unwrap(), ["c/z"]);
        assert_eq!(catalog_model(&s, id, "a/x:free").unwrap().unwrap().id, "a/x:free");

        assert!(delete(&s, id).unwrap());
        assert!(catalog(&s, id).unwrap().is_empty() && shortlist(&s, id).unwrap().is_empty());
    }

    #[test]
    fn the_capped_key_ledger_tracks_each_key() {
        let s = store("ledger");
        let k = open_capped_key(&s, 1, 7, 1.0).unwrap();
        set_capped_hash(&s, k, "abcd").unwrap();
        set_capped_run(&s, k, 3).unwrap();
        assert_eq!(open_capped_keys(&s).unwrap().len(), 1);
        close_capped_key(&s, k, None).unwrap();
        assert!(open_capped_keys(&s).unwrap().is_empty());
        set_capped_cost(&s, k, Some(0.01), "settled").unwrap();
        let row = last_capped_key(&s, 7).unwrap().unwrap();
        assert_eq!((row.hash.as_deref(), row.run_id, row.cost_usd), (Some("abcd"), Some(3), Some(0.01)));
        assert_eq!(max_capped_id(&s).unwrap(), k);
    }
}
