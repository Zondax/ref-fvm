// Copyright 2021-2023 Protocol Labs
// SPDX-License-Identifier: Apache-2.0, MIT
use anyhow::{anyhow, Context, Result};
use cid::Cid;
use fvm::call_manager::DefaultCallManager;
use fvm::engine::EnginePool;
use fvm::executor::DefaultExecutor;
use fvm::externs::Externs;
use fvm::machine::{DefaultMachine, Machine, MachineContext, NetworkConfig};
use fvm::state_tree::{ActorState, StateTree};
use fvm::{init_actor, system_actor, storagemarket_actor, storagepower_actor, DefaultKernel};
use fvm_ipld_blockstore::{Block, Blockstore};
use fvm_ipld_encoding::{ser, CborStore};
use fvm_shared::address::{Address, Protocol};
use fvm_shared::econ::TokenAmount;
use fvm_shared::state::StateTreeVersion;
use fvm_shared::version::NetworkVersion;
use fvm_shared::{ActorID, IPLD_RAW};
use lazy_static::lazy_static;
use libsecp256k1::{PublicKey, SecretKey};
use multihash::Code;

use crate::reward_actor;
use crate::verifiedregistry_actor;
use crate::datacap_actor;
use crate::builtin::{fetch_builtin_code_cid, set_eam_actor, set_init_actor, set_sys_actor, set_storagemarket_actor, set_storagepower_actor, set_verifiedregistry_actor, set_datacap_actor, set_reward_actor};
use crate::error::Error::{FailedToFlushTree, NoManifestInformation};

const DEFAULT_BASE_FEE: u64 = 100;

lazy_static! {
    pub static ref INITIAL_ACCOUNT_BALANCE: TokenAmount = TokenAmount::from_atto(10000);
}

pub trait Store: Blockstore + Sized + 'static {}

pub type IntegrationExecutor<B, E> =
    DefaultExecutor<DefaultKernel<DefaultCallManager<DefaultMachine<B, E>>>>;

pub type Account = (ActorID, Address);

pub struct Tester<B: Blockstore + 'static, E: Externs + 'static> {
    // Network version used in the test
    nv: NetworkVersion,
    // Builtin actors root Cid used in the Machine
    builtin_actors: Cid,
    // Accounts actor cid
    accounts_code_cid: Cid,
    // Placeholder code cid.
    placeholder_code_cid: Cid,
    // Custom code cid deployed by developer
    code_cids: Vec<Cid>,
    // Executor used to interact with deployed actors.
    pub executor: Option<IntegrationExecutor<B, E>>,
    // State tree constructed before instantiating the Machine
    pub state_tree: Option<StateTree<B>>,
}

