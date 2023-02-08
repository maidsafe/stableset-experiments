mod fake_crypto;
mod membership;
mod stable_set;

use std::collections::BTreeSet;

use membership::{Msg, Node};
use stateright::{
    actor::{model_peers, Actor, ActorModel, Id, Network},
    Expectation, Model,
};

#[derive(Clone)]
struct ModelCfg {
    elder_count: usize,
    server_count: usize,
    network: Network<<Node as Actor>::Msg>,
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
                "everyone joined and converged",
                |_, state| {
                    let reference_stable_set = state.actor_states[0].stable_set.clone();

                    let all_nodes_joined = (0..state.actor_states.len())
                        .into_iter()
                        .map(Id::from)
                        .all(|id| reference_stable_set.contains(id));

                    if !all_nodes_joined {
                        return false;
                    }

                    for state in state.actor_states.iter() {
                        if reference_stable_set != state.stable_set {
                            return false;
                        }
                    }

                    true
                },
            )
    }
}

fn main() {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));

    let network = Network::new_unordered_nonduplicating([]);

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
