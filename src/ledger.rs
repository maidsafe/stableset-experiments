use std::collections::{BTreeMap, BTreeSet};

use stateright::actor::{Id, Out};

use crate::{
    build_msg,
    fake_crypto::{majority, SigSet},
    membership::{Elders, Membership},
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Msg {
    ReqReissue(Tx),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Wallet {
    pub ledger: Ledger,
}

impl Wallet {
    pub fn new(elders: &Elders) -> Self {
        Self {
            ledger: Ledger::new(elders),
        }
    }

    pub fn read_tx(&self, dbc_id: &DbcId) -> Option<Tx> {
        self.ledger.commitments.get(dbc_id).cloned()
    }

    pub fn reissue(
        &mut self,
        membership: &Membership,
        inputs: Vec<Dbc>,
        outputs: Vec<u64>,
        o: &mut Out<crate::Node>,
    ) {
        let tx = Tx { inputs, outputs };

        o.broadcast(
            &membership.elders(),
            &build_msg(membership, Msg::ReqReissue(tx)),
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
        let elders = membership.elders();

        match msg {
            Msg::ReqReissue(tx) => {
                if self.ledger.log_tx_share(id, tx.clone(), src) {
                    o.broadcast(
                        elders.iter().filter(|e| e != &&id),
                        &build_msg(membership, Msg::ReqReissue(tx)),
                    )
                }
            }
        }

        self.ledger.process_completed_commitments(membership)
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

    pub fn output_dbcs(&self) -> Vec<Dbc> {
        Vec::from_iter((0..self.outputs.len() as u64).map(|output_index| Dbc {
            output_index,
            tx: self.clone(),
        }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Dbc {
    pub output_index: u64,
    pub tx: Tx,
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

    pub fn verify(&self) -> bool {
        self.output_index < self.tx.outputs.len() as u64 && self.tx.verify_sums()
    }
}

pub fn genesis_dbc() -> Dbc {
    Dbc {
        output_index: 0,
        tx: Tx {
            inputs: vec![],
            outputs: vec![100],
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ledger {
    pub commitments: BTreeMap<DbcId, Tx>,
    pub pending_commitments: BTreeMap<Tx, BTreeSet<Id>>,
}

impl Ledger {
    pub fn new(elders: &Elders) -> Self {
        Self {
            commitments: Default::default(),
            pending_commitments: Default::default(),
        }
    }

    pub fn sum_unspent_outputs(&self) -> u64 {
        let mut sum = 0;
        for (dbc_id, amount) in std::iter::once(&genesis_dbc().tx)
            .chain(self.commitments.values())
            .flat_map(|tx| tx.output_dbc_ids_and_amounts())
        {
            if !self.commitments.contains_key(&dbc_id) {
                sum += amount
            }
        }

        sum
    }

    pub fn validate_tx(&self, tx: &Tx) -> bool {
        if !tx.verify_sums() {
            return false;
        }

        for input_dbc in tx.inputs.iter() {
            if !(input_dbc.verify() || input_dbc == &genesis_dbc()) {
                return false;
            }

            // Check that the DBC's used to create this input were all committed to the dbc's TX
            for input_dbc_parent in input_dbc.tx.inputs.iter() {
                let parent_tx = if let Some(tx) = self.commitments.get(&input_dbc_parent.id()) {
                    tx
                } else {
                    return false;
                };
                if parent_tx != &input_dbc.tx {
                    return false;
                }
            }

            // Check that this input DBC isn't already committed to a tx.
            if self.commitments.contains_key(&input_dbc.id()) {
                return false;
            }

            // Check that this input DBC isn't already in a pending commitment
            for pending_tx in self.pending_commitments.keys() {
                let input_dbc_in_pending_tx = pending_tx
                    .inputs
                    .iter()
                    .any(|pending_dbc| pending_dbc == input_dbc);

                if input_dbc_in_pending_tx && pending_tx != tx {
                    return false;
                }
            }
        }

        true
    }

    // Returns true if this is the first time we've seen this tx and it was valid, false otherwise
    pub fn log_tx_share(&mut self, id: Id, tx: Tx, witness: Id) -> bool {
        if !self.validate_tx(&tx) {
            return false;
        }

        let first_time_seeing_tx = !self.pending_commitments.contains_key(&tx);

        // If all input dbc's are valid, then we add the Tx to the pending commitments.
        let witnesses = self.pending_commitments.entry(tx).or_default();
        witnesses.insert(witness);
        witnesses.insert(id);

        first_time_seeing_tx
    }

    pub fn process_completed_commitments(&mut self, membership: &Membership) {
        let elders = membership.elders();

        let ready_commitments = Vec::from_iter(
            self.pending_commitments
                .iter()
                .filter(|(_, witnesses)| {
                    majority(witnesses.intersection(&elders).count(), elders.len())
                })
                .map(|(tx, _)| tx)
                .cloned(),
        );

        for tx in ready_commitments {
            for input_dbc in tx.inputs.iter() {
                self.commitments.insert(input_dbc.id(), tx.clone());
            }

            self.pending_commitments.remove(&tx);
        }
    }
}
