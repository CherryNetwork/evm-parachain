use std::{collections::BTreeMap, str::FromStr};

use bip32::ExtendedPrivateKey;
use bip39::{Language, Mnemonic, Seed};
use cli_opt::account_key::Secp256k1SecretKey;
use cumulus_primitives_core::ParaId;
use hex_literal::hex;
use libsecp256k1::{PublicKey, PublicKeyFormat};
use log::debug;
use nimbus_primitives::NimbusId;
use parachain_template_runtime::{AccountId, EthereumChainIdConfig};
use sc_chain_spec::{ChainSpecExtension, ChainSpecGroup};
use sc_service::ChainType;
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use sp_core::{ecdsa, Pair, Public, H160, H256, U256};

/// Specialized `ChainSpec` for the normal parachain runtime.
pub type ChainSpec =
	sc_service::GenericChainSpec<parachain_template_runtime::GenesisConfig, Extensions>;

pub type RawChainSpec = sc_service::GenericChainSpec<(), Extensions>;

/// The default XCM version to set in genesis config.
const SAFE_XCM_VERSION: u32 = xcm::prelude::XCM_VERSION;

/// The extensions for the [`ChainSpec`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ChainSpecGroup, ChainSpecExtension)]
#[serde(deny_unknown_fields)]
pub struct Extensions {
	/// The relay chain of the Parachain.
	pub relay_chain: String,
	/// The id of the Parachain.
	pub para_id: u32,
}

impl Extensions {
	/// Try to get the extension from the given `ChainSpec`.
	pub fn try_get(chain_spec: &dyn sc_service::ChainSpec) -> Option<&Self> {
		sc_chain_spec::get_extension(chain_spec.extensions())
	}
}

/// Helper function to derive `num_accounts` child pairs from mnemonics
/// Substrate derive function cannot be used because the derivation is different than Ethereum's
/// https://substrate.dev/rustdocs/v2.0.0/src/sp_core/ecdsa.rs.html#460-470
pub fn derive_bip44_pairs_from_mnemonic<TPublic: Public>(
	mnemonic: &str,
	num_accounts: u32,
) -> Vec<TPublic::Pair> {
	let seed = Mnemonic::from_phrase(mnemonic, Language::English)
		.map(|x| Seed::new(&x, ""))
		.expect("Wrong mnemonic provided");

	let mut childs = Vec::new();
	for i in 0..num_accounts {
		if let Some(child_pair) = format!("m/44'/60'/0'/0/{}", i)
			.parse()
			.ok()
			.and_then(|derivation_path| {
				ExtendedPrivateKey::<Secp256k1SecretKey>::derive_from_path(&seed, &derivation_path)
					.ok()
			})
			.and_then(|extended| {
				TPublic::Pair::from_seed_slice(&extended.private_key().0.serialize()).ok()
			}) {
			childs.push(child_pair);
		} else {
			log::error!("An error ocurred while deriving key {} from parent", i)
		}
	}
	childs
}

/// Helper function to get an `AccountId` from an ECDSA Key Pair.
pub fn get_account_id_from_pair(pair: ecdsa::Pair) -> Option<AccountId> {
	let decompressed = PublicKey::parse_slice(&pair.public().0, Some(PublicKeyFormat::Compressed))
		.ok()?
		.serialize();

	let mut m = [0u8; 64];
	m.copy_from_slice(&decompressed[1..65]);

	Some(H160::from(H256::from_slice(Keccak256::digest(&m).as_slice())).into())
}

/// Function to generate accounts given a mnemonic and a number of child accounts to be generated
/// Defaults to a default mnemonic if no mnemonic is supplied
pub fn generate_accounts(mnemonic: String, num_accounts: u32) -> Vec<AccountId> {
	let childs = derive_bip44_pairs_from_mnemonic::<ecdsa::Public>(&mnemonic, num_accounts);
	debug!("Account Generation");
	childs
		.iter()
		.filter_map(|par| {
			let account = get_account_id_from_pair(par.clone());
			debug!(
				"private_key {} --------> Account {:x?}",
				sp_core::hexdisplay::HexDisplay::from(&par.clone().seed()),
				account
			);
			account
		})
		.collect()
}

/// Helper function to generate a crypto pair from seed
pub fn get_from_seed<TPublic: Public>(seed: &str) -> <TPublic::Pair as Pair>::Public {
	TPublic::Pair::from_string(&format!("//{}", seed), None)
		.expect("static values are valid; qed")
		.public()
}

pub fn development_config(mnemonic: Option<String>, num_accounts: Option<u32>) -> ChainSpec {
	// Give your base currency a unit name and decimal places
	let mut properties = sc_chain_spec::Properties::new();
	properties.insert("tokenSymbol".into(), "PARACHER".into());
	properties.insert("tokenDecimals".into(), 18.into());
	properties.insert("ss58Format".into(), 42.into());

	let parent_mnemonic = mnemonic.unwrap_or_else(|| {
		"bottom drive obey lake curtain smoke basket hold race lonely fit walk".to_string()
	});
	let mut accounts = generate_accounts(parent_mnemonic, num_accounts.unwrap_or(10));
	accounts.push(AccountId::from(hex!("6Be02d1d3665660d22FF9624b7BE0551ee1Ac91b")));
	ChainSpec::from_genesis(
		// Name
		"Cherry EVM Development Testnet",
		// ID
		"cherry_evm_dev",
		ChainType::Development,
		move || {
			testnet_genesis(
				// initial collators
				vec![(accounts[0], get_from_seed::<NimbusId>("Alice"))],
				accounts[0],
				vec![accounts[0], accounts[1], accounts[2], accounts[3]],
				2000.into(),
				42,
			)
		},
		Vec::new(),
		None,
		None,
		None,
		None,
		Extensions {
			relay_chain: "cherry-local".into(), // You MUST set this to the correct network!
			para_id: 2000,
		},
	)
}

