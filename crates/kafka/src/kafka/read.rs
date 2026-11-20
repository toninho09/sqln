//! Reads of topic messages: offsets, start positions, rows, and topic pages.

use super::*;

impl KafkaSession {
    /// Returns the partitions of a topic with their low and high offsets.
    pub(super) async fn offsets(
        &self,
        topic: &str,
        partition: Option<i32>,
    ) -> Result<Vec<(PartitionInfo, i64, i64)>, ExecError> {
        let info = self
            .metadata(Some(topic.to_owned()))
            .await?
            .into_iter()
            .find(|info| info.name == topic)
            .ok_or_else(|| ExecError::new(text::KAFKA_NO_TOPIC))?;
        if let Some(error) = info.error {
            return Err(ExecError::new(text::kafka_topic_error(&error)));
        }
        let mut partitions = info.partitions;
        partitions.sort_by_key(|partition| partition.id);
        if let Some(wanted) = partition {
            partitions.retain(|partition| partition.id == wanted);
            if partitions.is_empty() {
                return Err(ExecError::new(text::KAFKA_NO_PARTITION));
            }
        }
        let ids = partitions.iter().map(|partition| partition.id).collect();
        let marks = watermarks(&self.consumer, topic, ids).await?;
        Ok(partitions
            .into_iter()
            .zip(marks)
            .map(|(partition, (_, low, high))| (partition, low, high))
            .collect())
    }

    /// Returns the first offset of each partition for a start.
    pub(super) async fn starts(
        &self,
        topic: &str,
        start: Start,
        partitions: &[(PartitionInfo, i64, i64)],
    ) -> Result<Vec<i64>, ExecError> {
        let clamp = |offset: i64, low: i64, high: i64| offset.clamp(low, high);
        match start {
            Start::Earliest => Ok(partitions.iter().map(|(_, low, _)| *low).collect()),
            Start::Latest => Ok(partitions.iter().map(|(_, _, high)| *high).collect()),
            Start::Offset(offset) => Ok(partitions
                .iter()
                .map(|(_, low, high)| clamp(offset, *low, *high))
                .collect()),
            Start::Time(millis) => {
                let mut list = TopicPartitionList::new();
                for (partition, _, _) in partitions {
                    list.add_partition_offset(topic, partition.id, Offset::Offset(millis))
                        .map_err(|error| ExecError::new(crate::describe(&error)))?;
                }
                let found = client::blocking(&self.consumer, move |consumer| {
                    consumer.offsets_for_times(list, CALL_LIMIT).map(|list| {
                        list.elements()
                            .iter()
                            .map(|element| (element.partition(), element.offset()))
                            .collect::<Vec<_>>()
                    })
                })
                .await?;
                Ok(partitions
                    .iter()
                    .map(|(partition, low, high)| {
                        match found.iter().find(|(id, _)| *id == partition.id) {
                            Some((_, Offset::Offset(offset))) => clamp(*offset, *low, *high),
                            // No message at or after the time: the read starts at the end.
                            _ => *high,
                        }
                    })
                    .collect())
            }
        }
    }

