use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Debug,
};

use stateright::actor::Id;

use crate::{fake_crypto::majority, membership::Elders};

#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize)]
pub struct Member {
    pub ord_idx: u64,
    pub id: Id,
}

impl std::fmt::Debug for Member {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "M({}.{:?})", self.ord_idx, self.id)
    }
}

#[derive(
    Clone, Eq, Hash, PartialEq, PartialOrd, Ord, Default, serde::Serialize, serde::Deserialize,
)]
pub struct StableSet {
    members: BTreeSet<Member>,
    dead: BTreeSet<Id>,
    pub joining_members: BTreeMap<Member, BTreeSet<Id>>,
    pub leaving_members: BTreeMap<Member, BTreeSet<Id>>,
}

impl Debug for StableSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SS({:?}, joining:{:?})",
            self.members, self.joining_members
        )
    }
}

impl StableSet {
    pub fn merge(&mut self, witness: Id, other: StableSet, elders: &Elders) {
        for member in other.members {
            if self.has_seen(member.id) {
                continue;
            }

            self.joining_members
                .entry(member)
                .or_default()
                .insert(witness);
        }

        self.process_ready_actions(elders);
        // TODO: merge with the dead nodes as well (needs the same flow as the joining nodes)
    }

    pub fn process_ready_actions(&mut self, elders: &Elders) -> bool {
        let mut updated = false;

        let ready_to_join = Vec::from_iter(
            self.joining_members
                .iter()
                .filter(|(_, witnesses)| {
                    majority(witnesses.intersection(elders).count(), elders.len())
                })
                .map(|(member, _)| member)
                .cloned(),
        );

        updated |= !ready_to_join.is_empty();

        for member in ready_to_join {
            self.joining_members.remove(&member);

            if let Some(existing_member_with_id) = self.members().find(|m| m.id == member.id) {
                if existing_member_with_id.ord_idx >= member.ord_idx {
                    continue;
                } else {
                    self.members.remove(&existing_member_with_id);
                }
            }

            self.members.insert(member);
        }

        let ready_to_leave = Vec::from_iter(
            self.leaving_members
                .iter()
                .filter(|(_, witnesses)| {
                    majority(witnesses.intersection(elders).count(), elders.len())
                })
                .map(|(member, _)| member)
                .cloned(),
        );

        updated |= !ready_to_leave.is_empty();

        for member in ready_to_leave {
            self.leaving_members.remove(&member);

            if let Some(existing_member_with_id) = self.members().find(|m| m.id == member.id) {
                self.members.remove(&existing_member_with_id);
            }
        }

        updated
    }

    pub fn add(&mut self, member: Member, witness: Id) -> bool {
        if !self.has_seen(member.id) {
            self.joining_members
                .entry(member)
                .or_default()
                .insert(witness)
        } else {
            false
        }
    }

    pub fn witnesses(&mut self, member: &Member) -> BTreeSet<Id> {
        self.joining_members
            .get(member)
            .cloned()
            .unwrap_or_default()
    }

    pub fn remove(&mut self, id: Id) {
        self.dead.insert(id);

        let to_be_removed = Vec::from_iter(self.members.iter().filter(|m| m.id == id).cloned());

        for member in to_be_removed {
            self.members.remove(&member);
        }
    }

    pub fn has_member(&self, member: &Member) -> bool {
        self.members.contains(member)
    }

    pub fn contains(&self, id: Id) -> bool {
        self.ids().any(|m| m == id)
    }

    pub fn ids(&self) -> impl Iterator<Item = Id> + '_ {
        self.members.iter().map(|m| m.id)
    }

    pub fn members(&self) -> impl Iterator<Item = Member> {
        self.members.clone().into_iter()
    }

    pub(crate) fn has_seen(&self, id: Id) -> bool {
        self.dead.contains(&id) || self.contains(id)
    }
}
