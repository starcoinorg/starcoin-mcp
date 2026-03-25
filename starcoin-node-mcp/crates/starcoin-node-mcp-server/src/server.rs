use std::borrow::Cow;

use anyhow::Result;
use rmcp::{
    ErrorData, ServerHandler, ServiceExt,
    handler::server::tool::{parse_json_object, schema_for_type},
    model::{
        CallToolRequestParams, CallToolResult, Implementation, ListToolsResult, ServerCapabilities,
        ServerInfo, Tool,
    },
    service::RequestContext,
    transport::stdio,
};
use serde_json::Value;
use starcoin_node_mcp_core::AppContext;
use starcoin_node_mcp_types::{
    CallViewFunctionInput, EmptyParams, GetAccountOverviewInput, GetBlockInput, GetEventsInput,
    GetTransactionInput, ListBlocksInput, ListModulesInput, ListResourcesInput, Mode,
    PrepareContractCallInput, PreparePublishPackageInput, PrepareTransferInput,
    ResolveFunctionAbiInput, ResolveModuleAbiInput, ResolveStructAbiInput, RuntimeConfig,
    SimulateRawTransactionInput, SubmitSignedTransactionInput, WatchTransactionInput,
};

use crate::error_mapping::AdapterError;

pub struct StarcoinNodeMcpServer {
    app: AppContext,
}

impl StarcoinNodeMcpServer {
    pub fn new(app: AppContext) -> Self {
        Self { app }
    }

    pub async fn bootstrap(config: RuntimeConfig) -> Result<Self> {
        let app = AppContext::bootstrap(config).await?;
        Ok(Self::new(app))
    }

    pub async fn serve_stdio(self) -> Result<()> {
        let running_service = self.serve(stdio()).await?;
        let _ = running_service.waiting().await?;
        Ok(())
    }

    pub fn advertised_tools(&self) -> Vec<Tool> {
        let probe = self.app.startup_probe();
        let mut tools = vec![
            tool::<EmptyParams>(
                "chain_status",
                "Return the current high-level chain context.",
            ),
            tool::<EmptyParams>("node_health", "Return a summarized node health snapshot."),
            tool::<GetBlockInput>("get_block", "Get a block by hash or block number."),
        ];
        if probe.supports_block_listing {
            tools.push(tool::<ListBlocksInput>(
                "list_blocks",
                "Get a bounded recent block listing.",
            ));
        }
        if probe.supports_transaction_lookup {
            tools.push(tool::<GetTransactionInput>(
                "get_transaction",
                "Get a transaction and its execution context by transaction hash.",
            ));
        }
        if probe.supports_transaction_lookup && probe.supports_transaction_info_lookup {
            tools.push(tool::<WatchTransactionInput>(
                "watch_transaction",
                "Poll a transaction until terminal status or timeout.",
            ));
        }
        if probe.supports_account_state_lookup {
            tools.push(tool::<GetAccountOverviewInput>(
                "get_account_overview",
                "Return a task-oriented summary of an account.",
            ));
        }
        if probe.supports_events_query {
            tools.push(tool::<GetEventsInput>(
                "get_events",
                "Query events by filter.",
            ));
        }
        if probe.supports_resource_listing {
            tools.push(tool::<ListResourcesInput>(
                "list_resources",
                "List resources for an account.",
            ));
        }
        if probe.supports_module_listing {
            tools.push(tool::<ListModulesInput>(
                "list_modules",
                "List modules for an account.",
            ));
        }
        if probe.supports_abi_resolution {
            tools.extend([
                tool::<ResolveFunctionAbiInput>(
                    "resolve_function_abi",
                    "Resolve a function ABI from a fully qualified function id.",
                ),
                tool::<ResolveStructAbiInput>(
                    "resolve_struct_abi",
                    "Resolve a struct ABI from a fully qualified struct tag.",
                ),
                tool::<ResolveModuleAbiInput>(
                    "resolve_module_abi",
                    "Resolve a module ABI from a module id.",
                ),
            ]);
        }
        if probe.supports_view_call {
            tools.push(tool::<CallViewFunctionInput>(
                "call_view_function",
                "Execute a contract call without changing chain state.",
            ));
        }
        if self.app.mode() == Mode::Transaction {
            tools.extend([
                tool::<PrepareTransferInput>(
                    "prepare_transfer",
                    "Prepare an unsigned transfer transaction.",
                ),
                tool::<PrepareContractCallInput>(
                    "prepare_contract_call",
                    "Prepare an unsigned contract-call transaction.",
                ),
                tool::<PreparePublishPackageInput>(
                    "prepare_publish_package",
                    "Prepare an unsigned package publish transaction.",
                ),
                tool::<SimulateRawTransactionInput>(
                    "simulate_raw_transaction",
                    "Simulate a prepared raw transaction before signing.",
                ),
                tool::<SubmitSignedTransactionInput>(
                    "submit_signed_transaction",
                    "Submit an already signed transaction.",
                ),
            ]);
        }
        tools
    }

