use super::*;

#[test]
fn partial_selection_matches_the_stable_sort_with_duplicates_and_ties() {
    for count in [0, 1, 49, 50, 51, 251, 4096] {
        let mut builder = Builder::new("", None, Dialect::Postgres);
        for index in 0..count {
            let name = format!("name_{:03}", (index * 37) % 113);
            let label = if index % 3 == 0 {
                name.to_uppercase()
            } else {
                name.clone()
            };
            builder.push(
                (index % 2) as u8,
                Suggestion {
                    label,
                    insert: if index % 7 == 0 {
                        format!("{name}_{index}")
                    } else {
                        name
                    },
                    kind: if index % 5 == 0 {
                        SuggestionKind::Table
                    } else {
                        SuggestionKind::Column
                    },
                    detail: (index % 4 == 0).then(|| format!("owner_{}", index % 9)),
                    cursor_offset: None,
                },
            );
        }
        let mut expected: Vec<_> = builder
            .items
            .iter()
            .map(|item| (item.rank, item.suggestion.clone()))
            .collect();
        expected.sort_by_cached_key(|(rank, suggestion)| {
            (
                *rank,
                suggestion.kind,
                suggestion.label.to_lowercase(),
                suggestion.detail.clone(),
            )
        });
        expected.dedup_by(|a, b| a.1.kind == b.1.kind && a.1.insert == b.1.insert);
        expected.truncate(MAX_ITEMS);
        let expected: Vec<_> = expected
            .into_iter()
            .map(|(_, suggestion)| suggestion)
            .collect();
        assert_eq!(
            builder.finish(0..0, "").items,
            expected,
            "{count} candidates"
        );
    }
}

#[test]
fn duplicate_batches_do_not_hide_later_suggestions() {
    let mut builder = Builder::new("", None, Dialect::Postgres);
    for _ in 0..5000 {
        builder.add_name("aaa", SuggestionKind::Column, None);
    }
    for index in 0..75 {
        builder.add_name(&format!("column_{index:03}"), SuggestionKind::Column, None);
    }
    let completion = builder.finish(0..0, "");
    assert_eq!(completion.items.len(), MAX_ITEMS);
    assert_eq!(completion.items[0].label, "aaa");
    assert_eq!(completion.items[49].label, "column_048");
}
