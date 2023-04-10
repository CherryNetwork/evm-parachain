use pallet_evm_precompile_modexp::Modexp;
use pallet_evm_precompile_sha3fips::Sha3FIPS256;
use pallet_evm_precompile_simple::{ECRecover, ECRecoverPublicKey, Identity, Ripemd160, Sha256};
use pallet_evm_precompile_balances_erc20::{Erc20BalancesPrecompile, Erc20Metadata};
use pallet_evm_precompile_blake2::Blake2F;
use pallet_evm_precompile_bn128::{Bn128Add, Bn128Mul, Bn128Pairing};
use pallet_evm_precompile_batch::BatchPrecompile;
use precompile_utils::precompile_set::*;

pub struct NativeErc20Metadata;

/// ERC20 metadata for the native token.
impl Erc20Metadata for NativeErc20Metadata {
	/// Returns the name of the token.
	fn name() -> &'static str {
		"TPCHER token"
	}

	/// Returns the symbol of the token.
	fn symbol() -> &'static str {
		"TPCHER"
	}

	/// Returns the decimals places of the token.
	fn decimals() -> u8 {
		18
	}

	/// Must return `true` only if it represents the main native currency of
	/// the network. It must be the currency used in `pallet_evm`.
	fn is_native_currency() -> bool {
		true
	}
}

type EthereumPrecompilesChecks = (AcceptDelegateCall, CallableByContract, CallableByPrecompile);

#[precompile_utils::precompile_name_from_address]
type CherryPrecompilesAt<R> = (
	// Ethereum precompiles:
	PrecompileAt<AddressU64<1>, ECRecover, EthereumPrecompilesChecks>,
	PrecompileAt<AddressU64<2>, Sha256, EthereumPrecompilesChecks>,
	PrecompileAt<AddressU64<3>, Ripemd160, EthereumPrecompilesChecks>,
	PrecompileAt<AddressU64<4>, Identity, EthereumPrecompilesChecks>,
	PrecompileAt<AddressU64<5>, Modexp, EthereumPrecompilesChecks>,
	PrecompileAt<AddressU64<6>, Bn128Add, EthereumPrecompilesChecks>,
	PrecompileAt<AddressU64<7>, Bn128Mul, EthereumPrecompilesChecks>,
	PrecompileAt<AddressU64<8>, Bn128Pairing, EthereumPrecompilesChecks>,
	PrecompileAt<AddressU64<9>, Blake2F, EthereumPrecompilesChecks>,
	PrecompileAt<AddressU64<1024>, Sha3FIPS256, (CallableByContract, CallableByPrecompile)>,
	PrecompileAt<AddressU64<1025>, ECRecoverPublicKey, (CallableByContract, CallableByPrecompile)>,
	// Moonbeam precompiles: 
	PrecompileAt<
		AddressU64<2048>,
		Erc20BalancesPrecompile<R, NativeErc20Metadata>,
		(CallableByContract, CallableByPrecompile),
	>,
	PrecompileAt<
		AddressU64<2049>,
		BatchPrecompile<R>,
		(
			SubcallWithMaxNesting<2>,
			// Batch is the only precompile allowed to call Batch.
			CallableByPrecompile<OnlyFrom<AddressU64<2049>>>,
		),
	>,
);

pub type CherryPrecompiles<R> = PrecompileSetBuilder<
	R,
	(
		// Skip precompiles if out of range.
		PrecompilesInRangeInclusive<(AddressU64<1>, AddressU64<2049>), CherryPrecompilesAt<R>>,
	),
>;

// pub struct CherryPrecompiles<R>(PhantomData<R>);

// impl<R> CherryPrecompiles<R>
// where
// 	R: pallet_evm::Config,
// {
// 	pub fn new() -> Self {
// 		Self(Default::default())
// 	}
// 	pub fn used_addresses() -> [H160; 8] {
// 		[hash(1), hash(2), hash(3), hash(4), hash(5), hash(1024), hash(1025), hash(2048)]
// 	}
// }
// impl<R> PrecompileSet for CherryPrecompiles<R>
// where
// 	R: pallet_evm::Config,
// {
// 	fn execute(&self, handle: &mut impl PrecompileHandle) -> Option<PrecompileResult> {
// 		match handle.code_address() {
// 			// Ethereum precompiles :
// 			a if a == hash(1) => Some(ECRecover::execute(handle)),
// 			a if a == hash(2) => Some(Sha256::execute(handle)),
// 			a if a == hash(3) => Some(Ripemd160::execute(handle)),
// 			a if a == hash(4) => Some(Identity::execute(handle)),
// 			a if a == hash(5) => Some(Modexp::execute(handle)),
// 			// Non-Frontier specific nor Ethereum precompiles :
// 			a if a == hash(1024) => Some(Sha3FIPS256::execute(handle)),
// 			a if a == hash(1025) => Some(ECRecoverPublicKey::execute(handle)),
// 			// Moonbeam precompiles : 
// 			a if a == hash(2048) => Some(Erc20BalancesPrecompile::execute(handle)),
// 			_ => None,
// 		}
// 	}

// 	fn is_precompile(&self, address: H160) -> bool {
// 		Self::used_addresses().contains(&address)
// 	}
// }

// fn hash(a: u64) -> H160 {
// 	H160::from_low_u64_be(a)
// }
