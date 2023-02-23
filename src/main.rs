mod fake_crypto;
mod handover;
mod ledger;
mod membership;
mod stable_set;

use std::{borrow::Cow, collections::BTreeSet, fmt::Debug};

use handover::Handover;
use ledger::Wallet;
use membership::Membership;
use stateright::{
    actor::{model_peers, Actor, ActorModel, ActorModelState, Id, Network, Out},
    Expectation, Model,
};

const ELDER_COUNT: usize = 1;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct State {
    pub handover: Handover,
    pub membership: Membership,
    pub wallet: Wallet,
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
    Wallet(ledger::Msg),
    StartReissue,
}

impl Debug for Msg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Msg::Membership(m) => write!(f, "{m:?}"),
            Msg::Handover(m) => write!(f, "{m:?}"),
            Msg::Wallet(m) => write!(f, "{m:?}"),
            Msg::StartReissue => write!(f, "StartReissue"),
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

impl From<ledger::Msg> for Msg {
    fn from(msg: ledger::Msg) -> Self {
        Self::Wallet(msg)
    }
}

impl Actor for Node {
    type Msg = Msg;
    type State = State;

    fn on_start(&self, id: Id, o: &mut Out<Self>) -> Self::State {
        let membership = Membership::new(&self.genesis_nodes);
        let handover = Handover::new(self.genesis_nodes.clone());
        let wallet = Wallet::new(&self.genesis_nodes);

        if !self.genesis_nodes.contains(&id) {
            o.broadcast(&self.genesis_nodes, &membership.req_join(id).into());
        }

        // if !self.genesis_nodes.contains(&id) {
        //     o.send(id, Msg::StartReissue);
        // }

        State {
            handover,
            membership,
            wallet,
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
                let elders = state.elder_candidates();
                state.to_mut().membership.on_msg(&elders, id, src, msg, o);
            }
            Msg::Handover(msg) => {
                let elder_candidates = state.elder_candidates();
                state
                    .to_mut()
                    .handover
                    .on_msg(elder_candidates, id, src, msg, o)
            }
            Msg::Wallet(msg) => {
                let elders = state.elder_candidates();
                state.to_mut().wallet.on_msg(&elders, id, src, msg, o)
            }
            Msg::StartReissue => {
                let elders = state.elder_candidates();
                let input = state.wallet.ledger.genesis_dbc.clone();

                let reissue_amount = (0..self.peers.len() + 1)
                    .find(|x| Id::from(*x) == id)
                    .unwrap() as u64;
                let difference = input.amount() - reissue_amount;

                state.to_mut().wallet.reissue(
                    &elders,
                    vec![input],
                    vec![reissue_amount, difference],
                    o,
                );
            }
        }

        // let elder_candidates = state.elder_candidates();
        // state
        //     .to_mut()
        //     .handover
        //     .try_trigger_handover(id, elder_candidates, o)
    }
}

#[derive(Clone)]
struct ModelCfg {
    genesis_nodes: usize,
    final_nodes: usize,
    network: Network<<Node as Actor>::Msg>,
}

fn prop_stable_set_converged(state: &ActorModelState<Node, Vec<Msg>>) -> bool {
    let reference_stable_set = state.actor_states[0].membership.stable_set.clone();

    state
        .actor_states
        .iter()
        .all(|actor| actor.membership.stable_set == reference_stable_set)
        && reference_stable_set.ids().count() == reference_stable_set.members().count()
}

fn prop_all_nodes_joined(state: &ActorModelState<Node, Vec<Msg>>) -> bool {
    state
        .actor_states
        .iter()
        .enumerate()
        .all(|(id, actor)| actor.membership.stable_set.contains(id.into()))
}

fn prop_unspent_outputs_equals_genesis_amount(state: &ActorModelState<Node, Vec<Msg>>) -> bool {
    state.actor_states.iter().all(|actor| {
        actor.wallet.ledger.genesis_amount() == actor.wallet.ledger.sum_unspent_outputs()
    })
}

// fn prop_oldest_nodes_are_elders(state: &ActorModelState<Node, Vec<Msg>>) -> bool {
//     state
//         .actor_states
//         .iter()
//         .all(|actor| actor.handover.elders() == actor.elder_candidates())
// }

// fn prop_nodes_agree_on_sap_chain(state: &ActorModelState<Node, Vec<Msg>>) -> bool {
//     let common_chain: Vec<(Elders, SectionSig<(usize, Elders)>)> = vec![];

//     for actor in state.actor_states.iter() {}

//     state
//         .actor_states
//         .iter()
//         .all(|actor| actor.handover.elders() == actor.elder_candidates())
// }

impl ModelCfg {
    fn into_model(self) -> ActorModel<Node, Self, Vec<Msg>> {
        ActorModel::new(self.clone(), vec![])
            .actors((0..self.final_nodes).map(|i| Node {
                genesis_nodes: BTreeSet::from_iter(
                    (0..self.genesis_nodes).into_iter().map(Id::from),
                ),
                peers: model_peers(i, self.final_nodes),
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
            .property(Expectation::Always, "Ledger balances", |_, state| {
                prop_unspent_outputs_equals_genesis_amount(state)
            })
            .property(
                Expectation::Always,
                "Never two nodes aggregate a double spend",
                |_, state| {
                    let concurrent_txs = BTreeSet::from_iter(
                        state
                            .actor_states
                            .iter()
                            .filter_map(|a| a.wallet.pending_tx.clone())
                            .filter(|(tx, sig)| sig.verify(&sig.voters, tx))
                            .map(|(tx, _)| tx),
                    );

                    concurrent_txs.len() <= 1
                },
            )
        // .property(
        //     Expectation::Eventually,
        //     "the most stable nodes of the final stable set are elders",
        //     |_, state| {
        //         prop_stable_set_converged(state)
        //             && prop_all_nodes_joined(state)
        //             && prop_oldest_nodes_are_elders(state)
        //     },
        // )
    }
}

fn main() {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));

    let network = Network::new_unordered_nonduplicating([]);

    ModelCfg {
        genesis_nodes: 1,
        final_nodes: 4,
        network,
    }
    .into_model()
    .checker()
    .threads(num_cpus::get())
    .serve("localhost:3000");
}
