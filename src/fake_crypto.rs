use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Debug,
};

use stateright::actor::Id;

#[derive(Clone, Eq, Hash, PartialEq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct Sig<T> {
    // HACK: we'll just use the signer's Id and msg as the signature
    signer: Id,
    msg: T,
}

impl<T: Debug> Debug for Sig<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}@{:?}", self.msg, self.signer)
    }
}

impl<T: Eq> Sig<T> {
    pub fn verify(&self, id: Id, msg: &T) -> bool {
        &self.msg == msg && self.signer == id
    }

    pub fn sign(signer: Id, msg: T) -> Self {
        Self { signer, msg }
    }
}

#[derive(Clone, Hash, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct SectionSig<T> {
    voters: BTreeSet<Id>,
    shares: BTreeMap<Id, Sig<T>>,
}

impl<T: Eq> SectionSig<T> {
    pub fn new(voters: BTreeSet<Id>) -> Self {
        Self {
            voters,
            shares: Default::default(),
        }
    }

    pub fn verify(&self, voters: &BTreeSet<Id>, msg: &T) -> bool {
        &self.voters == voters
            && self.has_threshold()
            && self.shares.iter().all(|(id, sig)| sig.verify(*id, msg))
    }

    pub fn add_share(&mut self, signer: Id, sig: Sig<T>) -> bool {
        if self.voters.contains(&signer) {
            self.shares.insert(signer, sig);
        }

        self.has_threshold()
    }

    fn has_threshold(&self) -> bool {
        3 * self.shares.len() > 2 * self.voters.len()
    }
}

impl<T: Debug + Clone + Ord> Debug for SectionSig<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut msgs: BTreeMap<T, BTreeSet<Id>> = Default::default();

        for (signer, sig_share) in self.shares.iter() {
            msgs.entry(sig_share.msg.clone())
                .or_default()
                .insert(*signer);
        }

        write!(f, "section_sig(")?;

        for (msg, signers) in msgs {
            write!(f, "{msg:?}@{signers:?}")?;
        }

        if !self.has_threshold() {
            write!(f, ", not enough shares")?;
        }

        write!(f, ")")
    }
}
