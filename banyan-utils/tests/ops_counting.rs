use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use banyan::{
    query::{AllQuery, OffsetRangeQuery, Query},
    store::{BlockWriter, BranchCache, MemStore, ReadOnlyStore},
    Config, Forest, Secrets, StreamBuilder, Transaction, Tree,
};
use banyan_utils::{
    tag_index::TagSet,
    tags::{Key, Sha256Digest, TT},
};

#[derive(Clone)]
struct OpsCountingStore<S> {
    inner: S,
    reads: Arc<AtomicU64>,
    writes: Arc<AtomicU64>,
}

impl<S> OpsCountingStore<S> {
    fn new(inner: S) -> Self {
        Self {
            inner,
            reads: Arc::new(AtomicU64::default()),
            writes: Arc::new(AtomicU64::default()),
        }
    }

    fn reads(&self) -> u64 {
        self.reads.load(Ordering::SeqCst)
    }
}

impl<L, S: ReadOnlyStore<L>> ReadOnlyStore<L> for OpsCountingStore<S> {
    fn get(&self, link: &L) -> anyhow::Result<Box<[u8]>> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.inner.get(link)
    }
}

impl<L, S: BlockWriter<L> + Send + Sync> BlockWriter<L> for OpsCountingStore<S> {
    fn put(&self, data: Vec<u8>) -> anyhow::Result<L> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.inner.put(data)
    }
}

#[allow(clippy::clippy::type_complexity)]
fn test_ops_count(
    name: &str,
    forest: &Forest<TT, u64, OpsCountingStore<MemStore<Sha256Digest>>>,
    tree: &Tree<TT>,
    query: impl Query<TT> + Clone + 'static,
) -> (Vec<anyhow::Result<(u64, Key, u64)>>, Duration, u64) {
    let r0 = forest.store().reads();
    let t0 = Instant::now();
    let xs: Vec<anyhow::Result<(u64, Key, u64)>> = forest.iter_filtered(&tree, query).collect();
    let dt = t0.elapsed();
    let dr = forest.store().reads() - r0;
    println!("{} {} {}", name, dr, dt.as_micros());
    (xs, dt, dr)
}

#[test]
fn ops_count_1() -> anyhow::Result<()> {
    let n = 1000000;
    let capacity = 0;
    let xs = (0..n)
        .map(|i| (Key::single(i, i, TagSet::empty()), i))
        .collect::<Vec<_>>();
    let store = MemStore::new(usize::max_value(), Sha256Digest::digest);
    let store = OpsCountingStore::new(store);
    let branch_cache = BranchCache::<TT>::new(capacity);
    let txn = Transaction::new(Forest::new(store.clone(), branch_cache), store.clone());
    let mut builder = StreamBuilder::new(Config::debug_fast(), Secrets::default());
    txn.extend(&mut builder, xs)?;
    let tree = builder.snapshot();

    let r0 = store.reads();
    let xs1 = txn.collect(&tree)?;
    let r_collect = store.reads() - r0;

    let (xs2, _, r_iter) = test_ops_count("", &txn, &tree, AllQuery);
    let (xs3, _, r_iter_small) = test_ops_count("", &txn, &tree, OffsetRangeQuery::from(0..n / 10));
    let (xs4, _, r_iter_tiny) = test_ops_count("", &txn, &tree, OffsetRangeQuery::from(0..10));

    assert!(xs1.len() as u64 == n);
    assert!(xs2.len() as u64 == n);
    assert!(xs3.len() as u64 == n / 10);
    assert!(xs4.len() as u64 == 10);

    assert_eq!(r_collect, 65);
    assert_eq!(r_iter, 65);
    assert_eq!(r_iter_small, 10);
    assert_eq!(r_iter_tiny, 4);

    Ok(())
}