    /// Decodes a message and checks the filter. It returns `None` for a message that does
    /// not match.
    pub(super) async fn message_row(
        &self,
        raw: Raw,
        filter: Option<Arc<Filter>>,
        projection: Option<Arc<command::Projection>>,
        evaluation: &filter::Evaluation,
        notices: &mut Notices,
    ) -> Result<Option<Row>, ExecError> {
        let registry = self.registry.as_deref();
        let (key, key_notice) = decode(raw.key.as_deref(), registry).await;
        let (value, value_notice) = decode(raw.value.as_deref(), registry).await;
        key_notice
            .into_iter()
            .chain(value_notice)
            .for_each(|notice| notices.add(notice));
        let expressions = filter.is_some() || projection.is_some();
        let cancel_on_drop = evaluation.cancel_on_drop();
        let evaluation = evaluation.clone();
        let build = move || -> Result<Option<Row>, ExecError> {
            evaluation.check()?;
            let meta = Meta {
                now: chrono::Utc::now().timestamp_millis(),
                key_bytes: raw.key.as_ref().map(Vec::len),
                value_bytes: raw.value.as_ref().map(Vec::len),
            };
            let headers_json = headers_json(&raw.headers);
            let headers = headers_json.as_ref().map(Json::to_string);
            let schema = match (key.schema(), value.schema()) {
                (None, None) => None,
                (Some(key), None) => Some(format!("key {key}")),
                (None, Some(value)) => Some(format!("value {value}")),
                (Some(key), Some(value)) => Some(format!("key {key}, value {value}")),
            };
            if let Some(filter) = filter {
                let needs = filter.needs();
                let key_json = needs.key.then(|| key.json()).flatten();
                let value_json = needs.value.then(|| value.json()).flatten();
                let message = Message {
                    partition: raw.partition.into(),
                    offset: raw.offset,
                    timestamp: raw.timestamp,
                    key: key.text(),
                    key_json: key_json.as_deref(),
                    value: value.text(),
                    json: value_json.as_deref(),
                    headers: headers.as_deref(),
                    headers_json: headers_json.as_ref(),
                    schema: schema.as_deref(),
                    meta,
                };
                if !filter.matches(&message, &evaluation)? {
                    return Ok(None);
                }
            }
            let mut row = RowWriter::default();
            row.push(Value::Number(&raw.partition.to_string()));
            row.push(Value::Number(&raw.offset.to_string()));
            match raw.timestamp.and_then(time_text) {
                Some(time) => row.push(Value::Text(&time)),
                None => row.push(Value::Null),
            }
            push_decoded(&mut row, &key);
            push_decoded(&mut row, &value);
            match &headers {
                Some(text) => row.push(Value::Text(text)),
                None => row.push(Value::Null),
            }
            match &schema {
                Some(text) => row.push(Value::Text(text)),
                None => row.push(Value::Null),
            }
            let row = row.finish();
            Ok(Some(match projection {
                Some(projection) => projection.apply(row, meta, &evaluation)?,
                None => row,
            }))
        };
        // The runner remains free to process Stop and the statement deadline.
        // The visitor checks the stop token even when it produces no values.
        let result = if expressions {
            tokio::task::spawn_blocking(build)
                .await
                .map_err(|_| ExecError::new(text::KAFKA_CALL_FAILED))
                .and_then(|result| result)
        } else {
            build()
        };
        cancel_on_drop.disarm();
        result
    }

    pub(super) fn registry_notice(&self, notices: &mut Notices) {
        if let Some(message) = &self.registry_error {
            notices.add(text::kafka_registry_unavailable(message));
        }
    }

