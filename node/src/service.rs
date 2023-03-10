//! Service and ServiceFactory implementation. Specialized wrapper over substrate service.

// std
use std::{
	collections::BTreeMap,
	future,
	sync::{Arc, Mutex},
	time::Duration,
};

use fc_mapping_sync::{MappingSyncWorker, SyncStrategy};
use fc_rpc_core::types::{FeeHistoryCache, FilterPool};
use fc_consensus::FrontierBlockImport;
use fc_db::DatabaseSource;
use futures::StreamExt;
use maplit::hashmap;
use jsonrpsee::RpcModule;
use nimbus_consensus::NimbusManualSealConsensusDataProvider;
use nimbus_consensus::{BuildNimbusConsensusParams, NimbusConsensus};
use cumulus_client_cli::CollatorOptions;
// Local Runtime Types
use parachain_template_runtime::{
	opaque::Block, AccountId, Balance, Hash, Index as Nonce, RuntimeApi,
};

// Cumulus Imports
use cumulus_client_consensus_aura::{AuraConsensus, BuildAuraConsensusParams, SlotProportion};
use cumulus_client_consensus_common::{
	ParachainBlockImport as TParachainBlockImport, ParachainConsensus,
};
use cumulus_client_network::BlockAnnounceValidator;
use cumulus_client_service::{
	prepare_node_config, start_collator, start_full_node,
	StartCollatorParams, StartFullNodeParams,
};
use cumulus_primitives_core::ParaId;
use cumulus_relay_chain_interface::{RelayChainError, RelayChainInterface};

use sc_client_api::BlockchainEvents;
// Substrate Imports
use sc_consensus::ImportQueue;
use sc_executor::NativeElseWasmExecutor;
use sc_network::NetworkService;
use sc_network_common::service::NetworkBlock;
use sc_service::{
	BasePath, Configuration, PartialComponents, TFullBackend, TFullClient, TaskManager,
};
use sc_telemetry::{Telemetry, TelemetryHandle, TelemetryWorker, TelemetryWorkerHandle};
use sp_api::ConstructRuntimeApi;
use sp_keystore::SyncCryptoStorePtr;
use sp_runtime::traits::BlakeTwo256;
use substrate_prometheus_endpoint::Registry;

type FullClient<RuntimeApi, Executor> =
	TFullClient<Block, RuntimeApi, NativeElseWasmExecutor<Executor>>;
type FullBackend = TFullBackend<Block>;

/// A trait that must be implemented by all moon* runtimes executors.
///
/// This feature allows, for instance, to customize the client extensions according to the type
/// of network.
/// For the moment, this feature is only used to specify the first block compatible with
/// ed25519-zebra, but it could be used for other things in the future.
pub trait ExecutorT: sc_executor::NativeExecutionDispatch {
	/// The host function ed25519_verify has changed its behavior in the substrate history,
	/// because of the change from lib ed25519-dalek to lib ed25519-zebra.
	/// Some networks may have old blocks that are not compatible with ed25519-zebra,
	/// for these networks this function should return the 1st block compatible with the new lib.
	/// If this function returns None (default behavior), it implies that all blocks are compatible
	/// with the new lib (ed25519-zebra).
	fn first_block_number_compatible_with_ed25519_zebra() -> Option<u32> {
		None
	}
}

/// Native executor type.
pub struct ParachainNativeExecutor;

impl sc_executor::NativeExecutionDispatch for ParachainNativeExecutor {
	type ExtendHostFunctions = frame_benchmarking::benchmarking::HostFunctions;

	fn dispatch(method: &str, data: &[u8]) -> Option<Vec<u8>> {
		parachain_template_runtime::api::dispatch(method, data)
	}

	fn native_version() -> sc_executor::NativeVersion {
		parachain_template_runtime::native_version()
	}
}

pub fn frontier_database_dir(config: &Configuration) -> std::path::PathBuf {
	config
		.base_path
		.as_ref()
		.map(|base_path| base_path.config_dir(config.chain_spec.id()))
		.unwrap_or_else(|| {
			BasePath::from_project("", "", &crate::cli::Cli::executable_name())
				.config_dir(config.chain_spec.id())
		});
	config_dir.join("frontier").join(path)
}

