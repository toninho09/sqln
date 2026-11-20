//! The explorer of a Kafka cluster: topics, partitions, and consumer groups.

use super::*;

impl KafkaSession {
    pub(super) async fn topics(&self, sink: &mut ResultSink) -> Result<bool, ExecError> {
        let mut topics = self.metadata(None).await?;
        topics.sort_by(|a, b| a.name.cmp(&b.name));
        set_columns(&["topic", "partitions", "replicas", "internal"], sink).await;
        let rows = topics
            .iter()
            .map(|topic| {
                let replicas = topic.partitions.first().map_or(0, |p| p.replicas.len());
                number_row(&[
                    Some(topic.name.clone()),
                    Some(topic.partitions.len().to_string()),
                    Some(replicas.to_string()),
                    Some(is_internal(&topic.name).to_string()),
                ])
            })
            .collect();
        push_rows(rows, sink).await;
        Ok(true)
    }

    pub(super) async fn partitions(
        &self,
        topic: &str,
        sink: &mut ResultSink,
    ) -> Result<bool, ExecError> {
        let partitions = self.offsets(topic, None).await?;
        set_columns(
            &[
                "partition",
                "leader",
                "replicas",
                "isr",
                "low",
                "high",
                "messages",
            ],
            sink,
        )
        .await;
        let rows = partitions
            .iter()
            .map(|(partition, low, high)| {
                number_row(&[
                    Some(partition.id.to_string()),
                    Some(partition.leader.to_string()),
                    Some(list_text(&partition.replicas)),
                    Some(list_text(&partition.isr)),
                    Some(low.to_string()),
                    Some(high.to_string()),
                    Some((high - low).to_string()),
                ])
            })
            .collect();
        push_rows(rows, sink).await;
        Ok(true)
    }

    pub(super) async fn group_list(
        &self,
        group: Option<String>,
    ) -> Result<Vec<GroupInfo>, ExecError> {
        client::blocking(&self.consumer, move |consumer| {
            consumer
                .fetch_group_list(group.as_deref(), CALL_LIMIT)
                .map(|list| groups_of(&list))
        })
        .await
    }

    pub(super) async fn groups(&self, sink: &mut ResultSink) -> Result<bool, ExecError> {
        let mut groups = self.group_list(None).await?;
        groups.sort_by(|a, b| a.name.cmp(&b.name));
        set_columns(&["group", "state", "protocol", "members"], sink).await;
        let rows = groups
            .iter()
            .map(|group| {
                number_row(&[
                    Some(group.name.clone()),
                    Some(group.state.clone()),
                    Some(group.protocol.clone()).filter(|text| !text.is_empty()),
                    Some(group.members.len().to_string()),
                ])
            })
            .collect();
        push_rows(rows, sink).await;
        Ok(true)
    }

    /// Reads the committed offsets of a group and its lag. A consumer with the id of the
    /// group asks for the offsets only: it never joins the group.
    pub(super) async fn group(&self, id: &str, sink: &mut ResultSink) -> Result<bool, ExecError> {
        let members = self
            .group_list(Some(id.to_owned()))
            .await?
            .into_iter()
            .find(|group| group.name == id)
            .map(|group| group.members)
            .unwrap_or_default();
        let topics = self.metadata(None).await?;
        let mut list = TopicPartitionList::new();
        for topic in topics.iter().filter(|topic| !is_internal(&topic.name)) {
            for partition in &topic.partitions {
                list.add_partition(&topic.name, partition.id);
            }
        }
        let config =
            client::config(&self.target, id).map_err(|error| ExecError::new(error.message))?;
        let committed: Vec<(String, i32, i64, i64)> = tokio::task::spawn_blocking(move || {
            let consumer =
                client::consumer(&config).map_err(|error| ExecError::new(error.message))?;
            let found = consumer
                .committed_offsets(list, CALL_LIMIT)
                .map_err(|error| ExecError::new(client::call_error(&consumer, &error).1))?;
            let mut rows = Vec::new();
            for element in found.elements() {
                let Offset::Offset(offset) = element.offset() else {
                    continue;
                };
                let (_, high) = consumer
                    .fetch_watermarks(element.topic(), element.partition(), CALL_LIMIT)
                    .map_err(|error| ExecError::new(client::call_error(&consumer, &error).1))?;
                rows.push((
                    element.topic().to_owned(),
                    element.partition(),
                    offset,
                    high,
                ));
            }
            Ok::<_, ExecError>(rows)
        })
        .await
        .map_err(|_| ExecError::new(text::KAFKA_CALL_FAILED))??;
        let mut committed = committed;
        committed.sort();
        set_columns(
            &["topic", "partition", "committed", "end", "lag", "member"],
            sink,
        )
        .await;
        let rows = committed
            .iter()
            .map(|(topic, partition, offset, high)| {
                let member = members
                    .iter()
                    .find(|member| {
                        member
                            .assignment
                            .iter()
                            .any(|(name, id)| name == topic && id == partition)
                    })
                    .map(|member| member.client.clone());
                number_row(&[
                    Some(topic.clone()),
                    Some(partition.to_string()),
                    Some(offset.to_string()),
                    Some(high.to_string()),
                    Some((high - offset).max(0).to_string()),
                    member,
                ])
            })
            .collect();
        push_rows(rows, sink).await;
        if committed.is_empty() {
            sink.notices(vec![text::KAFKA_GROUP_EMPTY.to_owned()], 0)
                .await;
        }
        Ok(true)
    }
}