    /// Runs `CONSUME` or `TAIL`. `CONSUME` ends at the last offset of each partition at its
    /// start; `TAIL` waits for new messages until the user stops it.
    pub(super) async fn read(
        &self,
        read: Read,
        tail: bool,
        sink: &mut ResultSink,
    ) -> Result<bool, ExecError> {
        let partitions = self.offsets(&read.topic, read.partition).await?;
        let starts = self.starts(&read.topic, read.start, &partitions).await?;
        let windows: Vec<Window> = partitions
            .iter()
            .zip(&starts)
            .map(|((partition, _, high), start)| Window {
                partition: partition.id,
                start: *start,
                end: (!tail).then_some(*high),
            })
            .collect();
        let progress = Arc::new(ReadProgress::new(read.filter.is_some()));
        sink.track(progress.clone());
        if let Some(projection) = &read.projection {
            sink.set_columns(projection.names()).await;
        } else {
            set_columns(&MESSAGE_COLUMNS, sink).await;
        }
        let mut notices = Notices::default();
        self.registry_notice(&mut notices);
        let stop = sink.stop_token();
        let evaluation = filter::Evaluation::new(stop.clone(), sink.memory_limit());
        let filter = read.filter.map(Arc::new);
        let projection = read.projection.map(Arc::new);
        let mut scan = Scan::start(&self.target, &read.topic, &windows, !tail)?;
        let highs = partitions
            .iter()
            .map(|(partition, _, high)| (partition.id, *high));
        let mut position = Position::new(&windows, highs);
        let mut lag = tail.then(|| {
            let ids = windows.iter().map(|window| window.partition).collect();
            LagCheck::new(self.consumer.clone(), &read.topic, ids)
        });
        let mut lag_known = true;
        let limit = read.limit.map(|limit| limit as u64);
        // The read ends at the first of its ends: the high offsets, `SCAN`, or `LIMIT`.
        let read_percent = |position: &Position| {
            [
                percent(position.done(), position.total()),
                read.scan.and_then(|scan| percent(progress.scanned(), scan)),
                limit.and_then(|limit| percent(progress.matched(), limit)),
            ]
            .into_iter()
            .flatten()
            .max()
        };
        if !tail {
            progress.set_percent(read_percent(&position));
        }
        let mut end = ReadEnd::None;
        loop {
            match scan.next(&stop).await? {
                Next::Message(raw) => {
                    progress.add_scanned(raw.size());
                    position.advance(raw.partition, raw.offset);
                    let row = self
                        .message_row(
                            raw,
                            filter.clone(),
                            projection.clone(),
                            &evaluation,
                            &mut notices,
                        )
                        .await;
                    if evaluation.stopped() {
                        end = ReadEnd::Stopped;
                        break;
                    }
                    if let Some(row) = row? {
                        progress.add_matched();
                        if sink.push(row).await != Flow::Continue {
                            if stop.is_cancelled() {
                                end = ReadEnd::Stopped;
                            }
                            break;
                        }
                        if limit.is_some_and(|limit| progress.matched() >= limit) {
                            end = ReadEnd::Limit;
                            break;
                        }
                    }
                    if read.scan.is_some_and(|scan| progress.scanned() >= scan) {
                        end = ReadEnd::Scan;
                        break;
                    }
                    if !tail {
                        progress.set_percent(read_percent(&position));
                    }
                }
                Next::Idle => sink.flush().await,
                Next::Done => {
                    end = ReadEnd::End;
                    break;
                }
                Next::Stopped => {
                    end = ReadEnd::Stopped;
                    break;
                }
            }
            if let Some(check) = &mut lag {
                if let Some(highs) = check.poll().await {
                    lag_known = highs.is_some();
                    position.set_ends(highs.unwrap_or_default());
                }
                progress.set_lag(lag_known.then(|| position.left()));
            }
        }
        drop(scan);
        progress.set_end(end);
        let stopped = end == ReadEnd::Stopped;
        if filter.is_some() || read.scan.is_some() || stopped {
            notices.add(text::kafka_scanned(
                progress.scanned(),
                progress.matched(),
                stopped,
            ));
        }
        notices.send(sink).await;
        Ok(true)
    }