impl ExecutorT for ParachainNativeExecutor {
	fn first_block_number_compatible_with_ed25519_zebra() -> Option<u32> {
		Some(2_000_000)
	}
}

pub fn open_frontier_backend<C>(
	client: Arc<C>,
	config: &Configuration,
) -> Result<Arc<fc_db::Backend<Block>>, String>
where
	C: sp_blockchain::HeaderBackend<Block>,
{
	Ok(Arc::new(fc_db::Backend::<Block>::new(
		client,
		&fc_db::DatabaseSettings {
			source: match config.database {
				DatabaseSource::RocksDb { .. } => DatabaseSource::RocksDb {
					path: frontier_database_dir(config, "db"),
					cache_size: 0,
				},
				DatabaseSource::ParityDb { .. } => DatabaseSource::ParityDb {
					path: frontier_database_dir(config, "paritydb"),
				},
				DatabaseSource::Auto { .. } => DatabaseSource::Auto {
					rocksdb_path: frontier_database_dir(config, "db"),
					paritydb_path: frontier_database_dir(config, "paritydb"),
					cache_size: 0,
				},
				_ => {
					return Err("Supported db sources: `rocksdb` | `paritydb` | `auto`".to_string())
				}
			},
		},
	)?))
}

use sp_runtime::traits::Percent;
use sp_trie::PrefixedMemoryDB;

pub const SOFT_DEADLINE_PERCENT: Percent = Percent::from_percent(100);

/// Builds a new object suitable for chain operations.
#[allow(clippy::type_complexity)]
pub fn new_chain_ops(
	config: &mut Configuration,
) -> Result<
	(
		Arc<Client>,
		Arc<FullBackend>,
		sc_consensus::BasicQueue<Block, PrefixedMemoryDB<BlakeTwo256>>,
		TaskManager,
	),
	ServiceError,
> {
	new_chain_ops_inner::<parachain_template_runtime::RuntimeApi, ParachainNativeExecutor>(config)
}

#[allow(clippy::type_complexity)]
fn new_chain_ops_inner<RuntimeApi, Executor>(
	mut config: &mut Configuration,
) -> Result<
	(
		Arc<Client>,
		Arc<FullBackend>,
		sc_consensus::BasicQueue<Block, PrefixedMemoryDB<BlakeTwo256>>,
		TaskManager,
	),
	ServiceError,
>
where
	Client: From<Arc<crate::FullClient<RuntimeApi, Executor>>>,
	RuntimeApi:
		ConstructRuntimeApi<Block, FullClient<RuntimeApi, Executor>> + Send + Sync + 'static,
	RuntimeApi::RuntimeApi:
		RuntimeApiCollection<StateBackend = sc_client_api::StateBackendFor<FullBackend, Block>>,
	Executor: ExecutorT + 'static,
{
	config.keystore = sc_service::config::KeystoreConfig::InMemory;
	let PartialComponents {
		client,
		backend,
		import_queue,
		task_manager,
		..
	} = new_partial::<RuntimeApi, Executor>(config, config.chain_spec.is_dev())?;
	Ok((
		Arc::new(Client::from(client)),
		backend,
		import_queue,
		task_manager,
	))
}

// If we're using prometheus, use a registry with a prefix of `moonbeam`.
fn set_prometheus_registry(config: &mut Configuration) -> Result<(), ServiceError> {
	if let Some(PrometheusConfig { registry, .. }) = config.prometheus_config.as_mut() {
		let labels = hashmap! {
			"chain".into() => config.chain_spec.id().into(),
		};
		*registry = Registry::new_custom(Some("cherry".into()), Some(labels))?;
	}

	Ok(())
}

/// Starts a `ServiceBuilder` for a full service.
///
/// Use this macro if you don't actually need the full service, but just the builder in order to
/// be able to perform chain operations.
#[allow(clippy::type_complexity)]
pub fn new_partial<RuntimeApi, Executor>(
	config: &mut Configuration,
	dev_service: bool,
) -> Result<
	PartialComponents<
		FullClient<RuntimeApi, Executor>,
		FullBackend,
		(),
		sc_consensus::DefaultImportQueue<Block, FullClient<RuntimeApi, Executor>>,
		sc_transaction_pool::FullPool<Block, FullClient<RuntimeApi, Executor>>,
		(
			FrontierBlockImport<
				Block,
				Arc<FullClient<RuntimeApi, Executor>>,
				FullClient<RuntimeApi, Executor>,
			>,
			Option<FilterPool>,
			Option<Telemetry>,
			Option<TelemetryWorkerHandle>,
			Arc<fc_db::Backend<Block>>,
			FeeHistoryCache,
		),
	>,
	ServiceError,
