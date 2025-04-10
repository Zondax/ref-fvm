// Copyright 2021-2023 Protocol Labs
// Copyright 2019-2022 ChainSafe Systems
// SPDX-License-Identifier: Apache-2.0, MIT

use std::borrow::Borrow;
use std::marker::PhantomData;

use cid::Cid;
use forest_hash_utils::BytesKey;
use fvm_ipld_blockstore::Blockstore;
use fvm_ipld_encoding::CborStore;
use multihash::Code;
use serde::de::DeserializeOwned;
use serde::{Serialize, Serializer};

use crate::node::Node;
use crate::{Config, Error, Hash, HashAlgorithm, Sha256};

/// Implementation of the HAMT data structure for IPLD.
///
/// # Examples
///
/// ```
/// use fvm_ipld_hamt::Hamt;
///
/// let store = fvm_ipld_blockstore::MemoryBlockstore::default();
///
/// let mut map: Hamt<_, _, usize> = Hamt::new(store);
/// map.set(1, "a".to_string()).unwrap();
/// assert_eq!(map.get(&1).unwrap(), Some(&"a".to_string()));
/// assert_eq!(map.delete(&1).unwrap(), Some((1, "a".to_string())));
/// assert_eq!(map.get::<_>(&1).unwrap(), None);
/// let cid = map.flush().unwrap();
/// ```
#[derive(Debug)]
pub struct Hamt<BS, V, K = BytesKey, H = Sha256> {
    root: Node<K, V, H>,
    store: BS,
    conf: Config,
    hash: PhantomData<H>,
    /// Remember the last flushed CID until it changes.
    flushed_cid: Option<Cid>,
}

impl<BS, V, K, H> Serialize for Hamt<BS, V, K, H>
where
    K: Serialize,
    V: Serialize,
    H: HashAlgorithm,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.root.serialize(serializer)
    }
}

impl<K: PartialEq, V: PartialEq, S: Blockstore, H: HashAlgorithm> PartialEq for Hamt<S, V, K, H> {
    fn eq(&self, other: &Self) -> bool {
        self.root == other.root
    }
}