    /// Reads the last messages of a topic that match the filter, newest first. Each round
    /// reads an older window of each partition that can still hold a newer match.
    pub(super) async fn topic_page(
        &self,
        topic: &str,
        count: usize,
        filter: Option<Filter>,
        sink: &mut ResultSink,
    ) -> Result<bool, ExecError> {
        let progress = Arc::new(ReadProgress::new(filter.is_some()));
        sink.track(progress.clone());
        set_columns(&MESSAGE_COLUMNS, sink).await;
        if count == 0 {
            return Ok(true);
        }
        let partitions = self.offsets(topic, None).await?;
        let mut notices = Notices::default();
        self.registry_notice(&mut notices);
        let stop = sink.stop_token();
        let evaluation = filter::Evaluation::new(stop.clone(), sink.memory_limit());
        let filter = filter.map(Arc::new);
        let mut ends: HashMap<i32, i64> = partitions
            .iter()
            .map(|(p, _, high)| (p.id, *high))
            .collect();
        let lows: HashMap<i32, i64> = partitions.iter().map(|(p, low, _)| (p.id, *low)).collect();
        let mut sizes: HashMap<i32, i64> = lows.keys().map(|id| (*id, FIRST_WINDOW)).collect();
        let mut oldest: HashMap<i32, i64> = lows.keys().map(|id| (*id, i64::MAX)).collect();
        let mut found: Vec<Found> = Vec::new();
        let mut stopped = false;
        loop {
            keep_newest(&mut found, count);
            let threshold = (found.len() >= count).then(|| found[found.len() - 1].0);
            let windows: Vec<Window> = partitions
                .iter()
                .map(|(partition, _, _)| partition.id)
                .filter(|id| ends[id] > lows[id])
                .filter(|id| threshold.is_none_or(|time| oldest[id] >= time))
                .map(|id| Window {
                    partition: id,
                    start: (ends[&id] - sizes[&id]).max(lows[&id]),
                    end: Some(ends[&id]),
                })
                .collect();
            if windows.is_empty() || stopped {
                break;
            }
            let mut scan = Scan::start(&self.target, topic, &windows, true)?;
            loop {
                match scan.next(&stop).await? {
                    Next::Message(raw) => {
                        progress.add_scanned(raw.size());
                        let time = raw.timestamp.unwrap_or(-1);
                        let (partition, offset) = (raw.partition, raw.offset);
                        oldest
                            .entry(raw.partition)
                            .and_modify(|oldest| *oldest = (*oldest).min(time));
                        let row = self
                            .message_row(raw, filter.clone(), None, &evaluation, &mut notices)
                            .await;
                        if evaluation.stopped() {
                            stopped = true;
                            break;
                        }
                        if let Some(row) = row? {
                            progress.add_matched();
                            found.push((time, partition, offset, row));
                            // A window can hold thousands of matches, and these rows are
                            // outside the memory budget until the page ends. Only the
                            // newest `count` can reach the page.
                            if found.len() >= count.saturating_mul(2) {
                                keep_newest(&mut found, count);
                            }
                        }
                    }
                    Next::Idle => {}
                    Next::Done => break,
                    Next::Stopped => {
                        stopped = true;
                        break;
                    }
                }
            }
            drop(scan);
            for window in &windows {
                ends.insert(window.partition, window.start);
                sizes
                    .entry(window.partition)
                    .and_modify(|size| *size = (*size * 2).min(LARGEST_WINDOW));
            }
        }
        keep_newest(&mut found, count);
        push_rows(found.into_iter().map(|(.., row)| row).collect(), sink).await;
        if filter.is_some() || stopped {
            notices.add(text::kafka_scanned(
                progress.scanned(),
                progress.matched(),
                stopped,
            ));
        }
        notices.send(sink).await;
        Ok(true)
    }
}

/// A message of a topic page: its time, partition, offset, and row.
type Found = (i64, i32, i64, Row);

/// Keeps the newest `count` messages, newest first.
fn keep_newest(found: &mut Vec<Found>, count: usize) {
    found.sort_by_key(|item| std::cmp::Reverse((item.0, item.1, item.2)));
    found.truncate(count);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(time: i64, partition: i32, offset: i64) -> Found {
        (time, partition, offset, Row::default())
    }

    fn keys(found: &[Found]) -> Vec<(i64, i32, i64)> {
        found.iter().map(|item| (item.0, item.1, item.2)).collect()
    }

    #[test]
    fn a_page_keeps_the_newest_messages_whatever_the_order_of_the_reads() {
        let mut all = vec![
            found(5, 0, 1),
            found(9, 1, 7),
            found(-1, 0, 0),
            found(9, 0, 3),
        ];
        keep_newest(&mut all, 2);
        assert_eq!(keys(&all), [(9, 1, 7), (9, 0, 3)]);

        // Truncation in the middle of a read keeps the same final page.
        let mut step = vec![found(1, 0, 1), found(2, 0, 2)];
        keep_newest(&mut step, 1);
        step.push(found(3, 0, 3));
        keep_newest(&mut step, 1);
        assert_eq!(keys(&step), [(3, 0, 3)]);
    }
}
