use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::errors::{SharedError, SharedErrorCode};

pub const JSONRPC_VERSION: &str = "2.0";

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct JsonRpcRequest<T> {
    pub jsonrpc: String,
    pub id: String,
    pub method: String,
    pub params: T,
}

impl<T> JsonRpcRequest<T> {
    pub fn new(id: impl Into<String>, method: impl Into<String>, params: T) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id: id.into(),
            method: method.into(),
            params,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct JsonRpcSuccess<T> {
    pub jsonrpc: String,
    pub id: String,
    pub result: T,
}

impl<T> JsonRpcSuccess<T> {
    pub fn new(id: impl Into<String>, result: T) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id: id.into(),
            result,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct JsonRpcErrorObject {
    pub code: SharedErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retryable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl From<SharedError> for JsonRpcErrorObject {
    fn from(value: SharedError) -> Self {
        Self {
            code: value.code,
            message: value.message,
            retryable: value.retryable,
            details: value.details,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct JsonRpcErrorResponse {
    pub jsonrpc: String,
    pub id: String,
    pub error: JsonRpcErrorObject,
}

impl JsonRpcErrorResponse {
    pub fn new(id: impl Into<String>, error: impl Into<JsonRpcErrorObject>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_owned(),
            id: id.into(),
            error: error.into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(untagged)]
pub enum JsonRpcResponse<T> {
    Success(JsonRpcSuccess<T>),
    Error(JsonRpcErrorResponse),
}
