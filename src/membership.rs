use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::fmt::Debug;

use stateright::actor::{Id, Out};

use crate::fake_crypto::SigSet;
use crate::stable_set::StableSet;
use crate::Node;
use crate::{fake_crypto::Sig, stable_set::Member};

#[derive(
    Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize,
)]
pub enum Action {
    ReqJoin(Id),
    JoinShare(Member),
    Nop,
}

#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize)]
pub struct Msg {
    stable_set: StableSet,
    action: Action,
}

impl Debug for Msg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Msg({:?}, {:?})", self.stable_set, self.action)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Membership {
    pub stable_set: StableSet,
    pub joining_state: Option<(u64, SigSet<(u64, Id)>)>,
}

impl Membership {
    pub fn new(genesis: &BTreeSet<Id>) -> Self {
        let mut stable_set = StableSet::default();

        for genesis_id in genesis.iter().copied() {
            let genesis_member = Member {
                id: genesis_id,
                ord_idx: 0,
            };
            for other_genesis_id in genesis.iter().copied() {
                stable_set.add(genesis_member.clone(), other_genesis_id);
            }
        }

        stable_set.process_ready_to_join(genesis);

        assert_eq!(&BTreeSet::from_iter(stable_set.ids()), genesis);

        Self {
            stable_set,
            joining_state: None,
        }
    }

    fn build_msg(&self, action: Action) -> Msg {
        let stable_set = self.stable_set.clone();
        Msg { stable_set, action }
    }

    pub fn req_join(&self, id: Id) -> Msg {
        self.build_msg(Action::ReqJoin(id))
    }

    pub fn is_member(&self, id: Id) -> bool {
        self.stable_set.contains(id)
    }

    pub fn members(&self) -> impl Iterator<Item = Member> {
        self.stable_set.members()
    }

    pub fn on_msg(&mut self, elders: &BTreeSet<Id>, id: Id, src: Id, msg: Msg, o: &mut Out<Node>) {
        let Msg { stable_set, action } = msg;
        self.stable_set.merge(src, stable_set, elders);

        match action {
            Action::ReqJoin(candidate_id) => {
                if !self.stable_set.has_seen(candidate_id) && elders.contains(&id) {
                    let latest_ord_idx = self
                        .stable_set
                        .members()
                        .map(|m| m.ord_idx)
                        .max()
                        .unwrap_or(0);
                    let ord_idx = latest_ord_idx + 1;

                    let member = Member {
                        id: candidate_id,
                        ord_idx,
                    };
                    self.stable_set.add(member.clone(), src);
                    self.stable_set.add(member.clone(), id);

                    o.broadcast(
                        BTreeSet::from_iter([src, candidate_id]).union(elders),
                        &self.build_msg(Action::JoinShare(member)).into(),
                    );
                }
            }
            Action::JoinShare(member) => {
                if !self.stable_set.has_seen(member.id) {
                    self.stable_set.add(member, src);

                    if self.stable_set.process_ready_to_join(elders) {
                        o.broadcast(
                            &BTreeSet::from_iter(self.stable_set.ids()),
                            &self.build_msg(Action::Nop).into(),
                        );
                    }
                }
            }
            Action::Nop => {}
        }
    }
}
