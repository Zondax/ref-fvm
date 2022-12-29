// Copyright 2021-2023 Protocol Labs
// SPDX-License-Identifier: Apache-2.0, MIT
use std::collections::HashMap;

use anyhow::{anyhow, Context};
use cid::Cid;
use fvm_ipld_blockstore::Blockstore;
use fvm_ipld_encoding::CborStore;

const ACCOUNT_ACTOR_NAME: &str = "account";
const INIT_ACTOR_NAME: &str = "init";
const SYSTEM_ACTOR_NAME: &str = "system";
const PLACEHOLDER_ACTOR_NAME: &str = "placeholder";
const EAM_ACTOR_NAME: &str = "eam";
const ETHACCOUNT_ACTOR_NAME: &str = "ethaccount";
const STORAGE_MARKET_ACTOR_NAME: &str = "storagemarket";
const STORAGE_POWER_ACTOR_NAME: &str = "storagepower";
const VERIFIED_REGISTRY_ACTOR_NAME: &str = "verifiedregistry";
const DATA_CAP_ACTOR_NAME: &str = "datacap";
const REWARD_ACTOR_NAME: &str = "reward";


/// A mapping of builtin actor CIDs to their respective types.
pub struct Manifest {
    account_code: Cid,
    placeholder_code: Cid,
    system_code: Cid,
    init_code: Cid,
    eam_code: Cid,
    ethaccount_code: Cid,
    storagemarket_code: Cid,
    storagepower_code: Cid,
    verifiedregistry_code: Cid,
    datacap_code: Cid,
    reward_code: Cid,

    by_id: HashMap<u32, Cid>,
    by_code: HashMap<Cid, u32>,
}

/// Create an "id CID" (for testing).
#[cfg(any(feature = "testing", test))]
const fn id_cid(name: &[u8]) -> Cid {
    use std::mem;

    use fvm_shared::{IDENTITY_HASH, IPLD_RAW};
    use multihash::Multihash;

    // This code is ugly because const fns are a bit ugly right now:
    //
    // 1. Compiler can't drop the result, we need to forget it manually.
    //    https://doc.rust-lang.org/error-index.html#E0493
    // 2. We can't unwrap, we need to explicitly panic.
    // 3. Rust can't figure out that "panic" will prevent a drop of the error case.
    let result = Multihash::wrap(IDENTITY_HASH, name);
    let k = if let Ok(mh) = &result {
        Cid::new_v1(IPLD_RAW, *mh)
    } else {
        panic!();
    };
    mem::forget(result); // This is const, so we don't really care about "leaks".

    k
}

