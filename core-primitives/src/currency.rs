#![allow(clippy::from_over_into)]

use bstringify::bstringify;
use codec::{Decode, Encode};
use sp_runtime::RuntimeDebug;
use sp_std::{
	convert::{Into, TryFrom},
	prelude::*,
};

#[cfg(feature = "std")]
use serde::{Deserialize, Serialize};

macro_rules! create_currency_id {
    ($(#[$meta:meta])*
	$vis:vis enum TokenSymbol {
        $($(#[$vmeta:meta])* $symbol:ident($name:expr, $deci:literal) = $val:literal,)*
    }) => {
		$(#[$meta])*
		$vis enum TokenSymbol {
			$($(#[$vmeta])* $symbol = $val,)*
		}

		impl TryFrom<u8> for TokenSymbol {
			type Error = ();

			fn try_from(v: u8) -> Result<Self, Self::Error> {
				match v {
					$($val => Ok(TokenSymbol::$symbol),)*
					_ => Err(()),
				}
			}
		}

		impl Into<u8> for TokenSymbol {
			fn into(self) -> u8 {
				match self {
					$(TokenSymbol::$symbol => ($val),)*
				}
			}
		}

		impl TryFrom<Vec<u8>> for CurrencyId {
			type Error = ();
			fn try_from(v: Vec<u8>) -> Result<CurrencyId, ()> {
				match v.as_slice() {
					$(bstringify!($symbol) => Ok(CurrencyId::Token(TokenSymbol::$symbol)),)*
					_ => Err(()),
				}
			}
		}

		impl TokenInfo for CurrencyId {
			fn currency_id(&self) -> Option<u8> {
				match self {
					$(CurrencyId::Token(TokenSymbol::$symbol) => Some($val),)*
					_ => None,
				}
			}
			fn name(&self) -> Option<&str> {
				match self {
					$(CurrencyId::Token(TokenSymbol::$symbol) => Some($name),)*
					_ => None,
				}
			}
			fn symbol(&self) -> Option<&str> {
				match self {
					$(CurrencyId::Token(TokenSymbol::$symbol) => Some(stringify!($symbol)),)*
					_ => None,
				}
			}
			fn decimals(&self) -> Option<u8> {
				match self {
					$(CurrencyId::Token(TokenSymbol::$symbol) => Some($deci),)*
					_ => None,
				}
			}
		}

		$(pub const $symbol: CurrencyId = CurrencyId::Token(TokenSymbol::$symbol);)*

		impl TokenSymbol {
			pub fn get_info() -> Vec<(&'static str, u32)> {
				vec![
					$((stringify!($symbol), $deci),)*
				]
			}
		}

		// #[test]
		// #[ignore]
		// fn generate_token_resources() {
		// 	use crate::TokenSymbol::*;

		// 	#[allow(non_snake_case)]
		// 	#[derive(Serialize, Deserialize)]
		// 	struct Token {
		// 		symbol: String,
		// 		address: EvmAddress,
		// 	}

		// 	let mut tokens = vec![
		// 		$(
		// 			Token {
		// 				symbol: stringify!($symbol).to_string(),
		// 				address: EvmAddress::try_from(CurrencyId::Token(TokenSymbol::$symbol)).unwrap(),
		// 			},
		// 		)*
		// 	];

		// 	let mut lp_tokens = vec![
		// 		Token {
		// 			symbol: "LP_ACA_AUSD".to_string(),
		// 			address: EvmAddress::try_from(CurrencyId::DexShare(DexShare::Token(ACA), DexShare::Token(AUSD))).unwrap(),
		// 		},
		// 		Token {
		// 			symbol: "LP_DOT_AUSD".to_string(),
		// 			address: EvmAddress::try_from(CurrencyId::DexShare(DexShare::Token(DOT), DexShare::Token(AUSD))).unwrap(),
		// 		},
		// 		Token {
		// 			symbol: "LP_LDOT_AUSD".to_string(),
		// 			address: EvmAddress::try_from(CurrencyId::DexShare(DexShare::Token(LDOT), DexShare::Token(AUSD))).unwrap(),
		// 		},
		// 		Token {
		// 			symbol: "LP_RENBTC_AUSD".to_string(),
		// 			address: EvmAddress::try_from(CurrencyId::DexShare(DexShare::Token(RENBTC), DexShare::Token(AUSD))).unwrap(),
		// 		},
		// 		Token {
		// 			symbol: "LP_KAR_KUSD".to_string(),
		// 			address: EvmAddress::try_from(CurrencyId::DexShare(DexShare::Token(KAR), DexShare::Token(KUSD))).unwrap(),
		// 		},
		// 		Token {
		// 			symbol: "LP_KSM_KUSD".to_string(),
		// 			address: EvmAddress::try_from(CurrencyId::DexShare(DexShare::Token(KSM), DexShare::Token(KUSD))).unwrap(),
		// 		},
		// 		Token {
		// 			symbol: "LP_LKSM_KUSD".to_string(),
		// 			address: EvmAddress::try_from(CurrencyId::DexShare(DexShare::Token(LKSM), DexShare::Token(KUSD))).unwrap(),
		// 		},
		// 	];
		// 	tokens.append(&mut lp_tokens);

		// 	frame_support::assert_ok!(std::fs::write("../predeploy-contracts/resources/tokens.json", serde_json::to_string_pretty(&tokens).unwrap()));
		// }
    }
}

create_currency_id! {
    #[derive(Encode, Decode, Eq, PartialEq, Copy, Clone, RuntimeDebug, PartialOrd, Ord)]
	#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
	#[repr(u8)]
    pub enum TokenSymbol {
        CHER("Cherry", 18) = 0,
        PARACHER("Paracherry", 18) = 1,
    }
}

pub trait TokenInfo {
    fn currency_id(&self) -> Option<u8>;
	fn name(&self) -> Option<&str>;
	fn symbol(&self) -> Option<&str>;
	fn decimals(&self) -> Option<u8>;
}

#[derive(Encode, Decode, Eq, PartialEq, Copy, Clone, RuntimeDebug, PartialOrd, Ord)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "std", serde(rename_all = "camelCase"))]
pub enum CurrencyId {
    Token(TokenSymbol)

}

impl CurrencyId {
	pub fn is_token_currency_id(&self) -> bool {
		matches!(self, CurrencyId::Token(_))
	}
}