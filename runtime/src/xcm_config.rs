use super::{
	AccountId, Balances, ParachainInfo, ParachainSystem, PolkadotXcm, Runtime, RuntimeCall,
	RuntimeEvent, RuntimeOrigin, WeightToFee, XcmpQueue,
};
use codec::{Decode, Encode};
use frame_support::{
	match_types, parameter_types,
	traits::{Everything, Nothing},
};
use orml_traits::{location::AbsoluteReserveProvider, parameter_type_with_key};
use orml_xcm_support::{IsNativeConcrete, MultiNativeAsset};
use pallet_xcm::XcmPassthrough;
use polkadot_parachain::primitives::Sibling;
use sp_runtime::{traits::{ConstU32, Convert}, WeakBoundedVec};
use xcm::latest::{prelude::*, Weight as XCMWeight};
use xcm_builder::{
	AccountKey20Aliases, AllowKnownQueryResponses, AllowSubscriptionsFrom,
	AllowTopLevelPaidExecutionFrom, CurrencyAdapter, EnsureXcmOrigin, FixedWeightBounds,
	LocationInverter, ParentIsPreset, RelayChainAsNative, SiblingParachainAsNative,
	SiblingParachainConvertsVia, SignedAccountKey20AsNative, SovereignSignedViaLocation,
	TakeWeightCredit, UsingComponents,
};

use currency::{{CurrencyId::Erc20, EvmAddress, TokenSymbol::*},
	origin_conversion::SignedToAccountId20,
};
use xcm_executor::{traits::ShouldExecute, XcmExecutor};
use crate::{Balance, CurrencyId, DealWithFees};
use sp_std::{
	convert::{From, Into, TryFrom},
	vec::Vec,
	prelude::*,
};
use sp_core::Get;

parameter_types! {
	pub const RelayLocation: MultiLocation = MultiLocation::parent();
	pub const RelayNetwork: NetworkId = NetworkId::Any;
	pub RelayChainOrigin: RuntimeOrigin = cumulus_pallet_xcm::Origin::Relay.into();
	pub Ancestry: MultiLocation = Parachain(ParachainInfo::parachain_id().into()).into();
}

/// Type for specifying how a `MultiLocation` can be converted into an `AccountId`. This is used
/// when determining ownership of accounts for asset transacting and when attempting to use XCM
/// `Transact` in order to determine the dispatch Origin.
pub type LocationToAccountId = (
	// The parent (Relay-chain) origin converts to the parent `AccountId`.
	ParentIsPreset<AccountId>,
	// Sibling parachain origins convert to AccountId via the `ParaId::into`.
	SiblingParachainConvertsVia<Sibling, AccountId>,
	// Straight up local `AccountId32` origins just alias directly to `AccountId`.
	AccountKey20Aliases<RelayNetwork, AccountId>,
);

/// Means for transacting assets on this chain.
pub type LocalAssetTransactor = CurrencyAdapter<
	// Use this currency:
	Balances,
	// Use this currency when it is a fungible asset matching the given location or name:
	IsNativeConcrete<CurrencyId, CurrencyIdConvert>,
	// Do a simple punn to convert an AccountId32 MultiLocation into a native chain account ID:
	LocationToAccountId,
	// Our chain's account ID type (we can't get away without mentioning it explicitly):
	AccountId,
	// We don't track any teleports.
	(),
>;

/// This is the type we use to convert an (incoming) XCM origin into a local `Origin` instance,
/// ready for dispatching a transaction with Xcm's `Transact`. There is an `OriginKind` which can
/// biases the kind of local `Origin` it will become.
pub type XcmOriginToTransactDispatchOrigin = (
	// Sovereign account converter; this attempts to derive an `AccountId` from the origin location
	// using `LocationToAccountId` and then turn that into the usual `Signed` origin. Useful for
	// foreign chains who want to have a local sovereign account on this chain which they control.
	SovereignSignedViaLocation<LocationToAccountId, RuntimeOrigin>,
	// Native converter for Relay-chain (Parent) location; will converts to a `Relay` origin when
	// recognized.
	RelayChainAsNative<RelayChainOrigin, RuntimeOrigin>,
	// Native converter for sibling Parachains; will convert to a `SiblingPara` origin when
	// recognized.
	SiblingParachainAsNative<cumulus_pallet_xcm::Origin, RuntimeOrigin>,
	// Native signed account converter; this just converts an `AccountId32` origin into a normal
	// `RuntimeOrigin::Signed` origin of the same 32-byte value.
	SignedAccountKey20AsNative<RelayNetwork, RuntimeOrigin>,
	// Xcm origins can be represented natively under the Xcm pallet's Xcm origin.
	XcmPassthrough<RuntimeOrigin>,
);