    pub async fn call_tool_json(
        &self,
        name: &str,
        arguments: Option<rmcp::model::JsonObject>,
    ) -> Result<Value, AdapterError> {
        match name {
            "chain_status" => {
                let _: EmptyParams = parse_arguments_adapter(arguments)?;
                serialize_value(self.app.chain_status().await?)
            }
            "node_health" => {
                let _: EmptyParams = parse_arguments_adapter(arguments)?;
                serialize_value(self.app.node_health().await?)
            }
            "get_block" => {
                let params: GetBlockInput = parse_arguments_adapter(arguments)?;
                serialize_value(self.app.get_block(params).await?)
            }
            "list_blocks" => {
                let params: ListBlocksInput = parse_arguments_adapter(arguments)?;
                serialize_value(self.app.list_blocks(params).await?)
            }
            "get_transaction" => {
                let params: GetTransactionInput = parse_arguments_adapter(arguments)?;
                serialize_value(self.app.get_transaction(params).await?)
            }
            "watch_transaction" => {
                let params: WatchTransactionInput = parse_arguments_adapter(arguments)?;
                serialize_value(self.app.watch_transaction(params).await?)
            }
            "get_events" => {
                let params: GetEventsInput = parse_arguments_adapter(arguments)?;
                serialize_value(self.app.get_events(params).await?)
            }
            "get_account_overview" => {
                let params: GetAccountOverviewInput = parse_arguments_adapter(arguments)?;
                serialize_value(self.app.get_account_overview(params).await?)
            }
            "list_resources" => {
                let params: ListResourcesInput = parse_arguments_adapter(arguments)?;
                serialize_value(self.app.list_resources(params).await?)
            }
            "list_modules" => {
                let params: ListModulesInput = parse_arguments_adapter(arguments)?;
                serialize_value(self.app.list_modules(params).await?)
            }
            "resolve_function_abi" => {
                let params: ResolveFunctionAbiInput = parse_arguments_adapter(arguments)?;
                self.app
                    .resolve_function_abi(params)
                    .await
                    .map_err(AdapterError::from)
            }
            "resolve_struct_abi" => {
                let params: ResolveStructAbiInput = parse_arguments_adapter(arguments)?;
                self.app
                    .resolve_struct_abi(params)
                    .await
                    .map_err(AdapterError::from)
            }
            "resolve_module_abi" => {
                let params: ResolveModuleAbiInput = parse_arguments_adapter(arguments)?;
                self.app
                    .resolve_module_abi(params)
                    .await
                    .map_err(AdapterError::from)
            }
            "call_view_function" => {
                let params: CallViewFunctionInput = parse_arguments_adapter(arguments)?;
                serialize_value(self.app.call_view_function(params).await?)
            }
            "prepare_transfer" => {
                let params: PrepareTransferInput = parse_arguments_adapter(arguments)?;
                serialize_value(self.app.prepare_transfer(params).await?)
            }
            "prepare_contract_call" => {
                let params: PrepareContractCallInput = parse_arguments_adapter(arguments)?;
                serialize_value(self.app.prepare_contract_call(params).await?)
            }
            "prepare_publish_package" => {
                let params: PreparePublishPackageInput = parse_arguments_adapter(arguments)?;
                serialize_value(self.app.prepare_publish_package(params).await?)
            }
            "simulate_raw_transaction" => {
                let params: SimulateRawTransactionInput = parse_arguments_adapter(arguments)?;
                serialize_value(self.app.simulate_raw_transaction(params).await?)
            }
            "submit_signed_transaction" => {
                let params: SubmitSignedTransactionInput = parse_arguments_adapter(arguments)?;
                serialize_value(self.app.submit_signed_transaction(params).await?)
            }
            other => {
                return Err(AdapterError::InvalidRequest(format!(
                    "unknown tool: {other}"
                )));
            }
        }
    }
}

impl ServerHandler for StarcoinNodeMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_server_info(
            Implementation::new("starcoin-node-mcp", env!("CARGO_PKG_VERSION"))
                .with_title("Starcoin Node MCP"),
        )
    }

    async fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: RequestContext<rmcp::RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: self.advertised_tools(),
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = self
            .call_tool_json(request.name.as_ref(), request.arguments)
            .await
            .map_err(to_mcp_error)?;
        Ok(CallToolResult::structured(result))
    }
}

fn tool<T>(name: &'static str, description: &'static str) -> Tool
where
    T: rmcp::schemars::JsonSchema + 'static,
{
    let mut tool = Tool::default();
    tool.name = Cow::Borrowed(name);
    tool.description = Some(Cow::Borrowed(description));
    tool.input_schema = schema_for_type::<T>();
    tool
}

fn parse_arguments<T>(arguments: Option<rmcp::model::JsonObject>) -> Result<T, ErrorData>
where
    T: serde::de::DeserializeOwned,
{
    parse_json_object(
        serde_json::Value::Object(arguments.unwrap_or_default())
            .as_object()
            .cloned()
            .unwrap_or_default(),
    )
}

fn parse_arguments_adapter<T>(arguments: Option<rmcp::model::JsonObject>) -> Result<T, AdapterError>
where
    T: serde::de::DeserializeOwned,
{
    parse_arguments(arguments)
        .map_err(|error| AdapterError::InvalidRequest(error.message.into_owned()))
}

fn serialize_value<T: serde::Serialize>(value: T) -> Result<serde_json::Value, AdapterError> {
    serde_json::to_value(value).map_err(AdapterError::from)
}

fn to_mcp_error(error: impl Into<AdapterError>) -> ErrorData {
    error.into().into()
}
