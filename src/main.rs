mod fake_crypto;
mod stable_set;

use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    iter::FromIterator,
};

use stateright::{
    actor::{model_peers, Actor, ActorModel, Id, Network, Out},
    Expectation, Model,
};

use fake_crypto::{SectionSig, Sig};
use stable_set::StableSet;

#[derive(
    Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize,
)]
pub enum Msg {
    ReqAppend(Id),
    AppendShare(u64, Id, Sig<(u64, Id)>),
    Joined(u64, Id, SectionSig<(u64, Id)>),
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

        let mut sig = SectionSig::new(elders.clone());
        sig.add_share(self.genesis, Sig::sign(self.genesis, (0, self.genesis)));

        stable_set.add(0, self.genesis, sig);

        if id != self.genesis {
            o.broadcast(elders.iter(), &Msg::ReqAppend(id));
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
                    let joining_sig = state
                        .to_mut()
                        .joining_section_sig
                        .entry(ord_idx)
                        .or_insert(SectionSig::new(elders.clone()));

                    joining_sig.add_share(src, sig);

                    if joining_sig.verify(&elders, &join_msg) {
                        o.broadcast(
                            &self.peers,
                            &Msg::Joined(ord_idx, candidate_id, joining_sig.clone()),
                        )
                    }
                }
            }
            Msg::Joined(ord_idx, candidate_id, section_sig) => {
                if section_sig.verify(&state.elders, &(ord_idx, candidate_id)) {
                    state
                        .to_mut()
                        .stable_set
                        .add(ord_idx, candidate_id, section_sig)
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
