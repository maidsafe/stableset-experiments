mod fake_crypto;
mod ledger;
mod membership;
mod stable_set;

use std::{borrow::Cow, collections::BTreeSet, fmt::Debug};

use ledger::Wallet;
use membership::Membership;
use stable_set::StableSet;
use stateright::{
    actor::{model_peers, Actor, ActorModel, ActorModelState, Id, Network, Out},
    Expectation, Model,
};

const ELDER_COUNT: usize = 5;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct State {
    pub membership: Membership,
    is_leaving: bool,
    pub wallet: Wallet,
}

impl State {
    fn elders(&self) -> BTreeSet<Id> {
        self.membership.elders()
    }

    fn build_msg(&self, action: Action) -> Msg {
        let stable_set = self.membership.stable_set.clone();
        Msg { stable_set, action }
    }
}

#[derive(Clone)]
pub struct Node {
    pub genesis_nodes: BTreeSet<Id>,
    pub peers: Vec<Id>,
}

#[derive(Clone, Eq, Hash, PartialEq)]
pub struct Msg {
    stable_set: StableSet,
    action: Action,
}

impl Debug for Msg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Msg({:?}, {:?})", self.stable_set, self.action)
    }
}

#[derive(Clone, Eq, Hash, PartialEq)]
pub enum Action {
    Membership(membership::Msg),
    Wallet(ledger::Msg),
    Sync,
    StartReissue,
    TriggerLeave,
}

impl Debug for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Membership(m) => write!(f, "{m:?}"),
            Self::Wallet(m) => write!(f, "{m:?}"),
            Self::Sync => write!(f, "Sync"),
            Self::StartReissue => write!(f, "StartReissue"),
            Self::TriggerLeave => write!(f, "TriggerLeave"),
        }
    }
}

impl From<membership::Msg> for Action {
    fn from(msg: membership::Msg) -> Self {
        Self::Membership(msg)
    }
}

impl From<ledger::Msg> for Action {
    fn from(msg: ledger::Msg) -> Self {
        Self::Wallet(msg)
    }
}

impl Actor for Node {
    type Msg = Msg;
    type State = State;

    fn on_start(&self, id: Id, o: &mut Out<Self>) -> Self::State {
        let membership = Membership::new(&self.genesis_nodes);
        let wallet = Wallet::new(&self.genesis_nodes);

        let state = State {
            membership,
            wallet,
            is_leaving: false,
        };

        if !self.genesis_nodes.contains(&id) {
            o.broadcast(&self.genesis_nodes, &state.membership.req_join(id));
        }

        // if !self.genesis_nodes.contains(&id) {
        //     o.send(id, state.build_msg(Action::StartReissue));
        // }

        state
    }

    fn on_msg(
        &self,
        id: Id,
        state: &mut Cow<Self::State>,
        src: Id,
        msg: Self::Msg,
        o: &mut Out<Self>,
    ) {
        let elders = state.elders();
        let Msg { stable_set, action } = msg;

        let mut nodes_to_sync = state.to_mut().membership.merge(stable_set, id, src);

        match action {
            Action::Sync => (),
            Action::Membership(msg) => {
                nodes_to_sync.extend(state.to_mut().membership.on_msg(&elders, id, src, msg, o));
            }
            Action::Wallet(msg) => {
                let membership = state.membership.clone();
                state.to_mut().wallet.on_msg(&membership, id, src, msg, o)
            }
            Action::StartReissue => {
                let input = state.wallet.ledger.genesis_dbc.clone();

                let reissue_amount = (0..self.peers.len() + 1)
                    .find(|x| Id::from(*x) == id)
                    .unwrap() as u64;
                let difference = input.amount() - reissue_amount;

                let membership = state.membership.clone();
                state.to_mut().wallet.reissue(
                    &membership,
                    vec![input],
                    vec![reissue_amount, difference],
                    o,
                );
            }
            Action::TriggerLeave => {
                o.broadcast(&elders, &state.to_mut().membership.req_leave(id).into());
            }
        }
        if id > Id::from((self.peers.len() * 2) / 3)
            && state.membership.is_member(id)
            && !state.is_leaving
        {
            state.to_mut().is_leaving = true;
            o.send(id, state.build_msg(Action::TriggerLeave));
        }

        nodes_to_sync.extend(state.to_mut().membership.process_pending_actions(id));

        o.broadcast(&nodes_to_sync, &state.build_msg(Action::Sync))
    }
}

#[derive(Clone)]
struct ModelCfg {
    elder_count: usize,
    server_count: usize,
    network: Network<<Node as Actor>::Msg>,
}

fn prop_stable_set_converged(state: &ActorModelState<Node, Vec<Msg>>) -> bool {
    let mut non_leaving_nodes = state.actor_states.iter().filter(|s| !s.is_leaving);

    let reference_stable_set = if let Some(s) = non_leaving_nodes
        .next()
        .map(|s| s.membership.stable_set.members())
    {
        s
    } else {
        return true;
    };

    non_leaving_nodes.all(|actor| actor.membership.stable_set.members() == reference_stable_set)
    // && reference_stable_set.ids().count() == reference_stable_set.members().len()
}

fn prop_all_nodes_joined_who_havent_left(state: &ActorModelState<Node, Vec<Msg>>) -> bool {
    state
        .actor_states
        .iter()
        .enumerate()
        .filter(|(_, actor)| !actor.is_leaving)
        .all(|(id, actor)| actor.membership.stable_set.contains(id.into()))
}

fn prop_all_nodes_who_are_leaving_eventually_left(state: &ActorModelState<Node, Vec<Msg>>) -> bool {
    let reference_stable_set = if let Some(s) = state
        .actor_states
        .iter()
        .find(|s| !s.is_leaving)
        .map(|s| s.membership.stable_set.clone())
    {
        s
    } else {
        return true;
    };

    state
        .actor_states
        .iter()
        .enumerate()
        .filter(|(_, actor)| actor.is_leaving)
        .all(|(id, _)| !reference_stable_set.contains(id.into()))
}

#[allow(unused)]
fn prop_unspent_outputs_equals_genesis_amount(state: &ActorModelState<Node, Vec<Msg>>) -> bool {
    state.actor_states.iter().all(|actor| {
        actor.wallet.ledger.genesis_amount() == actor.wallet.ledger.sum_unspent_outputs()
    })
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
                "everyone who hasn't left converges on the same stable set",
                |_, state| prop_stable_set_converged(state),
            )
            .property(
                Expectation::Eventually,
                "everyone who hasn't left is part of the final stable set",
                |_, state| prop_all_nodes_joined_who_havent_left(state),
            )
            .property(
                Expectation::Eventually,
                "everyone who started leaving, will leave",
                |_, state| prop_all_nodes_who_are_leaving_eventually_left(state),
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
                            .filter_map(|a| a.wallet.pending_tx.clone().map(|tx| (a.clone(), tx)))
                            .filter(|(a, (tx, sig))| sig.verify(&a.membership.elders(), tx))
                            .map(|(_, tx)| tx),
                    );

                    concurrent_txs.len() <= 1
                },
            )
    }
}

fn main() {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));

    let network = Network::new_unordered_nonduplicating([]);

    ModelCfg {
        elder_count: 1,
        server_count: 6,
        network,
    }
    .into_model()
    .checker()
    .threads(num_cpus::get())
    .serve("localhost:3000");
}
