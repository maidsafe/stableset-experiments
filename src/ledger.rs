use std::collections::BTreeMap;

use stateright::actor::{Id, Out};

use crate::{
    fake_crypto::{SectionSig, Sig, SigSet},
    membership::{Elders, Membership},
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Msg {
    ReqReissue(Tx),
    ReissueShare(Tx, Sig<Tx>),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Wallet {
    pub ledger: Ledger,
    pub pending_tx: Option<(Tx, SigSet<Tx>)>,
}

impl Wallet {
    fn build_msg(&self, membership: &Membership, msg: Msg) -> crate::Msg {
        let stable_set = membership.stable_set.clone();

        crate::Msg {
            stable_set,
            action: msg.into(),
        }
    }

    pub fn new(elders: &Elders) -> Self {
        Self {
            ledger: Ledger::new(elders),
            pending_tx: None,
        }
    }

    pub fn reissue(
        &mut self,
        membership: &Membership,
        inputs: Vec<Dbc>,
        outputs: Vec<u64>,
        o: &mut Out<crate::Node>,
    ) {
        let tx = Tx { inputs, outputs };
        self.pending_tx = Some((tx.clone(), SigSet::new()));

        o.broadcast(
            &membership.elders(),
            &self.build_msg(membership, Msg::ReqReissue(tx)),
        )
    }

    pub fn on_msg(
        &mut self,
        membership: &Membership,
        id: Id,
        src: Id,
        msg: Msg,
        o: &mut Out<crate::Node>,
    ) {
        // If we have a pending transaction and the elders changed, we need to restart the reissue
        let elders = membership.elders();

        if let Some((tx, sig)) = self.pending_tx.as_ref() {
            self.reissue(membership, tx.inputs.clone(), tx.outputs.clone(), o);
        }

        match msg {
            Msg::ReqReissue(tx) => {
                if elders.contains(&id) {
                    if let Some(sig_share) = self.ledger.tx_share(id, &elders, tx.clone()) {
                        o.send(
                            src,
                            self.build_msg(membership, Msg::ReissueShare(tx, sig_share).into()),
                        )
                    }
                }
            }
            Msg::ReissueShare(tx, sig_share) => {
                if elders.contains(&src) {
                    if let Some((pending_tx, sig)) = self.pending_tx.as_mut() {
                        if pending_tx == &tx && sig_share.verify(src, &tx) {
                            sig.add_share(src, sig_share)

                            // todo: do something with finished transaction
                            // self.pending_tx = None;
                        }
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DbcId {
    inputs: Vec<Dbc>,
    output_index: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tx {
    pub inputs: Vec<Dbc>,
    pub outputs: Vec<u64>,
}

impl Tx {
    pub fn verify_sums(&self) -> bool {
        self.inputs.iter().map(Dbc::amount).sum::<u64>() == self.outputs.iter().sum::<u64>()
    }

    pub fn output_dbc_ids_and_amounts(&self) -> Vec<(DbcId, u64)> {
        Vec::from_iter(
            self.outputs
                .iter()
                .enumerate()
                .map(|(output_index, amount)| {
                    (
                        DbcId {
                            inputs: self.inputs.clone(),
                            output_index: output_index as u64,
                        },
                        *amount,
                    )
                }),
        )
    }

    pub fn output_dbcs(&self, tx_sig: SectionSig<Tx>) -> Vec<Dbc> {
        Vec::from_iter((0..self.outputs.len() as u64).map(|output_index| Dbc {
            output_index,
            tx: self.clone(),
            tx_sig: tx_sig.clone(),
        }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Dbc {
    pub output_index: u64,
    pub tx: Tx,
    pub tx_sig: SectionSig<Tx>,
}

impl Dbc {
    pub fn id(&self) -> DbcId {
        DbcId {
            inputs: self.tx.inputs.clone(),
            output_index: self.output_index,
        }
    }

    pub fn amount(&self) -> u64 {
        self.tx.outputs[self.output_index as usize]
    }

    pub fn verify(&self, elders: &Elders) -> bool {
        self.output_index < self.tx.outputs.len() as u64
            && self.tx.verify_sums()
            && self.tx_sig.verify(elders, &self.tx)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ledger {
    pub genesis_dbc: Dbc,
    pub commitments: BTreeMap<DbcId, Tx>,
}

impl Ledger {
    pub fn new(elders: &Elders) -> Self {
        let genesis_tx = Tx {
            inputs: vec![],
            outputs: vec![100],
        };
        let mut tx_sig = SectionSig::new(elders.clone());
        for elder in elders {
            tx_sig.add_share(*elder, Sig::sign(*elder, genesis_tx.clone()));
        }
        Self {
            genesis_dbc: Dbc {
                output_index: 0,
                tx: genesis_tx,
                tx_sig,
            },
            commitments: Default::default(),
        }
    }

    pub fn genesis_amount(&self) -> u64 {
        self.genesis_dbc.amount()
    }

    pub fn sum_unspent_outputs(&self) -> u64 {
        let mut sum = 0;
        for (dbc_id, amount) in std::iter::once(&self.genesis_dbc.tx)
            .chain(self.commitments.values())
            .flat_map(|tx| tx.output_dbc_ids_and_amounts())
        {
            if !self.commitments.contains_key(&dbc_id) {
                sum += amount
            }
        }

        sum
    }

    pub fn tx_share(&mut self, id: Id, elders: &Elders, tx: Tx) -> Option<Sig<Tx>> {
        if !tx.verify_sums() {
            return None;
        }

        for dbc in tx.inputs.iter() {
            if !dbc.verify(elders) && dbc != &self.genesis_dbc {
                return None;
            }

            for dbc_parent in dbc.tx.inputs.iter() {
                let parent_tx = self.commitments.get(&dbc_parent.id())?;
                if parent_tx != &dbc.tx {
                    return None;
                }
            }

            if let Some(existing_tx) = self.commitments.get(&dbc.id()) {
                if existing_tx != &tx {
                    return None;
                }
            }
        }

        // If all input dbc's are valid, then we update our ledger
        for dbc in tx.inputs.iter().cloned() {
            self.commitments.insert(dbc.id(), tx.clone());
        }

        Some(Sig::sign(id, tx))
    }
}