>
where
	RuntimeApi: ConstructRuntimeApi<Block, FullClient<RuntimeApi, Executor>>
		+ Send
		+ Sync
		+ 'static,
	RuntimeApi::RuntimeApi: sp_transaction_pool::runtime_api::TaggedTransactionQueue<Block>
		+ sp_api::ApiExt<Block>
		+ sp_block_builder::BlockBuilder<Block>
		+ substrate_frame_rpc_system::AccountNonceApi<Block, AccountId, Index>
		+ pallet_transaction_payment_rpc_runtime_api::TransactionPaymentApi<Block, Balance>
		+ sp_api::Metadata<Block>
		+ sp_offchain::OffchainWorkerApi<Block>
		+ sp_session::SessionKeys<Block>
		+ fp_rpc::ConvertTransactionRuntimeApi<Block>
		+ fp_rpc::EthereumRuntimeRPCApi<Block>
		+ cumulus_primitives_core::CollectCollationInfo<Block>,
	Executor: sc_executor::NativeExecutionDispatch + 'static,
{
	set_prometheus_registry(config)?;

	// Use ethereum style for subscription ids
	config.rpc_id_provider = Some(Box::new(fc_rpc::EthereumSubIdProvider));

	let telemetry = config
		.telemetry_endpoints
		.clone()
		.filter(|x| !x.is_empty())
		.map(|endpoints| -> Result<_, sc_telemetry::Error> {
			let worker = TelemetryWorker::new(16)?;
			let telemetry = worker.handle().new_telemetry(endpoints);
			Ok((worker, telemetry))
		})
		.transpose()?;

	let executor = sc_executor::NativeElseWasmExecutor::<Executor>::new(
		config.wasm_method,
		config.default_heap_pages,
		config.max_runtime_instances,
		config.runtime_cache_size,
	);

	let (client, backend, keystore_container, task_manager) =
		sc_service::new_full_parts::<Block, RuntimeApi, _>(
			config,
			telemetry.as_ref().map(|(_, telemetry)| telemetry.handle()),
			executor,
		)?;

	if let Some(block_number) = Executor::first_block_number_compatible_with_ed25519_zebra() {
		client
			.execution_extensions()
			.set_extensions_factory(sc_client_api::execution_extensions::ExtensionBeforeBlock::<
			Block,
			sp_io::UseDalekExt,
		>::new(block_number));
	}

	let client = Arc::new(client);

	let telemetry_worker_handle = telemetry.as_ref().map(|(worker, _)| worker.handle());

	let telemetry = telemetry.map(|(worker, telemetry)| {
		task_manager.spawn_handle().spawn("telemetry", None, worker.run());
		telemetry
	});

	let transaction_pool = sc_transaction_pool::BasicPool::new_full(
		config.transaction_pool.clone(),
		config.role.is_authority().into(),
		config.prometheus_registry(),
		task_manager.spawn_essential_handle(),
		client.clone(),
	);

	let filter_pool: Option<FilterPool> = Some(Arc::new(Mutex::new(BTreeMap::new())));
	let fee_history_cache: FeeHistoryCache = Arc::new(Mutex::new(BTreeMap::new()));

	let frontier_backend = open_frontier_backend(client.clone(), config)?;

	let frontier_block_import =
		FrontierBlockImport::new(client.clone(), client.clone(), frontier_backend.clone());

	// Depending whether we are
	let import_queue = nimbus_consensus::import_queue(
		client.clone(),
		frontier_block_import.clone(),
		move |_, _| async move {
			let time = sp_timestamp::InherentDataProvider::from_system_time();

			Ok((time,))
		},
		&task_manager.spawn_essential_handle(),
		config.prometheus_registry(),
		!dev_service,
	)?;

	Ok(PartialComponents {
		backend,
		client,
		import_queue,
		keystore_container,
		task_manager,
		transaction_pool,
		select_chain: (),
		other: (
			frontier_block_import,
			filter_pool,
			telemetry,
			telemetry_worker_handle,
			frontier_backend,
			fee_history_cache,
		),
	})
}

