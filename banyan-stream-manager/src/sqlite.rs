//! helper methods to work with ipfs/ipld
use std::{
    collections::BTreeSet,
    ops::DerefMut,
    sync::{Arc, Mutex},
};

use anyhow::{anyhow, Result};
use banyan::store::{BlockWriter, ReadOnlyStore};
use ipfs_sqlite_block_store::{BlockStore, Config, OwnedBlock, TempPin};
use libipld::Cid;

use crate::tags::Sha256Digest;

#[derive(Clone)]
pub struct SqliteStore(Arc<Mutex<BlockStore>>);

impl SqliteStore {
    pub fn new(inner: BlockStore) -> Self {
        SqliteStore(Arc::new(Mutex::new(inner)))
    }

    pub fn memory() -> anyhow::Result<Self> {
        let store = BlockStore::memory(Config::default())?;
        Ok(SqliteStore(Arc::new(Mutex::new(store))))
    }

    pub(crate) fn lock(&self) -> impl DerefMut<Target = BlockStore> + '_ {
        self.0.lock().unwrap()
    }

    pub fn write(&self) -> SqliteStoreWrite {
        let store = self.clone();
        let pin = self.lock().temp_pin();
        SqliteStoreWrite {
            store,
            pin,
            written: Mutex::new(BTreeSet::new()),
        }
    }
}

impl ReadOnlyStore<Sha256Digest> for SqliteStore {
    fn get(&self, link: &Sha256Digest) -> Result<Box<[u8]>> {
        let cid = Cid::from(*link);
        let block = self.lock().get_block(&cid)?;
        if let Some(block) = block {
            Ok(block.into())
        } else {
            Err(anyhow!("block not found!"))
        }
    }
}

pub struct SqliteStoreWrite {
    store: SqliteStore,
    pin: TempPin,
    written: Mutex<BTreeSet<Sha256Digest>>,
}

impl SqliteStoreWrite {
    pub fn into_written(self) -> BTreeSet<Sha256Digest> {
        self.written.into_inner().unwrap()
    }
}

impl BlockWriter<Sha256Digest> for SqliteStoreWrite {
    fn put(&self, data: Vec<u8>) -> Result<Sha256Digest> {
        let digest = Sha256Digest::new(&data);
        let cid = digest.into();
        let block = OwnedBlock::new(cid, data);
        self.store.lock().put_block(&block, Some(&self.pin))?;
        self.written.lock().unwrap().insert(digest);
        Ok(digest)
    }
}