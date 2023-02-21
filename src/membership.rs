use std::cmp::Ordering;
use std::collections::BTreeSet;

use stateright::actor::{Id, Out};

use crate::fake_crypto::SigSet;
use crate::stable_set::StableSet;
use crate::Node;
use crate::{fake_crypto::Sig, stable_set::Member};

#[derive(
    Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize,
)]
pub enum Msg {
    ReqJoin(Id, Member),
    JoinShare(u64, Id, Sig<(u64, Id)>, Member),
    Sync(Vec<Member>),
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Membership {
    pub stable_set: StableSet,
    pub joining_state: Option<(u64, SigSet<(u64, Id)>)>,
}

impl Membership {
    pub fn new(elders: &BTreeSet<Id>) -> Self {
        let mut stable_set = StableSet::default();

        for node in elders.iter().copied() {
            let mut sig = SigSet::new();
            for genesis_signer in elders.iter().copied() {
                sig.add_share(genesis_signer, Sig::sign(genesis_signer, (0, node)));
            }

            stable_set.add(0, node, sig);
        }

        Self {
            stable_set,
            joining_state: None,
        }
    }

    pub fn req_join(&self, id: Id) -> Msg {
        let last_member = self.stable_set.last_member().unwrap();
        Msg::ReqJoin(id, last_member)
    }

    pub fn is_member(&self, id: Id) -> bool {
        self.stable_set.contains(id)
    }

    pub fn members(&self) -> impl Iterator<Item = Member> {
        self.stable_set.members()
    }

    pub fn on_msg(&mut self, elders: &BTreeSet<Id>, id: Id, src: Id, msg: Msg, o: &mut Out<Node>) {
        match msg {
            Msg::ReqJoin(candidate_id, member) => {
                if !self.stable_set.contains(candidate_id) && member.verify(elders) {
                    self.stable_set.apply(member);
                    let last_member = self.stable_set.last_member().unwrap();
                    let ord_idx = last_member.ord_idx + 1;
                    let sig = Sig::sign(id, (ord_idx, candidate_id));
                    o.send(
                        src,
                        Msg::JoinShare(ord_idx, candidate_id, sig, last_member).into(),
                    );
                }
            }
            Msg::JoinShare(ord_idx, candidate_id, sig, last_member) => {
                let join_msg = (ord_idx, candidate_id);
                if id == candidate_id
                    && !self.stable_set.contains(id)
                    && sig.verify(src, &join_msg)
                    && last_member.verify(elders)
                    && last_member.ord_idx + 1 == ord_idx
                {
                    self.stable_set.apply(last_member);

                    let joining_sig = if let Some((curr_ord_idx, sig)) = self.joining_state.as_mut()
                    {
                        match (*curr_ord_idx).cmp(&ord_idx) {
                            Ordering::Greater => {
                                // we've moved on to larger indices
                                let last_member = self.stable_set.last_member().unwrap();
                                o.send(src, Msg::ReqJoin(id, last_member).into());
                                return;
                            }
                            Ordering::Less => {
                                let last_member = self.stable_set.last_member().unwrap();
                                o.broadcast(
                                    elders.iter().filter(|id| id != &&src),
                                    &Msg::ReqJoin(id, last_member).into(),
                                );

                                self.joining_state = Some((ord_idx, SigSet::new()));
                                &mut self.joining_state.as_mut().unwrap().1
                            }
                            Ordering::Equal => sig,
                        }
                    } else {
                        self.joining_state = Some((ord_idx, SigSet::new()));
                        &mut self.joining_state.as_mut().unwrap().1
                    };

                    joining_sig.add_share(src, sig);

                    if joining_sig.verify(elders, &join_msg) {
                        let member = Member {
                            ord_idx,
                            id: candidate_id,
                            sig: joining_sig.clone(),
                        };
                        self.stable_set.apply(member.clone());

                        o.broadcast(
                            self.stable_set.ids().filter(|i| i != &&id),
                            &Msg::Sync(vec![member]).into(),
                        )
                    }
                }
            }
            Msg::Sync(msgs) => {
                let mut new_members = Vec::new();
                for member in msgs {
                    if !self.stable_set.has_seen(member.id) && member.verify(elders) {
                        new_members.push(member.clone());
                        self.stable_set.apply(member);
                    }
                }

                if !new_members.is_empty() {
                    o.broadcast(
                        new_members.iter().map(|m| &m.id),
                        &Msg::Sync(Vec::from_iter(self.stable_set.members())).into(),
                    );
                    // o.broadcast(self.stable_set.ids(), &Msg::Sync(new_members));
                }
            }
        }
    }
}
