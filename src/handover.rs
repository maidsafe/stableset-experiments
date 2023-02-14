use std::collections::BTreeSet;

use stateright::actor::{Id, Out};

use crate::{fake_crypto::SectionSig, Node};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Msg {}

pub type Elders = BTreeSet<Id>;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Handover {
    genesis: Elders,
    chain: Vec<(Elders, SectionSig<(usize, Elders)>)>,
}

impl Handover {
    pub fn new(genesis: Elders) -> Self {
        let chain = vec![];
        Self { genesis, chain }
    }

    pub fn elders(&self) -> Elders {
        if let Some((elders, _)) = self.chain.last().cloned() {
            elders
        } else {
            self.genesis.clone()
        }
    }

    pub fn on_msg(&mut self, id: Id, src: Id, msg: Msg, o: &mut Out<Node>) {
        match msg {}
    }
}
