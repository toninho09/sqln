//! The schema context of AI completions: its reads, its cache, and its authorization.

use super::*;

impl App {
    pub(in crate::app) fn forget_ai_schema(&mut self, id: ConnectionId) {
        if let Some(cache) = self.ai.caches.remove(&id) {
            cache.cancel.cancel();
        }
    }

    pub(in crate::app) fn invalidate_ai_schema(&mut self, id: ConnectionId) {
        let ids: Vec<u64> = self
            .ai
            .runs
            .iter()
            .filter(|r| r.connection == id)
            .map(|r| r.id)
            .collect();
        for run in ids {
            self.cancel_ai_id(run);
        }
        if let Some(cache) = self.ai.caches.get_mut(&id) {
            cache.cancel.cancel();
            cache.snapshot = None;
            cache.loading = false;
        }
    }

    pub(super) fn ai_fingerprint(&self, id: ConnectionId) -> Option<(u64, u64, Option<u64>)> {
        let c = self.connection(id)?;
        Some((
            c.catalog_read,
            c.generation,
            self.manager.as_ref().and_then(|m| m.pool_serial(id)),
        ))
    }

    pub(in crate::app) fn note_ai_catalog_read(&mut self, id: ConnectionId) {
        if let Some(fingerprint) = self.ai_fingerprint(id)
            && let Some(cache) = self.ai.caches.get_mut(&id)
        {
            cache.fingerprint = fingerprint;
        }
    }

    pub(in crate::app) fn refresh_ai_schema(&mut self, id: ConnectionId) {
        if self.ai.preferences.enabled() && self.ai.caches.get(&id).is_some_and(|c| c.warmed) {
            self.ensure_ai_schema(id, None);
            if let Some(cache) = self.ai.caches.get_mut(&id) {
                cache.background = true;
            }
        }
    }

    /// Returns the generation of the snapshot of a connection, and starts a read when the
    /// cache is old or lacks the fields of `topic`. A name that the catalog does not
    /// know, such as a topic name that the user still types, reads no fields.
    pub(super) fn ensure_ai_schema(&mut self, id: ConnectionId, topic: Option<&str>) -> u64 {
        let fingerprint = self.ai_fingerprint(id).unwrap_or_default();
        let topic = topic.filter(|topic| {
            self.connection(id)
                .and_then(|c| c.catalog.as_ref())
                .is_none_or(|catalog| {
                    catalog
                        .relation(Some(crate::connector::kafka::TOPICS), topic)
                        .is_some()
                })
        });
        if let Some(cache) = self.ai.caches.get(&id)
            && cache.fingerprint == fingerprint
            && (cache.loading || cache.snapshot.is_some())
            && topic.is_none_or(|topic| cache.topics.iter().any(|known| known == topic))
        {
            return cache.generation;
        }
        self.ai.serial += 1;
        let generation = self.ai.serial;
        let warmed = self.ai.caches.get(&id).is_some_and(|c| c.warmed);
        let mut topics = Vec::new();
        if let Some(old) = self.ai.caches.remove(&id) {
            old.cancel.cancel();
            topics = old.topics;
        }
        if let Some(topic) = topic {
            topics.retain(|known| known != topic);
            topics.push(topic.to_owned());
            if topics.len() > FIELD_TOPICS {
                topics.remove(0);
            }
        }
        self.ai.caches.insert(
            id,
            Cache {
                generation,
                fingerprint,
                topics,
                snapshot: None,
                cancel: CancellationToken::new(),
                loading: true,
                warmed,
                background: false,
            },
        );
        self.authorize(id, crate::app::auth::Use::AiSchema(id, generation));
        generation
    }

    pub(in crate::app) fn dispatch_ai_schema(&mut self, id: ConnectionId, generation: u64) {
        if let Some(fingerprint) = self.ai_fingerprint(id)
            && let Some(cache) = self.ai.caches.get_mut(&id)
        {
            cache.fingerprint = fingerprint;
        }
        let Some(cache) = self
            .ai
            .caches
            .get(&id)
            .filter(|c| c.generation == generation && !c.cancel.is_cancelled())
        else {
            return;
        };
        let schemas = self
            .connection(id)
            .and_then(|c| c.definition.as_ref())
            .map(|d| d.schemas.clone())
            .unwrap_or_default();
        if let Some(manager) = &self.manager {
            manager.load_ai_schema(
                id,
                generation,
                schemas,
                cache.topics.clone(),
                cache.cancel.clone(),
                self.ai.schema_sender.clone(),
            );
        }
    }

    pub(in crate::app) fn cancel_ai_schema_auth(&mut self, id: ConnectionId, generation: u64) {
        if let Some(c) = self
            .ai
            .caches
            .get_mut(&id)
            .filter(|c| c.generation == generation)
        {
            c.loading = false;
            c.cancel.cancel();
        }
        let dead: Vec<u64> = self
            .ai
            .runs
            .iter()
            .filter(|r| r.connection == id && r.generation == generation)
            .map(|r| r.id)
            .collect();
        if !dead.is_empty() {
            for run in dead {
                self.cancel_ai_id(run);
            }
            self.status=Some("Schema authorization cancelled. Request completion again when access is available.".into());
        }
    }
}