parameter_types! {
	// One XCM operation is 1_000_000_000 weight - almost certainly a conservative estimate.
	pub UnitWeightCost: u64 = 1_000_000_000;
	pub const MaxInstructions: u32 = 100;
}

match_types! {
	pub type ParentOrParentsExecutivePlurality: impl Contains<MultiLocation> = {
		MultiLocation { parents: 1, interior: Here } |
		MultiLocation { parents: 1, interior: X1(Plurality { id: BodyId::Executive, .. }) }
	};
}

pub type XcmBarrier = (
	TakeWeightCredit,
	// AllowTopLevelPaidExecutionDescendOriginFirst<Everything>,
	AllowTopLevelPaidExecutionFrom<Everything>,
	// Expected responses are OK.
	AllowKnownQueryResponses<PolkadotXcm>,
	// Subscriptions for version tracking are OK.
	AllowSubscriptionsFrom<Everything>,
);

pub struct XcmConfig;
impl xcm_executor::Config for XcmConfig {
	type RuntimeCall = RuntimeCall;
	type XcmSender = XcmRouter;
	// How to withdraw and deposit an asset.
	type AssetTransactor = LocalAssetTransactor;
	type OriginConverter = XcmOriginToTransactDispatchOrigin;
	type IsReserve = MultiNativeAsset<AbsoluteReserveProvider>;
	type IsTeleporter = (); // Teleporting is disabled.
	type LocationInverter = LocationInverter<Ancestry>;
	type Barrier = XcmBarrier;
	type Weigher = FixedWeightBounds<UnitWeightCost, RuntimeCall, MaxInstructions>;
	type Trader =
		UsingComponents<WeightToFee, RelayLocation, AccountId, Balances, DealWithFees<Runtime>>;
	type ResponseHandler = PolkadotXcm;
	type AssetTrap = PolkadotXcm;
	type AssetClaims = PolkadotXcm;
	type SubscriptionService = PolkadotXcm;
	type CallDispatcher = RuntimeCall;
}

/// No local origins on this chain are allowed to dispatch XCM sends/executions.
pub type LocalOriginToLocation = SignedToAccountId20<RuntimeOrigin, AccountId, RelayNetwork>;

/// The means for routing XCM messages which are not for local execution into the right message
/// queues.
pub type XcmRouter = (
	// Two routers - use UMP to communicate with the relay chain:
	cumulus_primitives_utility::ParentAsUmp<ParachainSystem, PolkadotXcm>,
	// ..and XCMP to communicate with the sibling chains.
	XcmpQueue,
);

impl pallet_xcm::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type SendXcmOrigin = EnsureXcmOrigin<RuntimeOrigin, ()>;
	type XcmRouter = XcmRouter;
	type ExecuteXcmOrigin = EnsureXcmOrigin<RuntimeOrigin, LocalOriginToLocation>;
	type XcmExecuteFilter = Everything;
	// ^ Disable dispatchable execute on the XCM pallet.
	// Needs to be `Everything` for local testing.
	type XcmExecutor = XcmExecutor<XcmConfig>;
	type XcmTeleportFilter = Nothing;
	type XcmReserveTransferFilter = Everything;
	type Weigher = FixedWeightBounds<UnitWeightCost, RuntimeCall, MaxInstructions>;
	type LocationInverter = LocationInverter<Ancestry>;
	type RuntimeOrigin = RuntimeOrigin;
	type RuntimeCall = RuntimeCall;
	const VERSION_DISCOVERY_QUEUE_SIZE: u32 = 100;
	// ^ Override for AdvertisedXcmVersion default
	type AdvertisedXcmVersion = pallet_xcm::CurrentXcmVersion;
}

impl cumulus_pallet_xcm::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type XcmExecutor = XcmExecutor<XcmConfig>;
}

pub const SYSTEM_CONTRACT_ADDRESS_PREFIX: [u8; 9] = [0u8; 9];

pub fn is_system_contract(address: EvmAddress) -> bool {
	address.as_bytes().starts_with(&SYSTEM_CONTRACT_ADDRESS_PREFIX)
}

