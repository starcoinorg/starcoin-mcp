use std::borrow::Cow;

use rmcp::{
    ErrorData, ServerHandler,
    handler::server::tool::{parse_json_object, schema_for_type},
    model::{
        CallToolRequestParams, CallToolResult, Implementation, ListToolsResult, ServerCapabilities,
        ServerInfo, Tool,
    },
    service::RequestContext,
};

use starmask_types::{
    ClientRequestId, CreateSignMessageParams, CreateSignTransactionParams, RequestId,
};

use crate::{
    daemon_client::DaemonClient,
    dto::{
        EmptyParams, WalletCancelRequestInput, WalletGetPublicKeyInput,
        WalletGetRequestStatusInput, WalletListAccountsInput, WalletRequestSignTransactionInput,
        WalletSignMessageInput,
    },
    error_mapping::AdapterError,
};

pub struct StarmaskMcpServer<C> {
    daemon_client: C,
}

impl<C> StarmaskMcpServer<C> {
    pub fn new(daemon_client: C) -> Self {
        Self { daemon_client }
    }
}

impl<C> ServerHandler for StarmaskMcpServer<C>
where
    C: DaemonClient,
{
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_server_info(
            Implementation::new("starmask-mcp", env!("CARGO_PKG_VERSION"))
                .with_title("Starmask MCP"),
        )
    }

    async fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: vec![
                tool::<EmptyParams>(
                    "wallet_status",
                    "Get current Starmask wallet availability and known instances.",
                ),
                tool::<WalletListAccountsInput>(
                    "wallet_list_accounts",
                    "List visible Starmask accounts, optionally including cached public keys.",
                ),
                tool::<WalletGetPublicKeyInput>(
                    "wallet_get_public_key",
                    "Resolve the public key for one known wallet account.",
                ),
                tool::<WalletRequestSignTransactionInput>(
                    "wallet_request_sign_transaction",
                    "Create an asynchronous transaction-signing request in Starmask.",
                ),
                tool::<WalletGetRequestStatusInput>(
                    "wallet_get_request_status",
                    "Poll the lifecycle state of one signing request.",
                ),
                tool::<WalletCancelRequestInput>(
                    "wallet_cancel_request",
                    "Cancel one in-flight signing request.",
                ),
                tool::<WalletSignMessageInput>(
                    "wallet_sign_message",
                    "Create an asynchronous message-signing request in Starmask.",
                ),
                tool::<EmptyParams>(
                    "wallet_list_instances",
                    "List known Starmask wallet instances.",
                ),
            ],
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = match request.name.as_ref() {
            "wallet_status" => {
                let _: EmptyParams = parse_arguments(request.arguments)?;
                serde_json::to_value(
                    self.daemon_client
                        .wallet_status()
                        .await
                        .map_err(to_mcp_error)?,
                )
                .map_err(AdapterError::from)
                .map_err(to_mcp_error)?
            }
            "wallet_list_accounts" => {
                let params: WalletListAccountsInput = parse_arguments(request.arguments)?;
                serde_json::to_value(
                    self.daemon_client
                        .wallet_list_accounts(
                            params
                                .wallet_instance_id()
                                .map_err(AdapterError::from)
                                .map_err(to_mcp_error)?,
                            params.include_public_key,
                        )
                        .await
                        .map_err(to_mcp_error)?,
                )
                .map_err(AdapterError::from)
                .map_err(to_mcp_error)?
            }
            "wallet_get_public_key" => {
                let params: WalletGetPublicKeyInput = parse_arguments(request.arguments)?;
                let wallet_instance_id = params
                    .wallet_instance_id()
                    .map_err(AdapterError::from)
                    .map_err(to_mcp_error)?;
                serde_json::to_value(
                    self.daemon_client
                        .wallet_get_public_key(params.address, wallet_instance_id)
                        .await
                        .map_err(to_mcp_error)?,
                )
                .map_err(AdapterError::from)
                .map_err(to_mcp_error)?
            }
            "wallet_request_sign_transaction" => {
                let params: WalletRequestSignTransactionInput = parse_arguments(request.arguments)?;
                let wallet_instance_id = params
                    .wallet_instance_id()
                    .map_err(AdapterError::from)
                    .map_err(to_mcp_error)?;
                let ttl_seconds = params.ttl_seconds();
                serde_json::to_value(
                    self.daemon_client
                        .create_sign_transaction_request(CreateSignTransactionParams {
                            protocol_version: starmask_types::DAEMON_PROTOCOL_VERSION,
                            client_request_id: ClientRequestId::new(params.client_request_id)
                                .map_err(AdapterError::from)
                                .map_err(to_mcp_error)?,
                            account_address: params.account_address,
                            wallet_instance_id,
                            chain_id: params.chain_id,
                            raw_txn_bcs_hex: params.raw_txn_bcs_hex,
                            tx_kind: params.tx_kind,
                            display_hint: params.display_hint,
                            client_context: params.client_context,
                            ttl_seconds,
                        })
                        .await
                        .map_err(to_mcp_error)?,
                )
                .map_err(AdapterError::from)
                .map_err(to_mcp_error)?
            }
            "wallet_get_request_status" => {
                let params: WalletGetRequestStatusInput = parse_arguments(request.arguments)?;
                serde_json::to_value(
                    self.daemon_client
                        .get_request_status(
                            RequestId::new(params.request_id)
                                .map_err(AdapterError::from)
                                .map_err(to_mcp_error)?,
                        )
                        .await
                        .map_err(to_mcp_error)?,
                )
                .map_err(AdapterError::from)
                .map_err(to_mcp_error)?
            }
            "wallet_cancel_request" => {
                let params: WalletCancelRequestInput = parse_arguments(request.arguments)?;
                serde_json::to_value(
                    self.daemon_client
                        .cancel_request(
                            RequestId::new(params.request_id)
                                .map_err(AdapterError::from)
                                .map_err(to_mcp_error)?,
                        )
                        .await
                        .map_err(to_mcp_error)?,
                )
                .map_err(AdapterError::from)
                .map_err(to_mcp_error)?
            }
            "wallet_sign_message" => {
                let params: WalletSignMessageInput = parse_arguments(request.arguments)?;
                let wallet_instance_id = params
                    .wallet_instance_id()
                    .map_err(AdapterError::from)
                    .map_err(to_mcp_error)?;
                let ttl_seconds = params.ttl_seconds();
                serde_json::to_value(
                    self.daemon_client
                        .create_sign_message_request(CreateSignMessageParams {
                            protocol_version: starmask_types::DAEMON_PROTOCOL_VERSION,
                            client_request_id: ClientRequestId::new(params.client_request_id)
                                .map_err(AdapterError::from)
                                .map_err(to_mcp_error)?,
                            account_address: params.account_address,
                            wallet_instance_id,
                            message: params.message,
                            format: params.format.into(),
                            display_hint: params.display_hint,
                            client_context: params.client_context,
                            ttl_seconds,
                        })
                        .await
                        .map_err(to_mcp_error)?,
                )
                .map_err(AdapterError::from)
                .map_err(to_mcp_error)?
            }
            "wallet_list_instances" => {
                let _: EmptyParams = parse_arguments(request.arguments)?;
                serde_json::to_value(
                    self.daemon_client
                        .wallet_list_instances(false)
                        .await
                        .map_err(to_mcp_error)?,
                )
                .map_err(AdapterError::from)
                .map_err(to_mcp_error)?
            }
            other => {
                return Err(ErrorData::invalid_params(
                    format!("unknown tool: {other}"),
                    None,
                ));
            }
        };

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

fn to_mcp_error(error: impl Into<AdapterError>) -> ErrorData {
    error.into().into()
}