impl<BS, V, K, H> Hamt<BS, V, K, H>
where
    K: Hash + Eq + PartialOrd + Serialize + DeserializeOwned,
    V: Serialize + DeserializeOwned,
    BS: Blockstore,
    H: HashAlgorithm,
{
    pub fn new(store: BS) -> Self {
        Self::new_with_config(store, Config::default())
    }

    pub fn new_with_config(store: BS, conf: Config) -> Self {
        Self {
            root: Node::default(),
            store,
            conf,
            hash: Default::default(),
            flushed_cid: None,
        }
    }

    /// Construct hamt with a bit width
    pub fn new_with_bit_width(store: BS, bit_width: u32) -> Self {
        Self::new_with_config(
            store,
            Config {
                bit_width,
                ..Default::default()
            },
        )
    }

    /// Lazily instantiate a hamt from this root Cid.
    pub fn load(cid: &Cid, store: BS) -> Result<Self, Error> {
        Self::load_with_config(cid, store, Config::default())
    }

    /// Lazily instantiate a hamt from this root Cid with a specified parameters.
    pub fn load_with_config(cid: &Cid, store: BS, conf: Config) -> Result<Self, Error> {
        match store.get_cbor(cid)? {
            Some(root) => Ok(Self {
                root,
                store,
                conf,
                hash: Default::default(),
                flushed_cid: Some(*cid),
            }),
            None => Err(Error::CidNotFound(cid.to_string())),
        }
    }
    /// Lazily instantiate a hamt from this root Cid with a specified bit width.
    pub fn load_with_bit_width(cid: &Cid, store: BS, bit_width: u32) -> Result<Self, Error> {
        Self::load_with_config(
            cid,
            store,
            Config {
                bit_width,
                ..Default::default()
            },
        )
    }

    /// Sets the root based on the Cid of the root node using the Hamt store
    pub fn set_root(&mut self, cid: &Cid) -> Result<(), Error> {
        match self.store.get_cbor(cid)? {
            Some(root) => {
                self.root = root;
                self.flushed_cid = Some(*cid);
            }
            None => return Err(Error::CidNotFound(cid.to_string())),
        }

        Ok(())
    }

    /// Returns a reference to the underlying store of the Hamt.
    pub fn store(&self) -> &BS {
        &self.store
    }

    /// Inserts a key-value pair into the HAMT.
    ///
    /// If the HAMT did not have this key present, `None` is returned.
    ///
    /// If the HAMT did have this key present, the value is updated, and the old
    /// value is returned. The key is not updated, though;
    ///
    /// # Examples
    ///
    /// ```
    /// use fvm_ipld_hamt::Hamt;
    /// use std::rc::Rc;
    ///
    /// let store = fvm_ipld_blockstore::MemoryBlockstore::default();
    ///
    /// let mut map: Hamt<_, _, usize> = Hamt::new(Rc::new(store));
    /// map.set(37, "a".to_string()).unwrap();
    /// assert_eq!(map.is_empty(), false);
    ///
    /// map.set(37, "b".to_string()).unwrap();
    /// map.set(37, "c".to_string()).unwrap();
    /// ```
    pub fn set(&mut self, key: K, value: V) -> Result<Option<V>, Error>
    where
        V: PartialEq,
    {
        let (old, modified) = self
            .root
            .set(key, value, self.store.borrow(), &self.conf, true)?;

        if modified {
            self.flushed_cid = None;
        }

        Ok(old)
    }

    /// Inserts a key-value pair into the HAMT only if that key does not already exist.
    ///
    /// If the HAMT did not have this key present, `true` is returned and the key/value is added.
    ///
    /// If the HAMT did have this key present, this function will return false
    ///
    /// # Examples
    ///
    /// ```
    /// use fvm_ipld_hamt::Hamt;
    /// use std::rc::Rc;
    ///
    /// let store = fvm_ipld_blockstore::MemoryBlockstore::default();
    ///
    /// let mut map: Hamt<_, _, usize> = Hamt::new(Rc::new(store));
    /// let a = map.set_if_absent(37, "a".to_string()).unwrap();
    /// assert_eq!(map.is_empty(), false);
    /// assert_eq!(a, true);
    ///
    /// let b = map.set_if_absent(37, "b".to_string()).unwrap();
    /// assert_eq!(b, false);
    /// assert_eq!(map.get(&37).unwrap(), Some(&"a".to_string()));
    ///
    /// let c = map.set_if_absent(30, "c".to_string()).unwrap();
    /// assert_eq!(c, true);
    /// ```
    pub fn set_if_absent(&mut self, key: K, value: V) -> Result<bool, Error>
    where
        V: PartialEq,
    {
        let set = self
            .root
            .set(key, value, self.store.borrow(), &self.conf, false)
            .map(|(_, set)| set)?;

        if set {
            self.flushed_cid = None;
        }

        Ok(set)
    }

    /// Returns a reference to the value corresponding to the key.
    ///
    /// The key may be any borrowed form of the map's key type, but
    /// `Hash` and `Eq` on the borrowed form *must* match those for
    /// the key type.
    ///
    /// # Examples
    ///
    /// ```
    /// use fvm_ipld_hamt::Hamt;
    /// use std::rc::Rc;
    ///
    /// let store = fvm_ipld_blockstore::MemoryBlockstore::default();
    ///
    /// let mut map: Hamt<_, _, usize> = Hamt::new(Rc::new(store));
    /// map.set(1, "a".to_string()).unwrap();
    /// assert_eq!(map.get(&1).unwrap(), Some(&"a".to_string()));
    /// assert_eq!(map.get(&2).unwrap(), None);
    /// ```
    #[inline]
    pub fn get<Q: ?Sized>(&self, k: &Q) -> Result<Option<&V>, Error>
    where
        K: Borrow<Q>,
        Q: Hash + Eq,
        V: DeserializeOwned,
    {
        match self.root.get(k, self.store.borrow(), &self.conf)? {
            Some(v) => Ok(Some(v)),
            None => Ok(None),
        }
    }

    /// Returns `true` if a value exists for the given key in the HAMT.
    ///
    /// The key may be any borrowed form of the map's key type, but
    /// `Hash` and `Eq` on the borrowed form *must* match those for
    /// the key type.
    ///
    /// # Examples
    ///
    /// ```
    /// use fvm_ipld_hamt::Hamt;
    /// use std::rc::Rc;
    ///
    /// let store = fvm_ipld_blockstore::MemoryBlockstore::default();
    ///
    /// let mut map: Hamt<_, _, usize> = Hamt::new(Rc::new(store));
    /// map.set(1, "a".to_string()).unwrap();
    /// assert_eq!(map.contains_key(&1).unwrap(), true);
    /// assert_eq!(map.contains_key(&2).unwrap(), false);
    /// ```
    #[inline]
    pub fn contains_key<Q: ?Sized>(&self, k: &Q) -> Result<bool, Error>
    where
        K: Borrow<Q>,
        Q: Hash + Eq,
    {
        Ok(self.root.get(k, self.store.borrow(), &self.conf)?.is_some())
    }

    /// Removes a key from the HAMT, returning the value at the key if the key
    /// was previously in the HAMT.
    ///
    /// The key may be any borrowed form of the HAMT's key type, but
    /// `Hash` and `Eq` on the borrowed form *must* match those for
    /// the key type.
    ///
    /// # Examples
    ///
    /// ```
    /// use fvm_ipld_hamt::Hamt;
    /// use std::rc::Rc;
    ///
    /// let store = fvm_ipld_blockstore::MemoryBlockstore::default();
    ///
    /// let mut map: Hamt<_, _, usize> = Hamt::new(Rc::new(store));
    /// map.set(1, "a".to_string()).unwrap();
    /// assert_eq!(map.delete(&1).unwrap(), Some((1, "a".to_string())));
    /// assert_eq!(map.delete(&1).unwrap(), None);
    /// ```
    pub fn delete<Q: ?Sized>(&mut self, k: &Q) -> Result<Option<(K, V)>, Error>
    where
        K: Borrow<Q>,
        Q: Hash + Eq,
    {
        let deleted = self.root.remove_entry(k, self.store.borrow(), &self.conf)?;

        if deleted.is_some() {
            self.flushed_cid = None;
        }

        Ok(deleted)
    }

    /// Flush root and return Cid for hamt
    pub fn flush(&mut self) -> Result<Cid, Error> {
        if let Some(cid) = self.flushed_cid {
            return Ok(cid);
        }
        self.root.flush(self.store.borrow())?;
        let cid = self.store.put_cbor(&self.root, Code::Blake2b256)?;
        self.flushed_cid = Some(cid);
        Ok(cid)
    }

    /// Returns true if the HAMT has no entries
    pub fn is_empty(&self) -> bool {
        self.root.is_empty()
    }

    /// Iterates over each KV in the Hamt and runs a function on the values.
    ///
    /// This function will constrain all values to be of the same type
    ///
    /// # Examples
    ///
    /// ```
    /// use fvm_ipld_hamt::Hamt;
    ///
    /// let store = fvm_ipld_blockstore::MemoryBlockstore::default();
    ///
    /// let mut map: Hamt<_, _, usize> = Hamt::new(store);
    /// map.set(1, 1).unwrap();
    /// map.set(4, 2).unwrap();
    ///
    /// let mut total = 0;
    /// map.for_each(|_, v: &u64| {
    ///    total += v;
    ///    Ok(())
    /// }).unwrap();
    /// assert_eq!(total, 3);
    /// ```
    #[inline]
    pub fn for_each<F>(&self, mut f: F) -> Result<(), Error>
    where
        V: DeserializeOwned,
        F: FnMut(&K, &V) -> anyhow::Result<()>,
    {
        self.root.for_each(self.store.borrow(), &mut f)
    }

    /// Consumes this HAMT and returns the Blockstore it owns.
    pub fn into_store(self) -> BS {
        self.store
    }
}
