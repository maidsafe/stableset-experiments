use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    iter::FromIterator,
};

use stateright::{
    actor::{model_peers, Actor, ActorModel, Id, Network, Out},
    Expectation, Model,
};

#[derive(
    Clone, Debug, Eq, Hash, PartialEq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct Sig<T> {
    // HACK: we'll just use the signer's Id and msg as the signature
    signer: Id,
    msg: T,
}

impl<T: Eq> Sig<T> {
    fn verify(&self, id: Id, msg: &T) -> bool {
        &self.msg == msg && self.signer == id
    }

    fn sign(signer: Id, msg: T) -> Self {
        Self { signer, msg }
    }
}

#[derive(
    Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct SectionSig<T> {
    voters: BTreeSet<Id>,
    sigs: BTreeMap<Id, Sig<T>>,
}

impl<T: Eq> SectionSig<T> {
    fn verify(&self, msg: &T) -> bool {
        3 * self.sigs.len() > 2 * self.voters.len()
            && self.sigs.iter().all(|(id, sig)| sig.verify(*id, msg))
    }

    fn add_share(&mut self, id: Id, sig: Sig<T>) {
        if self.voters.contains(&id) {
            self.sigs.insert(id, sig);
        }
    }
}

#[derive(
    Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize,
)]
pub enum Msg {
    ReqAppend(Id),
    AppendShare(u64, Id, Sig<(u64, Id)>),
    Joined(u64, Id, SectionSig<(u64, Id)>),
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Default)]
struct StableSet {
    members: BTreeMap<(u64, Id), SectionSig<(u64, Id)>>,
    dead: BTreeSet<Id>,
}

impl StableSet {
    fn add(&mut self, ordering_id: u64, id: Id, sig: SectionSig<(u64, Id)>) {
        if sig.verify(&(ordering_id, id)) {
            self.members.insert((ordering_id, id), sig);
        }
    }

    fn remove(&mut self, id: Id) {
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

    fn contains(&self, id: Id) -> bool {
        !self.dead.contains(&id) && self.members.keys().any(|(_, m)| *m == id)
    }

    fn next_idx(&self) -> u64 {
        self.members
            .last_key_value()
            .map(|((idx, _), _)| *idx + 1)
            .unwrap_or(0)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct State {
    elders: BTreeSet<Id>,
    stable_set: StableSet,
    joining_section_sig: BTreeMap<u64, SectionSig<(u64, Id)>>,
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

        let sig = SectionSig {
            voters: elders.clone(),
            sigs: BTreeMap::from_iter([(self.genesis, Sig::sign(self.genesis, (0, self.genesis)))]),
        };

        stable_set.add(0, self.genesis, sig);

        if id != self.genesis {
            for peer in self.peers.iter() {
                o.send(*peer, Msg::ReqAppend(id));
            }
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
            Msg::ReqAppend(candidate_id) => {
                if !state.stable_set.contains(candidate_id) {
                    let ord_idx = state.stable_set.next_idx();
                    let sig = Sig::sign(id, (ord_idx, candidate_id));
                    o.send(src, Msg::AppendShare(ord_idx, candidate_id, sig));
                }
            }
            Msg::AppendShare(ord_idx, candidate_id, sig) => {
                let elders = state.elders.clone();
                let join_msg = (ord_idx, candidate_id);
                if id == candidate_id
                    && !state.stable_set.contains(id)
                    && sig.verify(src, &join_msg)
                {
                    let joining_sig =
                        state
                            .to_mut()
                            .joining_section_sig
                            .entry(ord_idx)
                            .or_insert(SectionSig {
                                voters: elders,
                                sigs: BTreeMap::new(),
                            });

                    joining_sig.add_share(src, sig);

                    if joining_sig.verify(&join_msg) {
                        o.broadcast(
                            &self.peers,
                            &Msg::Joined(ord_idx, candidate_id, joining_sig.clone()),
                        )
                    }
                }
            }
            Msg::Joined(ord_idx, candidate_id, sig) => {
                if sig.voters == state.elders && sig.verify(&(ord_idx, candidate_id)) {
                    state.to_mut().stable_set.add(ord_idx, candidate_id, sig)
                }
            }
        }
    }
}

#[derive(Clone)]
struct ModelCfg {
    server_count: usize,
    network: Network<<Node as Actor>::Msg>,
}

impl ModelCfg {
    fn into_model(self) -> ActorModel<Node, Self, Vec<Msg>> {
        ActorModel::new(self.clone(), vec![])
            .actors((0..self.server_count).map(|i| Node {
                genesis: 0.into(),
                peers: model_peers(i, self.server_count),
            }))
            .init_network(self.network)
            .property(Expectation::Eventually, "converged", |_, state| {
                let reference_stable_set = state.actor_states[0].stable_set.clone();

                for state in state.actor_states.iter() {
                    if reference_stable_set != state.stable_set {
                        return false;
                    }
                }

                true
            })
    }
}

fn main() {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));

    let network = Network::new_unordered_nonduplicating([]);

    ModelCfg {
        server_count: 3,
        network,
    }
    .into_model()
    .checker()
    .threads(num_cpus::get())
    .serve("localhost:3000");
}
