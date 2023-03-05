use std::collections::BTreeSet;
use std::fmt::Debug;

use stateright::actor::{Id, Out};

use crate::stable_set::{Member, StableSet};
use crate::{Node, ELDER_COUNT};

pub type Elders = BTreeSet<Id>;

#[derive(
    Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize,
)]
pub enum Action {
    ReqJoin(Id),
    ReqLeave(Id),
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

        stable_set.process_ready_actions(genesis);

        assert_eq!(&BTreeSet::from_iter(stable_set.ids()), genesis);

        Self { stable_set }
    }

    fn build_msg(&self, action: Action) -> Msg {
        let stable_set = self.stable_set.clone();
        Msg { stable_set, action }
    }

    pub fn req_join(&self, id: Id) -> Msg {
        self.build_msg(Action::ReqJoin(id))
    }

    pub fn req_leave(&self, id: Id) -> Msg {
        self.build_msg(Action::ReqJoin(id))
    }

    pub fn is_member(&self, id: Id) -> bool {
        self.stable_set.contains(id)
    }

    pub fn members(&self) -> BTreeSet<Member> {
        self.stable_set.members()
    }

    pub fn elders(&self) -> Elders {
        BTreeSet::from_iter(self.members().into_iter().take(ELDER_COUNT).map(|m| m.id))
    }

    pub fn on_msg(&mut self, elders: &BTreeSet<Id>, id: Id, src: Id, msg: Msg, o: &mut Out<Node>) {
        let Msg { stable_set, action } = msg;
        for member in stable_set.members() {
            self.handle_join_share(id, elders, member, src, o);
        }

        for member in stable_set.joining() {
            self.handle_join_share(id, elders, member, src, o);
        }

        for member in stable_set.leaving() {
            self.handle_leave_share(id, elders, member, src, o);
        }

        match action {
            Action::ReqJoin(candidate_id) => {
                if self.stable_set.member_by_id(candidate_id).is_none() && elders.contains(&id) {
                    let latest_ord_idx = self
                        .stable_set
                        .members()
                        .iter()
                        .map(|m| m.ord_idx)
                        .max()
                        .unwrap_or(0);
                    let ord_idx = latest_ord_idx + 1;

                    let member = Member {
                        id: candidate_id,
                        ord_idx,
                    };

                    self.handle_join_share(id, elders, member, id, o);
                }
            }
            Action::ReqLeave(to_remove) => {
                if let Some(member) = self.stable_set.member_by_id(to_remove) {
                    self.handle_leave_share(id, elders, member.clone(), id, o);
                    self.handle_leave_share(id, elders, member, src, o);
                }
            }
            Action::JoinShare(member) => {
                self.handle_join_share(id, elders, member, src, o);
            }
            Action::Nop => {}
        }

        let mut should_send_sync = false;
        for member in self.stable_set.members() {
            if !stable_set.is_member(&member) {
                should_send_sync = true;
            }
        }

        if should_send_sync {
            o.send(src, self.build_msg(Action::Nop).into());
        }
    }

    fn handle_join_share(
        &mut self,
        id: Id,
        elders: &Elders,
        member: Member,
        witness: Id,
        o: &mut Out<Node>,
    ) {
        if self.stable_set.is_member(&member) {
            return;
        }

        let first_time_seeing_join = self.stable_set.joining_witnesses(&member).is_empty();

        self.stable_set.add(member.clone(), witness);
        self.stable_set.add(member.clone(), id);

        if self.stable_set.process_ready_actions(elders) && elders.contains(&id) {
            o.broadcast(
                &Vec::from_iter(self.stable_set.ids().filter(|e| e != &id)),
                &self.build_msg(Action::Nop).into(),
            );
            // o.send(member.id, self.build_msg(Action::Nop).into());
        } else if first_time_seeing_join && member.id != id {
            o.broadcast(
                BTreeSet::from_iter(elders.iter().filter(|e| e != &&id).chain([&member.id])),
                &self.build_msg(Action::JoinShare(member.clone())).into(),
            );
        }
    }

    fn handle_leave_share(
        &mut self,
        id: Id,
        elders: &Elders,
        member: Member,
        witness: Id,
        o: &mut Out<Node>,
    ) {
        if !self.stable_set.is_member(&member) {
            return;
        }

        let first_time_seeing_leave = self.stable_set.leaving_witnesses(&member).is_empty();

        self.stable_set.remove(member.clone(), witness);
        self.stable_set.remove(member.clone(), id);

        if self.stable_set.process_ready_actions(elders) && elders.contains(&id) {
            o.broadcast(
                &Vec::from_iter(self.stable_set.ids().filter(|e| e != &id)),
                &self.build_msg(Action::Nop).into(),
            );
            // o.send(member.id, self.build_msg(Action::Nop).into());
        } else if first_time_seeing_leave && member.id != id {
            o.broadcast(
                BTreeSet::from_iter(elders.iter().filter(|e| e != &&id).chain([&member.id])),
                &self.build_msg(Action::JoinShare(member.clone())).into(),
            );
        }
    }
}