async fn build_relay_chain_interface(
	polkadot_config: Configuration,
	parachain_config: &Configuration,
	telemetry_worker_handle: Option<TelemetryWorkerHandle>,
	task_manager: &mut TaskManager,
	collator_options: CollatorOptions,
	hwbench: Option<sc_sysinfo::HwBench>,
) -> RelayChainResult<(
	Arc<(dyn RelayChainInterface + 'static)>,
	Option<CollatorPair>,
)> {
	if !collator_options.relay_chain_rpc_urls.is_empty() {
		build_minimal_relay_chain_node(
			polkadot_config,
			task_manager,
			collator_options.relay_chain_rpc_urls,
		)
		.await
	} else {
		build_inprocess_relay_chain(
			polkadot_config,
			parachain_config,
			telemetry_worker_handle,
			task_manager,
			hwbench,
		)
	}
}

/// Start a node with the given parachain `Configuration` and relay chain `Configuration`.
///
/// This is the actual implementation that is abstract over the executor and the runtime api.
#[sc_tracing::logging::prefix_logs_with("Parachain")]
async fn start_node<RuntimeApi, Executor, BIC>(
	parachain_config: Configuration,
	polkadot_config: Configuration,
	para_id: ParaId,
	rpc_config: RpcConfig,
	hwbench: Option<sc_sysinfo::HwBench>,
	build_consensus: BIC,
) -> sc_service::error::Result<(
	TaskManager,
	Arc<FullClient<RuntimeApi, Executor>>
)>
where
	RuntimeApi: ConstructRuntimeApi<Block, FullClient<RuntimeApi, Executor>>
		+ Send
		+ Sync
		+ 'static,
	RuntimeApi::RuntimeApi: sp_transaction_pool::runtime_api::TaggedTransactionQueue<Block>
		+ sp_api::ApiExt<Block>
		+ sp_block_builder::BlockBuilder<Block>
		+ substrate_frame_rpc_system::AccountNonceApi<Block, AccountId, Index>
		+ pallet_transaction_payment_rpc_runtime_api::TransactionPaymentApi<Block, Balance>
		+ sp_api::Metadata<Block>
		+ sp_offchain::OffchainWorkerApi<Block>
		+ sp_session::SessionKeys<Block>
		+ fp_rpc::ConvertTransactionRuntimeApi<Block>
		+ fp_rpc::EthereumRuntimeRPCApi<Block>
		+ cumulus_primitives_core::CollectCollationInfo<Block>,
	Executor: sc_executor::NativeExecutionDispatch + 'static,
	BIC: FnOnce(
		Arc<TFullClient<Block, RuntimeApi, NativeElseWasmExecutor<Executor>>>,
		Arc<sc_client_db::Backend<Block>>,
		Option<&Registry>,
		Option<TelemetryHandle>,
		&TaskManager,
		Arc<dyn RelayChainInterface>,
		Arc<
			sc_transaction_pool::FullPool<
				Block,
				TFullClient<Block, RuntimeApi, NativeElseWasmExecutor<Executor>>,
			>,
		>,
		Arc<NetworkService<Block, Hash>>,
		SyncCryptoStorePtr,
		bool,
	) -> Result<Box<dyn ParachainConsensus<Block>>, sc_service::Error>,
{
	start_node_impl(
		parachain_config,
		polkadot_config,
		para_id,
		rpc_config,
		hwbench,
		|
			client,
			backend,
			prometheus_registry,
			telemetry,
			task_manager,
			relay_chain_interface,
			transaction_pool,
			_sync_oracle,
			keystore,
			force_authoring
		| {
			let mut proposer_factory = sc_basic_authorship::ProposerFactory::with_proof_recording(
				task_manager.spawn_handle(),
				client.clone(),
				transaction_pool,
				prometheus_registry,
				telemetry.clone(),
			);
			proposer_factory.set_soft_deadline(SOFT_DEADLINE_PERCENT);

			let provider = move |_, (relay_parent, validation_data, _author_id)| {
				let relay_chain_interface = relay_chain_interface.clone();
				async move {
					let parachain_inherent =
						cumulus_primitives_parachain_inherent::ParachainInherentData::create_at(
							relay_parent,
							&relay_chain_interface,
							&validation_data,
							id,
						)
						.await;

					let time = sp_timestamp::InherentDataProvider::from_system_time();

					let parachain_inherent = parachain_inherent.ok_or_else(|| {
						Box::<dyn std::error::Error + Send + Sync>::from(
							"Failed to create parachain inherent",
						)
					})?;

					let author = nimbus_primitives::InherentDataProvider;

					let randomness = nimbus_primitives::InherentDataProvider;

					Ok((time, parachain_inherent, author, randomness))
				}
			};
			let client_clone = client.clone();
			let keystore_clone = keystore.clone();

			Ok(NimbusConsensus::build(BuildNimbusConsensusParams {
				para_id: id,
				proposer_factory,
				block_import: client.clone(),
				backend,
				parachain_client: client.clone(),
				keystore,
				skip_prediction: force_authoring,
				create_inherent_data_providers: provider,
				additional_digests_provider: maybe_provide_vrf_digest,
			}))
		},
	)
	.await
}

/// Builds a new development service. This service uses manual seal, and mocks
/// the parachain inherent.
pub fn new_dev<RuntimeApi, Executor>(
	mut config: Configuration,
	_author_id: Option<NimbusId>,
	sealing: cli_opt::Sealing,
	rpc_config: RpcConfig,
	hwbench: Option<sc_sysinfo::HwBench>,
) -> Result<TaskManager, ServiceError>
where
	RuntimeApi: ConstructRuntimeApi<Block, FullClient<RuntimeApi, Executor>>
		+ Send
		+ Sync
		+ 'static,
	RuntimeApi::RuntimeApi: sp_transaction_pool::runtime_api::TaggedTransactionQueue<Block>
		+ sp_api::ApiExt<Block>
		+ sp_block_builder::BlockBuilder<Block>
		+ substrate_frame_rpc_system::AccountNonceApi<Block, AccountId, Index>
		+ pallet_transaction_payment_rpc_runtime_api::TransactionPaymentApi<Block, Balance>
		+ sp_api::Metadata<Block>
		+ sp_offchain::OffchainWorkerApi<Block>
		+ sp_session::SessionKeys<Block>
		+ fp_rpc::ConvertTransactionRuntimeApi<Block>
		+ fp_rpc::EthereumRuntimeRPCApi<Block>
		+ cumulus_primitives_core::CollectCollationInfo<Block>,
	Executor: sc_executor::NativeExecutionDispatch + 'static,
{
	use async_io::Timer;
	use futures::Stream;
	use sc_consensus_manual_seal::{run_manual_seal, EngineCommand, ManualSealParams};
	use sp_core::H256;

	let sc_service::PartialComponents {
		client,
		backend,
		mut task_manager,
		import_queue,
		keystore_container,
		select_chain: maybe_select_chain,
		transaction_pool,
		other:
			(
				block_import,
				filter_pool,
				mut telemetry,
				_telemetry_worker_handle,
				frontier_backend,
				fee_history_cache,
			),
	} = new_partial::<RuntimeApi, Executor>(&mut config, true)?;

	let (network, system_rpc_tx, tx_handler_controller, network_starter) =
		sc_service::build_network(sc_service::BuildNetworkParams {
			config: &config,
			client: client.clone(),
			transaction_pool: transaction_pool.clone(),
			spawn_handle: task_manager.spawn_handle(),
			import_queue,
			block_announce_validator_builder: None,
			warp_sync: None,
		})?;

	if config.offchain_worker.enabled {
		sc_service::build_offchain_workers(
			&config,
			task_manager.spawn_handle(),
			client.clone(),
			network.clone(),
		);
	}

	let prometheus_registry = config.prometheus_registry().cloned();
	let overrides = crate::rpc::overrides_handle(client.clone());
	let fee_history_limit = rpc_config.fee_history_limit;
	let mut command_sink = None;
	let mut xcm_senders = None;
	let collator = config.role.is_authority();

	if collator {
		let mut env = sc_basic_authorship::ProposerFactory::new(
			task_manager.spawn_handle(),
			client.clone(),
			transaction_pool.clone(),
			prometheus_registry.as_ref(),
			telemetry.as_ref().map(|x| x.handle()),
		);
		env.set_soft_deadline(SOFT_DEADLINE_PERCENT);
		let commands_stream: Box<dyn Stream<Item = EngineCommand<H256>> + Send + Sync + Unpin> =
			match sealing {
				cli_opt::Sealing::Instant => {
					Box::new(
						// This bit cribbed from the implementation of instant seal.
						transaction_pool
							.pool()
							.validated_pool()
							.import_notification_stream()
							.map(|_| EngineCommand::SealNewBlock {
								create_empty: false,
								finalize: false,
								parent_hash: None,
								sender: None,
							}),
					)
				}
				cli_opt::Sealing::Manual => {
					let (sink, stream) = futures::channel::mpsc::channel(1000);
					// Keep a reference to the other end of the channel. It goes to the RPC.
					command_sink = Some(sink);
					Box::new(stream)
				}
				cli_opt::Sealing::Interval(millis) => Box::new(StreamExt::map(
					Timer::interval(Duration::from_millis(millis)),
					|_| EngineCommand::SealNewBlock {
						create_empty: true,
						finalize: false,
						parent_hash: None,
						sender: None,
					},
				)),
			};

		let select_chain = maybe_select_chain.expect(
			"`new_partial` builds a `LongestChainRule` when building dev service.\
				We specified the dev service when calling `new_partial`.\
				Therefore, a `LongestChainRule` is present. qed.",
		);

		let client_set_aside_for_cidp = client.clone();

		// Create channels for mocked XCM messages.
		let (downward_xcm_sender, downward_xcm_receiver) = flume::bounded::<Vec<u8>>(100);
		let (hrmp_xcm_sender, hrmp_xcm_receiver) = flume::bounded::<(ParaId, Vec<u8>)>(100);
		xcm_senders = Some((downward_xcm_sender, hrmp_xcm_sender));

		let client_clone = client.clone();
		let keystore_clone = keystore_container.sync_keystore().clone();
		let maybe_provide_vrf_digest =
			move |nimbus_id: NimbusId, parent: Hash| -> Option<sp_runtime::generic::DigestItem> {
				moonbeam_vrf::vrf_pre_digest::<Block, FullClient<RuntimeApi, Executor>>(
					&client_clone,
					&keystore_clone,
					nimbus_id,
					parent,
				)
			};

		task_manager.spawn_essential_handle().spawn_blocking(
			"authorship_task",
			Some("block-authoring"),
			run_manual_seal(ManualSealParams {
				block_import,
				env,
				client: client.clone(),
				pool: transaction_pool.clone(),
				commands_stream,
				select_chain,
				consensus_data_provider: Some(Box::new(NimbusManualSealConsensusDataProvider {
					keystore: keystore_container.sync_keystore(),
					client: client.clone(),
					additional_digests_provider: maybe_provide_vrf_digest,
					_phantom: Default::default(),
				})),
				create_inherent_data_providers: move |block: H256, ()| {
					let current_para_block = client_set_aside_for_cidp
						.number(block)
						.expect("Header lookup should succeed")
						.expect("Header passed in as parent should be present in backend.");

					let downward_xcm_receiver = downward_xcm_receiver.clone();
					let hrmp_xcm_receiver = hrmp_xcm_receiver.clone();

					let client_for_xcm = client_set_aside_for_cidp.clone();
					async move {
						let time = sp_timestamp::InherentDataProvider::from_system_time();

						let mocked_parachain = MockValidationDataInherentDataProvider {
							current_para_block,
							relay_offset: 1000,
							relay_blocks_per_para_block: 2,
							// TODO: Recheck
							para_blocks_per_relay_epoch: 10,
							relay_randomness_config: (),
							xcm_config: MockXcmConfig::new(
								&*client_for_xcm,
								block,
								Default::default(),
								Default::default(),
							),
							raw_downward_messages: downward_xcm_receiver.drain().collect(),
							raw_horizontal_messages: hrmp_xcm_receiver.drain().collect(),
						};

						let randomness = session_keys_primitives::InherentDataProvider;

						Ok((time, mocked_parachain, randomness))
					}
				},
			}),
		);
	}
	rpc::spawn_essential_tasks(rpc::SpawnTasksParams {
		task_manager: &task_manager,
		client: client.clone(),
		substrate_backend: backend.clone(),
		frontier_backend: frontier_backend.clone(),
		filter_pool: filter_pool.clone(),
		overrides: overrides.clone(),
		fee_history_limit,
		fee_history_cache: fee_history_cache.clone(),
	});
	let ethapi_cmd = rpc_config.ethapi.clone();
	let tracing_requesters =
		if ethapi_cmd.contains(&EthApiCmd::Debug) || ethapi_cmd.contains(&EthApiCmd::Trace) {
			rpc::tracing::spawn_tracing_tasks(
				&rpc_config,
				rpc::SpawnTasksParams {
					task_manager: &task_manager,
					client: client.clone(),
					substrate_backend: backend.clone(),
					frontier_backend: frontier_backend.clone(),
					filter_pool: filter_pool.clone(),
					overrides: overrides.clone(),
					fee_history_limit,
					fee_history_cache: fee_history_cache.clone(),
				},
			)
		} else {
			rpc::tracing::RpcRequesters {
				debug: None,
				trace: None,
			}
		};

	let block_data_cache = Arc::new(fc_rpc::EthBlockDataCacheTask::new(
		task_manager.spawn_handle(),
		overrides.clone(),
		rpc_config.eth_log_block_cache,
		rpc_config.eth_statuses_cache,
		prometheus_registry,
	));

	let rpc_builder = {
		let client = client.clone();
		let pool = transaction_pool.clone();
		let backend = backend.clone();
		let network = network.clone();
		let ethapi_cmd = ethapi_cmd.clone();
		let max_past_logs = rpc_config.max_past_logs;
		let overrides = overrides.clone();
		let fee_history_cache = fee_history_cache.clone();
		let block_data_cache = block_data_cache.clone();

		move |deny_unsafe, subscription_task_executor| {
			let deps = rpc::FullDeps {
				backend: backend.clone(),
				client: client.clone(),
				command_sink: command_sink.clone(),
				deny_unsafe,
				ethapi_cmd: ethapi_cmd.clone(),
				filter_pool: filter_pool.clone(),
				frontier_backend: frontier_backend.clone(),
				graph: pool.pool().clone(),
				pool: pool.clone(),
				is_authority: collator,
				max_past_logs,
				fee_history_limit,
				fee_history_cache: fee_history_cache.clone(),
				network: network.clone(),
				xcm_senders: xcm_senders.clone(),
				overrides: overrides.clone(),
				block_data_cache: block_data_cache.clone(),
			};

			if ethapi_cmd.contains(&EthApiCmd::Debug) || ethapi_cmd.contains(&EthApiCmd::Trace) {
				rpc::create_full(
					deps,
					subscription_task_executor,
					Some(crate::rpc::TracingConfig {
						tracing_requesters: tracing_requesters.clone(),
						trace_filter_max_count: rpc_config.ethapi_trace_max_count,
					}),
				)
				.map_err(Into::into)
			} else {
				rpc::create_full(deps, subscription_task_executor, None).map_err(Into::into)
			}
		}
	};

	let _rpc_handlers = sc_service::spawn_tasks(sc_service::SpawnTasksParams {
		network,
		client,
		keystore: keystore_container.sync_keystore(),
		task_manager: &mut task_manager,
		transaction_pool,
		rpc_builder: Box::new(rpc_builder),
		backend,
		system_rpc_tx,
		config,
		tx_handler_controller,
		telemetry: None,
	})?;

	if let Some(hwbench) = hwbench {
		sc_sysinfo::print_hwbench(&hwbench);

		if let Some(ref mut telemetry) = telemetry {
			let telemetry_handle = telemetry.handle();
			task_manager.spawn_handle().spawn(
				"telemetry_hwbench",
				None,
				sc_sysinfo::initialize_hwbench_telemetry(telemetry_handle, hwbench),
			);
		}
	}

	log::info!("Development Service Ready");

	network_starter.start_network();
	Ok(task_manager)
}
