//! The map from keys to actions, with the overrides of the configuration.

use std::collections::HashMap;

use super::*;
/// The answer of the map to one key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lookup {
    Action(Action),
    /// The keys start a sequence that needs more keys.
    Sequence,
    None,
}

#[derive(Debug)]
pub struct Keymap {
    regions: HashMap<Region, Bindings>,
    keys: HashMap<Action, Vec<String>>,
}

#[derive(Debug, Default)]
struct Bindings {
    complete: HashMap<Vec<Chord>, Action>,
    prefixes: HashMap<Vec<Chord>, Action>,
}

impl Keymap {
    #[expect(clippy::expect_used, reason = "the default keys are constant")]
    pub fn new(mode: KeyboardMode) -> Self {
        Self::with_overrides(mode, &HashMap::new()).expect("the default keys have no conflict")
    }

    /// Applies the `keymap` part of the configuration. A list replaces the default
    /// keys of its action, and an empty list removes them.
    pub fn with_overrides(
        mode: KeyboardMode,
        overrides: &HashMap<String, Vec<String>>,
    ) -> Result<Self, String> {
        if let Some(name) = overrides
            .keys()
            .find(|name| !DEFAULTS.iter().any(|item| item.name == *name))
        {
            return Err(format!("Unknown action: {name}"));
        }
        let mut map = Self {
            regions: HashMap::new(),
            keys: HashMap::new(),
        };
        for item in DEFAULTS {
            let keys: Vec<String> = match overrides.get(item.name) {
                Some(keys) => keys.clone(),
                None => default_keys(item, mode),
            };
            for text in &keys {
                map.bind(item.region, item.action, item.name, text)?;
            }
            map.keys.insert(item.action, keys);
        }
        Ok(map)
    }

    fn bind(
        &mut self,
        region: Region,
        action: Action,
        name: &str,
        text: &str,
    ) -> Result<(), String> {
        let conflict = |other: Action| format!("{} conflicts with {name}: {text}.", other.name());
        let chords = text
            .split_whitespace()
            .map(Chord::parse)
            .collect::<Result<Vec<_>, _>>()?;
        if chords.is_empty() {
            return Err(format!("Unknown key: {text}"));
        }
        let bindings = self.regions.entry(region).or_default();
        for end in 1..=chords.len() {
            if let Some(other) = bindings.complete.get(&chords[..end]) {
                return Err(conflict(*other));
            }
        }
        if let Some(other) = bindings.prefixes.get(chords.as_slice()) {
            return Err(conflict(*other));
        }
        for end in 1..chords.len() {
            bindings.prefixes.insert(chords[..end].to_vec(), action);
        }
        bindings.complete.insert(chords, action);
        Ok(())
    }

    pub fn lookup(&self, region: Region, key: Chord) -> Lookup {
        self.sequence(region, &[key])
    }

    /// Returns the action or prefix of a sequence of any length.
    pub fn sequence(&self, region: Region, keys: &[Chord]) -> Lookup {
        let Some(bindings) = self.regions.get(&region) else {
            return Lookup::None;
        };
        if let Some(action) = bindings.complete.get(keys) {
            return Lookup::Action(*action);
        }
        if bindings.prefixes.contains_key(keys) {
            return Lookup::Sequence;
        }
        Lookup::None
    }

    /// Returns the action of a sequence with two keys.
    pub fn after(&self, region: Region, first: Chord, second: Chord) -> Option<Action> {
        match self.sequence(region, &[first, second]) {
            Lookup::Action(action) => Some(action),
            _ => None,
        }
    }

    /// Returns the keys of an action, as the user writes them.
    pub fn keys(&self, action: Action) -> &[String] {
        self.keys.get(&action).map_or(&[], Vec::as_slice)
    }

    /// Lists the actions of a region in the order of the help screen.
    pub fn actions(&self, region: Region) -> impl Iterator<Item = Action> + '_ {
        Action::in_region(region)
    }
}

pub(super) fn default_keys(item: &Item, mode: KeyboardMode) -> Vec<String> {
    let vim = mode == KeyboardMode::Vim;
    let base = if vim && item.conventional_only {
        &[][..]
    } else {
        item.keys
    };
    let extra = if vim { item.vim_keys } else { &[] };
    base.iter()
        .chain(extra)
        .map(|key| (*key).to_owned())
        .collect()
}
