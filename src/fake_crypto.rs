use std::collections::{BTreeMap, BTreeSet};

use stateright::actor::Id;

#[derive(
    Clone, Debug, Eq, Hash, PartialEq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct Sig<T> {
    // HACK: we'll just use the signer's Id and msg as the signature
    signer: Id,
    msg: T,
}

impl<T: Eq> Sig<T> {
    pub fn verify(&self, id: Id, msg: &T) -> bool {
        &self.msg == msg && self.signer == id
    }

    pub fn sign(signer: Id, msg: T) -> Self {
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
    pub fn new(voters: BTreeSet<Id>) -> Self {
        Self {
            voters,
            sigs: Default::default(),
        }
    }

    pub fn verify(&self, voters: &BTreeSet<Id>, msg: &T) -> bool {
        &self.voters == voters
            && 3 * self.sigs.len() > 2 * self.voters.len()
            && self.sigs.iter().all(|(id, sig)| sig.verify(*id, msg))
    }

    pub fn add_share(&mut self, id: Id, sig: Sig<T>) {
        if self.voters.contains(&id) {
            self.sigs.insert(id, sig);
        }
    }
}
