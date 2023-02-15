use std::collections::BTreeSet;

use stateright::actor::{Id, Out};

use crate::{
    fake_crypto::{SectionSig, Sig},
    Node,
};

#[derive(Clone, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub struct Sap {
    gen: usize,
    elders: Elders,
    sig: SectionSig<(usize, Elders)>,
}
impl Sap {
    fn verify(&self, prev_elders: &BTreeSet<Id>) -> bool {
        self.sig
            .verify(prev_elders, &(self.gen, self.elders.clone()))
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Msg {
    ReqHandoverShare(usize, Elders),
    HandoverShare(usize, Elders, Sig<(usize, Elders)>),
    Handover(Sap),
}

pub type Elders = BTreeSet<Id>;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Handover {
    genesis: Elders,
    chain: Vec<(Elders, SectionSig<(usize, Elders)>)>,
    handover_sig: Option<Sap>,
}

impl Handover {
    pub fn new(genesis: Elders) -> Self {
        let chain = vec![];
        Self {
            genesis,
            chain,
            handover_sig: None,
        }
    }

    pub fn elders(&self) -> Elders {
        if let Some((elders, _)) = self.chain.last().cloned() {
            elders
        } else {
            self.genesis.clone()
        }
    }

    pub fn gen(&self) -> usize {
        self.chain.len()
    }

    pub fn on_msg(
        &mut self,
        elder_candidates: BTreeSet<Id>,
        id: Id,
        src: Id,
        msg: Msg,
        o: &mut Out<Node>,
    ) {
        let elders = self.elders();
        match msg {
            Msg::ReqHandoverShare(gen, candidates) => {
                if gen == self.gen() + 1 && candidates == elder_candidates {
                    o.send(
                        src,
                        Msg::HandoverShare(gen, elder_candidates, Sig::sign(id, (gen, candidates)))
                            .into(),
                    )
                }
            }
            Msg::HandoverShare(gen, candidates, sig) => {
                if let Some(sap) = self.handover_sig.as_mut() {
                    if sap.gen == gen
                        && sap.elders == candidates
                        && elders.contains(&src)
                        && sig.verify(src, &(gen, candidates))
                    {
                        sap.sig.add_share(src, sig);

                        if sap.verify(&elders) {
                            o.broadcast(
                                &BTreeSet::from_iter(
                                    elders.iter().chain(sap.elders.iter()).copied(),
                                ),
                                &Msg::Handover(sap.clone()).into(),
                            );
                        }
                    }
                }
            }
            Msg::Handover(sap) => {
                if sap.gen == self.gen() + 1 && sap.verify(&elders) {
                    self.chain.push((sap.elders, sap.sig))
                }
            }
        }
    }

    pub(crate) fn try_trigger_handover(
        &mut self,
        id: Id,
        elder_candidates: BTreeSet<Id>,
        o: &mut Out<Node>,
    ) {
        if self.elders() != elder_candidates && elder_candidates.contains(&id) {
            let sap = Sap {
                gen: self.gen() + 1,
                elders: elder_candidates.clone(),
                sig: SectionSig::new(self.elders()),
            };

            if Some(&sap) == self.handover_sig.as_ref() {
                return;
            }

            self.handover_sig = Some(sap);

            o.broadcast(
                &self.elders(),
                &Msg::ReqHandoverShare(self.gen() + 1, elder_candidates).into(),
            )
        }
    }
}
