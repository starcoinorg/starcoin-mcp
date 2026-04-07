# Starmask Native Messaging Examples

## Status

These examples are canonical for the current `v1` extension-backed Native Messaging contract.

Repository status note: the in-tree `crates/starmask-runtime` adapter has been removed. These examples
still apply to the remaining daemon/native-host side of the stack.

They should be kept consistent with:

- `docs/native-messaging-contract.md`
- `DAEMON_PROTOCOL_VERSION = 1`
- `NATIVE_BRIDGE_PROTOCOL_VERSION = 1`

Future generic backend-agent examples should be added separately rather than mutating these `v1`
extension examples in place.

## Purpose

This document provides canonical example payloads for the Native Messaging contract.

These examples are intended for:

- Rust implementation
- extension implementation
- integration tests
- manual debugging

## Conventions

- JSON examples show the message body before Native Messaging framing
- field ordering in examples is illustrative, not normative
- timestamps use Unix seconds in examples

## Example 1: Extension Register

From extension to daemon:

```json
{
  "type": "extension.register",
  "message_id": "msg-0001",
  "protocol_version": 1,
  "wallet_instance_id": "wallet-01HZY6E7T8QX9R7B1M",
  "extension_id": "abcdefghijklmnopqrstuvwxyzabcdef",
  "extension_version": "0.1.0",
  "profile_hint": "Default",
  "lock_state": "unlocked",
  "accounts_summary": [
    {
      "address": "0x1a2b3c",
      "label": "Primary",
      "public_key": "0x1234abcd",
      "is_default": true
    }
  ]
}
```

From daemon to extension:

```json
{
  "type": "extension.registered",
  "reply_to": "msg-0001",
  "wallet_instance_id": "wallet-01HZY6E7T8QX9R7B1M",
  "daemon_protocol_version": 1,
  "accepted": true
}
```

## Example 2: Heartbeat

```json
{
  "type": "extension.heartbeat",
  "message_id": "msg-0002",
  "wallet_instance_id": "wallet-01HZY6E7T8QX9R7B1M",
  "presented_request_ids": [
    "req-01HZY6R8MKP4V6P7TK"
  ]
}
```

Daemon response:

```json
{
  "type": "extension.ack",
  "reply_to": "msg-0002"
}
```

## Example 3: Account Update

```json
{
  "type": "extension.updateAccounts",
  "message_id": "msg-0003",
  "wallet_instance_id": "wallet-01HZY6E7T8QX9R7B1M",
  "lock_state": "unlocked",
  "accounts": [
    {
      "address": "0x1a2b3c",
      "label": "Primary",
      "public_key": "0x1234abcd",
      "is_default": true
    },
    {
      "address": "0x4d5e6f",
      "label": "Secondary",
      "public_key": null,
      "is_default": false
    }
  ]
}
```

Daemon response:

```json
{
  "type": "extension.ack",
  "reply_to": "msg-0003"
}
```

## Example 4: Request Available Hint

From daemon to extension:

```json
{
  "type": "request.available",
  "wallet_instance_id": "wallet-01HZY6E7T8QX9R7B1M"
}
```

## Example 5: Pull Next for First Presentation

From extension to daemon:

```json
{
  "type": "request.pullNext",
  "message_id": "msg-0004",
  "wallet_instance_id": "wallet-01HZY6E7T8QX9R7B1M"
}
```

From daemon to extension:

```json
{
  "type": "request.next",
  "reply_to": "msg-0004",
  "request_id": "req-01HZY6R8MKP4V6P7TK",
  "client_request_id": "cli-prepare-transfer-001",
  "kind": "sign_transaction",
  "account_address": "0x1a2b3c",
  "payload_hash": "0x7f9c4b1e",
  "display_hint": "Transfer 10 STC to 0x9f00",
  "client_context": "codex",
  "resume_required": false,
  "delivery_lease_id": "lease-01HZY6R9QQW5FEXD4M",
  "lease_expires_at": 1760000030,
  "raw_txn_bcs_hex": "0xabcd1234"
}
```

## Example 5b: Pull Next With No Work

```json
{
  "type": "request.none",
  "reply_to": "msg-0004",
  "wallet_instance_id": "wallet-01HZY6E7T8QX9R7B1M"
}
```

## Example 6: Mark Presented

From extension to daemon:

