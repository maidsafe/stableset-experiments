use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Debug,
};

use stateright::actor::Id;

pub fn majority(m: usize, n: usize) -> bool {
    m > n / 2
}

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
pub struct SigSet<T> {
    shares: BTreeMap<Id, Sig<T>>,
}

impl<T: Eq> SigSet<T> {
    pub fn new() -> Self {
        Self {
            shares: BTreeMap::new(),
        }
    }

    pub fn merge(&mut self, other: SigSet<T>) {
        for (signer, sig) in other.shares {
            self.add_share(signer, sig);
        }
    }

    pub fn add_share(&mut self, signer: Id, sig: Sig<T>) {
        self.shares.insert(signer, sig);
    }

    pub fn verify(&self, voters: &BTreeSet<Id>, msg: &T) -> bool {
        let valid_shares_from_voters = self
            .shares
            .iter()
            .filter(|(id, _)| voters.contains(id))
            .filter(|(id, sig)| sig.verify(**id, msg))
            .count();

        majority(valid_shares_from_voters, voters.len())
    }

    pub fn ids(&self) -> BTreeSet<Id> {
        self.shares.iter().map(|(id, _)| id).cloned().collect()
    }
}

impl<T: Debug + Clone + Ord> Debug for SigSet<T> {
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

        write!(f, ")")
    }
}

#[derive(Clone, Hash, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct SectionSig<T> {
    pub voters: BTreeSet<Id>,
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
        majority(self.shares.len(), self.voters.len())
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