impl Manifest {
    #[cfg(any(feature = "testing", test))]
    pub const DUMMY_CODES: &'static [(&'static str, Cid)] = &[
        ("system", id_cid(b"fil/test/system")),
        ("init", id_cid(b"fil/test/init")),
        ("eam", id_cid(b"fil/test/eam")),
        ("ethaccount", id_cid(b"fil/test/ethaccount")),
        ("cron", id_cid(b"fil/test/cron")),
        ("account", id_cid(b"fil/test/account")),
        ("placeholder", id_cid(b"fil/test/placeholder")),
        ("embryo", id_cid(b"fil/test/embryo")),
        ("storagemarket", id_cid(b"fil/test/storagemarket")),
        ("storagepower", id_cid(b"fil/test/storagepower")),
        ("datacap", id_cid(b"fil/test/datacap")),
        ("verifiedregistry", id_cid(b"fil/test/verifiedregistry")),
        ("reward", id_cid(b"fil/test/reward")),
    ];

    #[cfg(any(feature = "testing", test))]
    pub fn dummy() -> Self {
        Self::new(Self::DUMMY_CODES.iter().copied()).unwrap()
    }

    /// Load a manifest from the blockstore.
    pub fn load<B: Blockstore>(bs: &B, root_cid: &Cid, ver: u32) -> anyhow::Result<Manifest> {
        if ver != 1 {
            return Err(anyhow!("unsupported manifest version {}", ver));
        }

        let vec: Vec<(String, Cid)> = match bs.get_cbor(root_cid)? {
            Some(vec) => vec,
            None => {
                return Err(anyhow!("cannot find manifest root cid {}", root_cid));
            }
        };

        Manifest::new(vec)
    }

    /// Construct a new manifest from actor name/cid tuples.
    pub fn new(iter: impl IntoIterator<Item = (impl Into<String>, Cid)>) -> anyhow::Result<Self> {
        let mut by_name = HashMap::new();
        let mut by_id = HashMap::new();
        let mut by_code = HashMap::new();

        // Actors are indexed sequentially, starting at 1, in the order in which they appear in the
        // manifest. 0 is reserved for "everything else" (i.e., not a builtin actor).
        for ((name, code_cid), id) in iter.into_iter().zip(1u32..) {
            let name = name.into();
            by_id.insert(id, code_cid);
            by_code.insert(code_cid, id);
            by_name.insert(name, code_cid);
        }

        let account_code = *by_name
            .get(ACCOUNT_ACTOR_NAME)
            .context("manifest missing account actor")?;

        let system_code = *by_name
            .get(SYSTEM_ACTOR_NAME)
            .context("manifest missing system actor")?;

        let init_code = *by_name
            .get(INIT_ACTOR_NAME)
            .context("manifest missing init actor")?;

        let placeholder_code = *by_name
            .get(PLACEHOLDER_ACTOR_NAME)
            .context("manifest missing placeholder actor")?;

        let eam_code = *by_name
            .get(EAM_ACTOR_NAME)
            .context("manifest missing eam actor")?;

        let ethaccount_code = *by_name
            .get(ETHACCOUNT_ACTOR_NAME)
            .context("manifest missing ethaccount actor")?;

        let storagemarket_code = *by_name
            .get(STORAGE_MARKET_ACTOR_NAME)
            .context("manifest missing storagemarket actor")?;

        let storagepower_code = *by_name
            .get(STORAGE_POWER_ACTOR_NAME)
            .context("manifest missing storagepower actor")?;

        let verifiedregistry_code = *by_name
            .get(VERIFIED_REGISTRY_ACTOR_NAME)
            .context("manifest missing verifiedregistry actor")?;

        let datacap_code = *by_name
            .get(DATA_CAP_ACTOR_NAME)
            .context("manifest missing datacap actor")?;

        let reward_code = *by_name
            .get(REWARD_ACTOR_NAME)
            .context("manifest missing reward actor")?;

        Ok(Self {
            account_code,
            system_code,
            init_code,
            placeholder_code,
            eam_code,
            ethaccount_code,
            storagemarket_code,
            storagepower_code,
            verifiedregistry_code,
            datacap_code,
            reward_code,
            by_id,
            by_code,
        })
    }

    /// Returns the code CID for a builtin actor, given the actor's ID.
    pub fn code_by_id(&self, id: u32) -> Option<&Cid> {
        self.by_id.get(&id)
    }

    /// Returns the the actor code's "id" if it's a builtin actor. Otherwise, returns 0.
    pub fn id_by_code(&self, code: &Cid) -> u32 {
        self.by_code.get(code).copied().unwrap_or(0)
    }

    /// Returns true id the passed code CID is the account actor.
    pub fn is_account_actor(&self, cid: &Cid) -> bool {
        &self.account_code == cid
    }

    /// Returns true id the passed code CID is the placeholder actor.
    pub fn is_placeholder_actor(&self, cid: &Cid) -> bool {
        &self.placeholder_code == cid
    }

    /// Returns true id the passed code CID is the EthAccount actor.
    pub fn is_ethaccount_actor(&self, cid: &Cid) -> bool {
        &self.ethaccount_code == cid
    }

    /// Returns true id the passed code CID is the storagemarket actor.
    pub fn is_storagemarket_actor(&self, cid: &Cid) -> bool {
        &self.storagemarket_code == cid
    }

    /// Returns true id the passed code CID is the storagepower actor.
    pub fn is_storagepower_actor(&self, cid: &Cid) -> bool {
        &self.storagepower_code == cid
    }

    /// Returns true id the passed code CID is the verifiedregistry actor.
    pub fn is_verifiedregistry_actor(&self, cid: &Cid) -> bool {
        &self.verifiedregistry_code == cid
    }

    /// Returns true id the passed code CID is the datacap actor.
    pub fn is_datacap_actor(&self, cid: &Cid) -> bool {
        &self.datacap_code == cid
    }

    /// Returns true id the passed code CID is the reward actor.
    pub fn is_reward_actor(&self, cid: &Cid) -> bool {
        &self.reward_code == cid
    }

    pub fn builtin_actor_codes(&self) -> impl Iterator<Item = &Cid> {
        self.by_id.values()
    }

    /// Returns the code CID for the account actor.
    pub fn get_account_code(&self) -> &Cid {
        &self.account_code
    }

    /// Returns the code CID for the init actor.
    pub fn get_init_code(&self) -> &Cid {
        &self.init_code
    }

    /// Returns the code CID for the system actor.
    pub fn get_system_code(&self) -> &Cid {
        &self.system_code
    }

    /// Returns the code CID for the eam actor.
    pub fn get_eam_code(&self) -> &Cid {
        &self.eam_code
    }

    /// Returns the code CID for the system actor.
    pub fn get_placeholder_code(&self) -> &Cid {
        &self.placeholder_code
    }

    /// Returns the code CID for the Ethereum Account actor.
    pub fn get_ethaccount_code(&self) -> &Cid {
        &self.ethaccount_code
    }

    /// Returns the code CID for the storagemarket actor.
    pub fn get_storagemarket_code(&self) -> &Cid {
        &self.storagemarket_code
    }

    /// Returns the code CID for the storagemarket actor.
    pub fn get_storagepower_code(&self) -> &Cid {
        &self.storagepower_code
    }

    /// Returns the code CID for the verifiedregistry actor.
    pub fn get_verifiedregistry_code(&self) -> &Cid {
        &self.verifiedregistry_code
    }

    /// Returns the code CID for the datacap actor.
    pub fn get_datacap_code(&self) -> &Cid {
        &self.datacap_code
    }
    
    /// Returns the code CID for the reward actor.
    pub fn get_reward_code(&self) -> &Cid {
        &self.reward_code
    }
}