pub fn local_testnet_config(para_id: ParaId) -> ChainSpec {
	// Give your base currency a unit name and decimal places
	let mut properties = sc_chain_spec::Properties::new();
	properties.insert("tokenSymbol".into(), "PARACHER".into());
	properties.insert("tokenDecimals".into(), 18.into());
	properties.insert("ss58Format".into(), 42.into());

	ChainSpec::from_genesis(
		// Name
		"Cherry EVM Local Testnet",
		// ID
		"cherry_evm_local",
		ChainType::Local,
		move || {
			testnet_genesis(
				// initial collators.
				vec![
					(
						AccountId::from(hex!("f24FF3a9CF04c71Dbc94D0b566f7A27B94566cac")),
						get_from_seed::<NimbusId>("Alice"),
					),
					(
						AccountId::from(hex!("3Cd0A705a2DC65e5b1E1205896BaA2be8A07c6e0")),
						get_from_seed::<NimbusId>("Bob"),
					),
				],
				AccountId::from(hex!("f24FF3a9CF04c71Dbc94D0b566f7A27B94566cac")),
				vec![
					AccountId::from(hex!("f24FF3a9CF04c71Dbc94D0b566f7A27B94566cac")),
					AccountId::from(hex!("3Cd0A705a2DC65e5b1E1205896BaA2be8A07c6e0")),
					AccountId::from(hex!("798d4Ba9baf0064Ec19eB4F0a1a45785ae9D6DFc")),
					AccountId::from(hex!("773539d4Ac0e786233D90A233654ccEE26a613D9")),
				],
				2000.into(),
				42,
			)
		},
		// Bootnodes
		Vec::new(),
		// Telemetry
		None,
		// Protocol ID
		None,
		// Fork ID
		None,
		// Properties
		Some(properties),
		// Extensions
		Extensions {
			relay_chain: "cherry-local".into(), // You MUST set this to the correct network!
			para_id: 2000,
		},
	)
}

fn testnet_genesis(
	authorities: Vec<(AccountId, NimbusId)>,
	sudo_key: AccountId,
	endowed_accounts: Vec<AccountId>,
	id: ParaId,
	chain_id: u64,
) -> parachain_template_runtime::GenesisConfig {
	parachain_template_runtime::GenesisConfig {
		system: parachain_template_runtime::SystemConfig {
			code: parachain_template_runtime::WASM_BINARY
				.expect("WASM binary was not build, please build it!")
				.to_vec(),
		},
		balances: parachain_template_runtime::BalancesConfig {
			balances: endowed_accounts.iter().cloned().map(|k| (k, 1 << 60)).collect(),
		},
		parachain_info: parachain_template_runtime::ParachainInfoConfig { parachain_id: id },
		author_filter: parachain_template_runtime::AuthorFilterConfig {
			eligible_count: parachain_template_runtime::EligibilityValue::default(),
		},
		potential_author_set: parachain_template_runtime::PotentialAuthorSetConfig {
			mapping: authorities,
		},
		polkadot_xcm: parachain_template_runtime::PolkadotXcmConfig {
			safe_xcm_version: Some(SAFE_XCM_VERSION),
		},
		evm: parachain_template_runtime::EVMConfig {
			accounts: {
				let mut map = BTreeMap::new();
				map.insert(
					// H160 address of Alice dev account
					// Derived from SS58 (42 prefix) address
					// SS58: 5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY
					// hex: 0xd43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d
					// Using the full hex key, truncating to the first 20 bytes (the first 40 hex chars)
					H160::from_str("d43593c715fdd31c61141abd04a99fd6822c8558")
						.expect("internal H160 is valid; qed"),
					fp_evm::GenesisAccount {
						balance: U256::from_str("0xffffffffffffffffffffffffffffffff")
							.expect("internal U256 is valid; qed"),
						code: Default::default(),
						nonce: Default::default(),
						storage: Default::default(),
					},
				);
				map.insert(
					// H160 address of CI test runner account
					H160::from_str("6be02d1d3665660d22ff9624b7be0551ee1ac91b")
						.expect("internal H160 is valid; qed"),
					fp_evm::GenesisAccount {
						balance: U256::from_str("0xffffffffffffffffffffffffffffffff")
							.expect("internal U256 is valid; qed"),
						code: Default::default(),
						nonce: Default::default(),
						storage: Default::default(),
					},
				);
				map.insert(
					// H160 address for benchmark usage
					H160::from_str("1000000000000000000000000000000000000001")
						.expect("internal H160 is valid; qed"),
					fp_evm::GenesisAccount {
						nonce: U256::from(1),
						balance: U256::from(1_000_000_000_000_000_000_000_000u128),
						storage: Default::default(),
						code: vec![0x00],
					},
				);
				map
			},
		},
		ethereum: Default::default(),
		ethereum_chain_id: EthereumChainIdConfig { chain_id },
		sudo: parachain_template_runtime::SudoConfig { key: Some(sudo_key) },
	}
}
