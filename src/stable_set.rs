use std::collections::{BTreeMap, BTreeSet};

use stateright::actor::Id;

use crate::fake_crypto::SectionSig;

#[derive(Clone, Debug, Eq, Hash, PartialEq, Default)]
pub struct StableSet {
    members: BTreeMap<(u64, Id), SectionSig<(u64, Id)>>,
    dead: BTreeSet<Id>,
}

impl StableSet {
    pub fn add(&mut self, ordering_id: u64, id: Id, section_sig: SectionSig<(u64, Id)>) {
        self.members.insert((ordering_id, id), section_sig);
    }

    pub fn remove(&mut self, id: Id) {
        self.dead.insert(id);

        let to_be_removed = Vec::from_iter(
            self.members
                .keys()
                .filter(|(_, other_id)| other_id == &id)
                .cloned(),
        );

        for member in to_be_removed {
            self.members.remove(&member);
        }
    }

    pub fn contains(&self, id: Id) -> bool {
        !self.dead.contains(&id) && self.members.keys().any(|(_, m)| *m == id)
    }

    pub fn next_idx(&self) -> u64 {
        self.members
            .last_key_value()
            .map(|((idx, _), _)| *idx + 1)
            .unwrap_or(0)
    }
}
