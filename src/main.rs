mod fake_crypto;
mod handover;
mod membership;
mod stable_set;

use std::{borrow::Cow, collections::BTreeSet, fmt::Debug};

use handover::Handover;
use membership::Membership;
use stateright::{
    actor::{model_peers, Actor, ActorModel, ActorModelState, Id, Network, Out},
    Expectation, Model,
};

const ELDER_COUNT: usize = 7;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct State {
    pub handover: Handover,
    pub membership: Membership,
}

impl State {
    fn elder_candidates(&self) -> BTreeSet<Id> {
        BTreeSet::from_iter(self.membership.members().take(ELDER_COUNT).map(|m| m.id))
    }
}

#[derive(Clone)]
pub struct Node {
    pub genesis_nodes: BTreeSet<Id>,
    pub peers: Vec<Id>,
}

#[derive(Clone, Eq, Hash, PartialEq)]
pub enum Msg {
    Membership(membership::Msg),
    Handover(handover::Msg),
}

impl Debug for Msg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Msg::Membership(m) => write!(f, "{m:?}"),
            Msg::Handover(m) => write!(f, "{m:?}"),
        }
    }
}

impl From<membership::Msg> for Msg {
    fn from(msg: membership::Msg) -> Self {
        Self::Membership(msg)
    }
}

impl From<handover::Msg> for Msg {
    fn from(msg: handover::Msg) -> Self {
        Self::Handover(msg)
    }
}

impl Actor for Node {
    type Msg = Msg;
    type State = State;

    fn on_start(&self, id: Id, o: &mut Out<Self>) -> Self::State {
        let membership = Membership::new(&self.genesis_nodes);
        let handover = Handover::new(self.genesis_nodes.clone());

        if !self.genesis_nodes.contains(&id) {
            o.broadcast(&self.genesis_nodes, &membership.req_join(id).into());
        }

        State {
            handover,
            membership,
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
            Msg::Membership(msg) => {
                let elders = state.handover.elders();
                state.to_mut().membership.on_msg(&elders, id, src, msg, o);
            }
            Msg::Handover(msg) => {
                let elder_candidates = state.elder_candidates();
                state
                    .to_mut()
                    .handover
                    .on_msg(elder_candidates, id, src, msg, o)
            }
        }

        let elder_candidates = state.elder_candidates();
        state
            .to_mut()
            .handover
            .try_trigger_handover(id, elder_candidates, o)
    }
}

#[derive(Clone)]
struct ModelCfg {
    elder_count: usize,
    server_count: usize,
    network: Network<<Node as Actor>::Msg>,
}

fn prop_stable_set_converged(state: &ActorModelState<Node, Vec<Msg>>) -> bool {
    let reference_stable_set = state.actor_states[0].membership.stable_set.clone();

    state
        .actor_states
        .iter()
        .all(|actor| actor.membership.stable_set == reference_stable_set)
}

fn prop_all_nodes_joined(state: &ActorModelState<Node, Vec<Msg>>) -> bool {
    state
        .actor_states
        .iter()
        .enumerate()
        .all(|(id, actor)| actor.membership.stable_set.contains(id.into()))
}

fn prop_oldest_nodes_are_elders(state: &ActorModelState<Node, Vec<Msg>>) -> bool {
    state
        .actor_states
        .iter()
        .all(|actor| actor.handover.elders() == actor.elder_candidates())
}

impl ModelCfg {
    fn into_model(self) -> ActorModel<Node, Self, Vec<Msg>> {
        ActorModel::new(self.clone(), vec![])
            .actors((0..self.server_count).map(|i| Node {
                genesis_nodes: BTreeSet::from_iter((0..self.elder_count).into_iter().map(Id::from)),
                peers: model_peers(i, self.server_count),
            }))
            .init_network(self.network)
            .property(
                Expectation::Eventually,
                "everyone eventually sees the same stable set",
                |_, state| prop_stable_set_converged(state),
            )
            .property(
                Expectation::Eventually,
                "everyone is part of the final stable set",
                |_, state| prop_stable_set_converged(state) && prop_all_nodes_joined(state),
            )
            .property(
                Expectation::Eventually,
                "the most stable nodes of the final stable set are elders",
                |_, state| {
                    prop_stable_set_converged(state)
                        && prop_all_nodes_joined(state)
                        && prop_oldest_nodes_are_elders(state)
                },
            )
    }
}

fn main() {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));

    let network = Network::new_unordered_duplicating([]);

    ModelCfg {
        elder_count: 2,
        server_count: 5,
        network,
    }
    .into_model()
    .checker()
    .threads(num_cpus::get())
    .serve("localhost:3000");
}
