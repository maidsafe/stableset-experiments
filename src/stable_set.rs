use std::collections::{BTreeMap, BTreeSet};

use stateright::actor::Id;

use crate::fake_crypto::SigSet;

#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize)]
pub struct Member {
    pub ord_idx: u64,
    pub id: Id,
    pub sig: SigSet<(u64, Id)>,
}

impl Member {
    pub fn verify(&self, voters: &BTreeSet<Id>) -> bool {
        self.sig.verify(voters, &(self.ord_idx, self.id))
    }
}

impl std::fmt::Debug for Member {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "M({}.{:?})", self.ord_idx, self.id)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Default)]
pub struct StableSet {
    members: BTreeMap<(u64, Id), SigSet<(u64, Id)>>,
    dead: BTreeSet<Id>,
}

impl StableSet {
    pub fn apply(&mut self, member: Member) {
        self.add(member.ord_idx, member.id, member.sig);
    }

    pub fn add(&mut self, ordering_id: u64, id: Id, sig: SigSet<(u64, Id)>) {
        self.members.insert((ordering_id, id), sig);
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

    pub fn last_member(&self) -> Option<Member> {
        self.members
            .last_key_value()
            .map(|((ord_idx, id), sig)| Member {
                ord_idx: *ord_idx,
                id: *id,
                sig: sig.clone(),
            })
    }

    pub fn ids(&self) -> impl Iterator<Item = &Id> {
        self.members.keys().map(|(_, id)| id)
    }

    pub fn members(&self) -> impl Iterator<Item = Member> {
        self.members
            .clone()
            .into_iter()
            .map(|((ord_idx, id), sig)| Member { ord_idx, id, sig })
    }

    pub(crate) fn has_seen(&self, id: Id) -> bool {
        self.dead.contains(&id) || self.members.keys().any(|(_, m)| *m == id)
    }
}