impl<B, E> Tester<B, E>
where
    B: Blockstore,
    E: Externs,
{
    pub fn new(
        nv: NetworkVersion,
        stv: StateTreeVersion,
        builtin_actors: Cid,
        blockstore: B,
    ) -> Result<Self> {
        let (manifest_version, manifest_data_cid): (u32, Cid) =
            match blockstore.get_cbor(&builtin_actors)? {
                Some((manifest_version, manifest_data)) => (manifest_version, manifest_data),
                None => return Err(NoManifestInformation(builtin_actors).into()),
            };

        // Get sys and init actors code cid
        let (sys_code_cid, init_code_cid, accounts_code_cid, placeholder_code_cid, eam_code_cid, market_code_cid, power_code_cid, verifreg_code_cid, datacap_code_cid, reward_code_cid) =
            fetch_builtin_code_cid(&blockstore, &manifest_data_cid, manifest_version)?;

        // Initialize state tree
        let init_state = init_actor::State::new_test(&blockstore);
        let storagemarket_state = storagemarket_actor::State::new_test(&blockstore);
        let storagepower_state = storagepower_actor::State::new_test(&blockstore);
        let verifreg_state = verifiedregistry_actor::State::new_test(&blockstore, Address::new_id(199));
        // DATACAP actor's governor has to be Verifreg (actor id 6)
        let datacap_state = datacap_actor::State::new_test(&blockstore, Address::new_id(6));
        let reward_state = reward_actor::State::new_test();
        let mut state_tree = StateTree::new(blockstore, stv).map_err(anyhow::Error::from)?;

        // Deploy init, sys, and eam actors
        let sys_state = system_actor::State { builtin_actors };

        set_sys_actor(&mut state_tree, sys_state, sys_code_cid)?;
        set_init_actor(&mut state_tree, init_code_cid, init_state)?;
        set_eam_actor(&mut state_tree, eam_code_cid)?;
        set_storagemarket_actor(&mut state_tree, market_code_cid, storagemarket_state)?;
        set_storagepower_actor(&mut state_tree, power_code_cid, storagepower_state)?;
        set_verifiedregistry_actor(&mut state_tree, verifreg_code_cid, verifreg_state)?;
        set_datacap_actor(&mut state_tree, datacap_code_cid, datacap_state)?;
        set_reward_actor(&mut state_tree, reward_code_cid, reward_state)?;

        Ok(Tester {
            nv,
            builtin_actors,
            executor: None,
            code_cids: vec![],
            state_tree: Some(state_tree),
            accounts_code_cid,
            placeholder_code_cid,
        })
    }

    /// Creates new accounts in the testing context
    /// Inserts the specified number of accounts in the state tree, all with 1000 FIL，returning their IDs and Addresses.
    pub fn create_accounts<const N: usize>(&mut self) -> Result<[Account; N]> {
        use rand::SeedableRng;

        let rng = &mut rand_chacha::ChaCha8Rng::seed_from_u64(8);

        let mut ret: [Account; N] = [(0, Address::default()); N];
        for account in ret.iter_mut().take(N) {
            let priv_key = SecretKey::random(rng);
            *account = self.make_secp256k1_account(priv_key, INITIAL_ACCOUNT_BALANCE.clone())?;
        }
        Ok(ret)
    }

    pub fn set_account_sequence(&mut self, id: ActorID, new_sequence: u64) -> anyhow::Result<()> {
        let state_tree = self
            .state_tree
            .as_mut()
            .ok_or_else(|| anyhow!("Expected state tree in set_account_sequence."))?;

        let mut state = state_tree
            .get_actor(id)?
            .ok_or_else(|| anyhow!("Can't set sequence of account that doesn't exist."))?;

        state.sequence = new_sequence;

        state_tree.set_actor(id, state).map_err(anyhow::Error::from)
    }

    pub fn create_placeholder(&mut self, address: &Address, init_balance: TokenAmount) -> Result<ActorID> {
        assert_eq!(address.protocol(), Protocol::Delegated);

        let state_tree = self
            .state_tree
            .as_mut()
            .ok_or_else(|| anyhow!("unable get state tree"))?;

        let id = state_tree.register_new_address(address).unwrap();
        let state: [u8; 32] = [0; 32];

        let cid = state_tree.store().put_cbor(&state, Code::Blake2b256)?;

        let actor_state = ActorState {
            code: self.placeholder_code_cid,
            state: cid,
            sequence: 0,
            balance: init_balance,
            delegated_address: Some(*address),
        };

        state_tree
            .set_actor(id, actor_state)
            .map_err(anyhow::Error::from)?;

        Ok(id)
    }

    /// Set a new state in the state tree
    pub fn set_state<S: ser::Serialize>(&mut self, state: &S) -> Result<Cid> {
        // Put state in tree
        let state_cid = self
            .state_tree
            .as_mut()
            .unwrap()
            .store()
            .put_cbor(state, Code::Blake2b256)?;

        Ok(state_cid)
    }

    /// Set a new at a given address, provided with a given token balance
    /// and returns the CodeCID of the installed actor
    pub fn set_actor_from_bin(
        &mut self,
        wasm_bin: &[u8],
        state_cid: Cid,
        actor_address: Address,
        balance: TokenAmount,
    ) -> Result<Cid> {
        // Register actor address (unless it's an ID address)
        let actor_id = match actor_address.id() {
            Ok(id) => id,
            Err(_) => self
                .state_tree
                .as_mut()
                .unwrap()
                .register_new_address(&actor_address)
                .unwrap(),
        };

        // Put the WASM code into the blockstore.
        let code_cid = put_wasm_code(self.state_tree.as_mut().unwrap().store(), wasm_bin)?;

        // Add code cid to list of deployed contract
        self.code_cids.push(code_cid);

        // Initialize actor state
        let actor_state = ActorState::new(
            code_cid,
            state_cid,
            balance,
            1,
            match actor_address.protocol() {
                Protocol::ID | Protocol::Actor => None,
                _ => Some(actor_address),
            },
        );

        // Create actor
        self.state_tree
            .as_mut()
            .unwrap()
            .set_actor(actor_id, actor_state)
            .map_err(anyhow::Error::from)?;

        Ok(code_cid)
    }

    /// Sets the Machine and the Executor in our Tester structure.
    pub fn instantiate_machine(&mut self, externs: E) -> Result<()> {
        self.instantiate_machine_with_config(externs, |_| (), |_| ())
    }

    /// Sets the Machine and the Executor in our Tester structure.
    ///
    /// The `configure_nc` and `configure_mc` functions allows the caller to adjust the
    /// `NetworkConfiguration` and `MachineContext` before they are used to instantiate
    /// the rest of the components.
    pub fn instantiate_machine_with_config<F, G>(
        &mut self,
        externs: E,
        configure_nc: F,
        configure_mc: G,
    ) -> Result<()>
    where
        F: FnOnce(&mut NetworkConfig),
        G: FnOnce(&mut MachineContext),
    {
        // Take the state tree and leave None behind.
        let mut state_tree = self.state_tree.take().unwrap();

        // Calculate the state root.
        let state_root = state_tree
            .flush()
            .map_err(anyhow::Error::from)
            .context(FailedToFlushTree)?;

        // Consume the state tree and take the blockstore.
        let blockstore = state_tree.into_store();

        let mut nc = NetworkConfig::new(self.nv);
        nc.override_actors(self.builtin_actors);
        nc.enable_actor_debugging();

        // Custom configuration.
        configure_nc(&mut nc);

        let mut mc = nc.for_epoch(0, 0, state_root);
        mc.set_base_fee(TokenAmount::from_atto(DEFAULT_BASE_FEE))
            .enable_tracing();

        // Custom configuration.
        configure_mc(&mut mc);

        let engine = EnginePool::new_default((&mc.network.clone()).into())?;
        engine.acquire().preload(&blockstore, &self.code_cids)?;

        let machine = DefaultMachine::new(&mc, blockstore, externs)?;

        let executor =
            DefaultExecutor::<DefaultKernel<DefaultCallManager<DefaultMachine<B, E>>>>::new(
                engine, machine,
            )?;

        self.executor = Some(executor);

        Ok(())
    }

    /// Get blockstore
    pub fn blockstore(&self) -> &dyn Blockstore {
        if self.executor.is_some() {
            self.executor.as_ref().unwrap().blockstore()
        } else {
            self.state_tree.as_ref().unwrap().store()
        }
    }

    /// Put account with specified private key and balance
    pub fn make_secp256k1_account(
        &mut self,
        priv_key: SecretKey,
        init_balance: TokenAmount,
    ) -> Result<Account> {
        let pub_key = PublicKey::from_secret_key(&priv_key);
        let pub_key_addr = Address::new_secp256k1(&pub_key.serialize())?;

        let state_tree = self
            .state_tree
            .as_mut()
            .ok_or_else(|| anyhow!("unable get state tree"))?;
        let assigned_addr = state_tree.register_new_address(&pub_key_addr).unwrap();
        let state = fvm::account_actor::State {
            address: pub_key_addr,
        };

        let cid = state_tree.store().put_cbor(&state, Code::Blake2b256)?;

        let actor_state = ActorState {
            code: self.accounts_code_cid,
            state: cid,
            sequence: 0,
            balance: init_balance,
            delegated_address: None,
        };

        state_tree
            .set_actor(assigned_addr, actor_state)
            .map_err(anyhow::Error::from)?;
        Ok((assigned_addr, pub_key_addr))
    }
}
/// Inserts the WASM code for the actor into the blockstore.
fn put_wasm_code(blockstore: &impl Blockstore, wasm_binary: &[u8]) -> Result<Cid> {
    let cid = blockstore.put(
        Code::Blake2b256,
        &Block {
            codec: IPLD_RAW,
            data: wasm_binary,
        },
    )?;
    Ok(cid)
}
