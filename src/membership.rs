use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    iter::FromIterator,
};

use stateright::actor::{Actor, Id, Out};

use crate::fake_crypto::{SectionSig, Sig};
use crate::stable_set::StableSet;

#[derive(
    Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize,
)]
pub enum Msg {
    ReqJoin(Id),
    JoinShare(u64, Id, Sig<(u64, Id)>),
    Joined(u64, Id, SectionSig<(u64, Id)>),
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct State {
    pub elders: BTreeSet<Id>,
    pub stable_set: StableSet,
    pub joining_section_sig: BTreeMap<u64, SectionSig<(u64, Id)>>,
}

#[derive(Clone)]
pub struct Node {
    pub genesis: Id,
    pub peers: Vec<Id>,
}

impl Actor for Node {
    type Msg = Msg;
    type State = State;

    fn on_start(&self, id: Id, o: &mut Out<Self>) -> Self::State {
        let elders = BTreeSet::from_iter([self.genesis]);
        let mut stable_set = StableSet::default();

        let mut sig = SectionSig::new(elders.clone());
        sig.add_share(self.genesis, Sig::sign(self.genesis, (0, self.genesis)));

        stable_set.add(0, self.genesis, sig);

        if id != self.genesis {
            o.broadcast(elders.iter(), &Msg::ReqJoin(id));
        }

        State {
            elders,
            stable_set,
            joining_section_sig: BTreeMap::new(),
        }
    }

    fn on_msg(
        &self,
        id: Id,
        state: &mut Cow<Self::State>,
        src: Id,
        msg: Self::Msg,
        o: &mut Out<Self>,
    ) {
        match msg {
            Msg::ReqJoin(candidate_id) => {
                if !state.stable_set.contains(candidate_id) {
                    let ord_idx = state.stable_set.next_idx();
                    let sig = Sig::sign(id, (ord_idx, candidate_id));
                    o.send(src, Msg::JoinShare(ord_idx, candidate_id, sig));
                }
            }
            Msg::JoinShare(ord_idx, candidate_id, sig) => {
                let elders = state.elders.clone();
                let join_msg = (ord_idx, candidate_id);
                if id == candidate_id
                    && !state.stable_set.contains(id)
                    && sig.verify(src, &join_msg)
                {
                    let section_sig = state
                        .to_mut()
                        .joining_section_sig
                        .entry(ord_idx)
                        .or_insert(SectionSig::new(elders.clone()));

                    section_sig.add_share(src, sig);

                    if section_sig.verify(&elders, &join_msg) {
                        o.broadcast(
                            &elders,
                            &Msg::Joined(ord_idx, candidate_id, section_sig.clone()),
                        )
                    }
                }
            }
            Msg::Joined(ord_idx, candidate_id, section_sig) => {
                if !state.stable_set.has_seen(candidate_id)
                    && section_sig.verify(&state.elders, &(ord_idx, candidate_id))
                {
                    state
                        .to_mut()
                        .stable_set
                        .add(ord_idx, candidate_id, section_sig.clone());

                    o.broadcast(
                        state.stable_set.iter(),
                        &Msg::Joined(ord_idx, candidate_id, section_sig),
                    );

                    for ((ord_idx, member), sig) in state.stable_set.iter_signed() {
                        o.send(candidate_id, Msg::Joined(*ord_idx, *member, sig.clone()));
                    }
                }
            }
        }
    }
}