pub struct CurrencyIdConvert;
impl Convert<CurrencyId, Option<MultiLocation>> for CurrencyIdConvert {
	fn convert(id: CurrencyId) -> Option<MultiLocation> {
		use crate::CurrencyId::Token;
		match id {
			Token(CHER) => Some(MultiLocation::parent()),
			Token(PARACHER) =>
				Some(native_currency_location(ParachainInfo::get().into(), id.encode())),
			Erc20(address) if !is_system_contract(address) =>
				Some(native_currency_location(ParachainInfo::get().into(), id.encode())),
			_ => None,
		}
	}
}
impl Convert<MultiLocation, Option<CurrencyId>> for CurrencyIdConvert {
	fn convert(location: MultiLocation) -> Option<CurrencyId> {
		use currency::TokenSymbol::*;
		use crate::CurrencyId::Token;

		if location == MultiLocation::parent() {
			return Some(Token(CHER))
		}

		match location {
			MultiLocation { parents, interior: X2(Parachain(para_id), GeneralKey(key)) }
				if parents == 1 =>
			{
				match (para_id, &key.into_inner()[..]) {
					(id, key) if id == u32::from(ParachainInfo::get()) => {
						// Acala
						if let Ok(currency_id) = CurrencyId::decode(&mut &*key) {
							// check `currency_id` is cross-chain asset
							match currency_id {
								Token(PARACHER) => Some(currency_id),
								Erc20(address) if !is_system_contract(address) => Some(currency_id),
								_ => None,
							}
						} else {
							// invalid general key
							None
						}
					},
					_ => None,
				}
			},
			// adapt for re-anchor canonical location: https://github.com/paritytech/polkadot/pull/4470
			MultiLocation { parents: 0, interior: X1(GeneralKey(key)) } => {
				let key = &key.into_inner()[..];
				let currency_id = CurrencyId::decode(&mut &*key).ok()?;
				match currency_id {
					Token(PARACHER) => Some(currency_id),
					Erc20(address) if !is_system_contract(address) => Some(currency_id),
					_ => None,
				}
			},
			_ => None,
		}
	}
}
impl Convert<MultiAsset, Option<CurrencyId>> for CurrencyIdConvert {
	fn convert(asset: MultiAsset) -> Option<CurrencyId> {
		if let MultiAsset { id: Concrete(location), .. } = asset {
			Self::convert(location)
		} else {
			None
		}
	}
}

pub struct AccountIdToMultiLocation<AccountId>(sp_std::marker::PhantomData<AccountId>);
impl<AccountId> sp_runtime::traits::Convert<AccountId, MultiLocation>
	for AccountIdToMultiLocation<AccountId>
where
	AccountId: Into<[u8; 20]>,
{
	fn convert(account: AccountId) -> MultiLocation {
		MultiLocation {
			parents: 0,
			interior: X1(AccountKey20 {
				network: NetworkId::Any,
				key: account.into(),
			}),
		}
	}
}

parameter_types! {
	pub SelfLocation: MultiLocation = MultiLocation::new(1, X1(Parachain(ParachainInfo::get().into())));
	pub const BaseXcmWeight: XCMWeight = 100_000_000;
	pub const MaxAssetsForTransfer: usize = 2;
}

parameter_type_with_key! {
	pub ParachainMinFee: |_location: MultiLocation| -> Option<u128> {
		Some(u128::MAX)
	};
}

pub fn native_currency_location(para_id: u32, key: Vec<u8>) -> MultiLocation {
	MultiLocation::new(
		1,
		X2(
			Parachain(para_id),
			GeneralKey(WeakBoundedVec::<u8, ConstU32<32>>::force_from(key, None)),
		),
	)
}

impl orml_xtokens::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type Balance = Balance;
	type CurrencyId = CurrencyId;
	type AccountIdToMultiLocation = AccountIdToMultiLocation<AccountId>;
	type CurrencyIdConvert = CurrencyIdConvert;
	type XcmExecutor = XcmExecutor<XcmConfig>;
	type SelfLocation = SelfLocation;
	type Weigher = FixedWeightBounds<UnitWeightCost, RuntimeCall, MaxInstructions>;
	type BaseXcmWeight = BaseXcmWeight;
	type LocationInverter = LocationInverter<Ancestry>;
	type MaxAssetsForTransfer = MaxAssetsForTransfer;
	type MinXcmFee = ParachainMinFee;
	type MultiLocationsFilter = Everything;
	type ReserveProvider = AbsoluteReserveProvider;
}