```json
{
  "type": "request.presented",
  "message_id": "msg-0005",
  "wallet_instance_id": "wallet-01HZY6E7T8QX9R7B1M",
  "request_id": "req-01HZY6R8MKP4V6P7TK",
  "delivery_lease_id": "lease-01HZY6R9QQW5FEXD4M",
  "presentation_id": "pres-01HZY6RXF11M4KJK1D"
}
```

Daemon response:

```json
{
  "type": "extension.ack",
  "reply_to": "msg-0005"
}
```

## Example 7: Resolve Transaction Approval

From extension to daemon:

```json
{
  "type": "request.resolve",
  "message_id": "msg-0006",
  "wallet_instance_id": "wallet-01HZY6E7T8QX9R7B1M",
  "request_id": "req-01HZY6R8MKP4V6P7TK",
  "presentation_id": "pres-01HZY6RXF11M4KJK1D",
  "result_kind": "signed_transaction",
  "signed_txn_bcs_hex": "0xfeedbeef"
}
```

Daemon response:

```json
{
  "type": "extension.ack",
  "reply_to": "msg-0006"
}
```

## Example 8: Reject Request

From extension to daemon:

```json
{
  "type": "request.reject",
  "message_id": "msg-0007",
  "wallet_instance_id": "wallet-01HZY6E7T8QX9R7B1M",
  "request_id": "req-01HZY6R8MKP4V6P7TK",
  "presentation_id": "pres-01HZY6RXF11M4KJK1D",
  "reason_code": "request_rejected",
  "reason_message": "User rejected the request"
}
```

Daemon response:

```json
{
  "type": "extension.ack",
  "reply_to": "msg-0007"
}
```

## Example 9: Resume Previously Presented Request

From extension to daemon:

```json
{
  "type": "request.pullNext",
  "message_id": "msg-0008",
  "wallet_instance_id": "wallet-01HZY6E7T8QX9R7B1M"
}
```

From daemon to extension:

```json
{
  "type": "request.next",
  "reply_to": "msg-0008",
  "request_id": "req-01HZY6R8MKP4V6P7TK",
  "client_request_id": "cli-prepare-transfer-001",
  "kind": "sign_transaction",
  "account_address": "0x1a2b3c",
  "payload_hash": "0x7f9c4b1e",
  "display_hint": "Transfer 10 STC to 0x9f00",
  "client_context": "codex",
  "resume_required": true,
  "presentation_id": "pres-01HZY6RXF11M4KJK1D",
  "presentation_expires_at": 1760000060,
  "raw_txn_bcs_hex": "0xabcd1234"
}
```

## Example 10: Cancel Notification

From daemon to extension:

```json
{
  "type": "request.cancelled",
  "wallet_instance_id": "wallet-01HZY6E7T8QX9R7B1M",
  "request_id": "req-01HZY6R8MKP4V6P7TK"
}
```

## Example 11: Extension Error

From daemon to extension:

```json
{
  "type": "extension.error",
  "reply_to": "msg-0001",
  "code": "protocol_version_mismatch",
  "message": "Native bridge protocol version 2 is not supported",
  "retryable": false
}
```

## Example 12: Message Signing

First presentation:

```json
{
  "type": "request.next",
  "reply_to": "msg-0009",
  "request_id": "req-01HZY7AAA22DF4GH12",
  "client_request_id": "cli-sign-message-001",
  "kind": "sign_message",
  "account_address": "0x1a2b3c",
  "payload_hash": "0xdedede01",
  "display_hint": "Sign login challenge",
  "client_context": "claude-code",
  "resume_required": false,
  "delivery_lease_id": "lease-01HZY7ABCD1234",
  "lease_expires_at": 1760000130,
  "message": "Sign in to Starcoin MCP"
}
```

Approval result:

```json
{
  "type": "request.resolve",
  "message_id": "msg-0010",
  "wallet_instance_id": "wallet-01HZY6E7T8QX9R7B1M",
  "request_id": "req-01HZY7AAA22DF4GH12",
  "presentation_id": "pres-01HZY7AXYZ7788",
  "result_kind": "signed_message",
  "signature": "0xdeadbead"
}
```

Daemon response:

```json
{
  "type": "extension.ack",
  "reply_to": "msg-0010"
}
```

## Native Framing Notes

The actual Native Messaging wire frame is:

1. 32-bit native-endian length
2. UTF-8 JSON bytes

These examples intentionally omit the binary prefix so they remain easy to inspect in docs and tests.

## Test Usage

These payloads should be reused in:

- Native Messaging integration tests
- fixture-based parser tests
- manual debugging sessions
- fake extension and fake daemon harnesses
