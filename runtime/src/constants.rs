/// Fee-related
pub mod fee {
	use frame_support::weights::{
		constants::{ExtrinsicBaseWeight, WEIGHT_PER_SECOND},
	};
    use super::{Balance, CurrencyId};
	use sp_runtime::Perbill;

	/// The block saturation level. Fees will be updates based on this value.
	pub const TARGET_BLOCK_FULLNESS: Perbill = Perbill::from_percent(25);

	// TODO: make those const fn
    pub fn dollar(currency_id: CurrencyId) -> Balance {
	    10u128.saturating_pow(currency_id.decimals().expect("Not support Erc20 decimals").into())
    }

    pub fn cent(currency_id: CurrencyId) -> Balance {
	    dollar(currency_id) / 100
    }

    fn base_tx_in_cher() -> Balance {
        cent(CHER) / 10
    }

	pub fn cher_per_second() -> u128 {
		let base_weight = Balance::from(ExtrinsicBaseWeight::get());
		let base_tx_per_second = (WEIGHT_PER_SECOND as u128) / base_weight;
		let cher_per_second = base_tx_per_second * base_tx_in_cher() / 100;
	}
}