#![forbid(unsafe_code)]

use std::{
    fmt::Write as _,
    io::{self, Write},
};

use starcoin_account_api::AccountInfo;
use starcoin_types::{
    account_address::AccountAddress,
    account_config::core_code_address,
    transaction::{RawUserTransaction, ScriptFunction, TransactionPayload},
};
use starmask_types::{MessageFormat, PulledRequest, RequestKind, WalletCapability};

use crate::request_support::{RequestRejection, ensure_local_unlock_capability};

const CARD_INNER_WIDTH: usize = 76;
const PREVIEW_SUFFIX: &str = "...";

pub(crate) trait ApprovalPrompt: Send + Sync {
    fn prompt_for_request(
        &self,
        request: &PulledRequest,
        account_info: &AccountInfo,
        capabilities: &[WalletCapability],
    ) -> std::result::Result<PromptApproval, RequestRejection>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PromptApproval {
    pub(crate) approved: bool,
    pub(crate) password: Option<String>,
}

impl PromptApproval {
    pub(crate) fn approved(&self) -> bool {
        self.approved
    }

    pub(crate) fn password(&self) -> Option<&str> {
        self.password.as_deref()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PromptAction {
    Approve,
    Reject,
    ViewRaw,
}

#[derive(Default)]
pub(crate) struct TtyApprovalPrompt;

impl ApprovalPrompt for TtyApprovalPrompt {
    fn prompt_for_request(
        &self,
        request: &PulledRequest,
        account_info: &AccountInfo,
        capabilities: &[WalletCapability],
    ) -> std::result::Result<PromptApproval, RequestRejection> {
        ensure_local_unlock_capability(account_info.is_locked, capabilities)?;
        print_request_summary(request, account_info);

        loop {
            match prompt_for_action().map_err(|error| RequestRejection {
                reason_code: starmask_types::RejectReasonCode::BackendUnavailable,
                message: Some(format!("Failed to read local approval input: {error}")),
            })? {
                PromptAction::Approve => break,
                PromptAction::Reject => {
                    return Ok(PromptApproval {
                        approved: false,
                        password: None,
                    });
                }
                PromptAction::ViewRaw => {
                    print_raw_payload_details(request);
                }
            }
        }

        let password = if account_info.is_locked {
            let password = rpassword::prompt_password("Account password: ").map_err(|error| {
                RequestRejection {
                    reason_code: starmask_types::RejectReasonCode::BackendUnavailable,
                    message: Some(format!("Failed to read account password: {error}")),
                }
            })?;
            if password.is_empty() {
                return Err(RequestRejection {
                    reason_code: starmask_types::RejectReasonCode::WalletLocked,
                    message: Some("Password entry was cancelled".to_owned()),
                });
            }
            Some(password)
        } else {
            None
        };

        Ok(PromptApproval {
            approved: true,
            password,
        })
    }
}

fn prompt_for_action() -> io::Result<PromptAction> {
    loop {
        let mut stderr = io::stderr().lock();
        stderr.write_all(b"Choose action [a]pprove / [r]eject / [v]iew raw: ")?;
        stderr.flush()?;

        let mut line = String::new();
        let bytes_read = io::stdin().read_line(&mut line)?;
        match parse_prompt_action(bytes_read, &line) {
            Ok(action) => return Ok(action),
            Err(error) if error.kind() == io::ErrorKind::InvalidInput => {
                writeln!(stderr, "Enter one of: a, approve, r, reject, v, view.")?;
            }
            Err(error) => return Err(error),
        }
    }
}

fn parse_prompt_action(bytes_read: usize, line: &str) -> io::Result<PromptAction> {
    if bytes_read == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "approval prompt stdin is closed",
        ));
    }

    match line.trim().to_ascii_lowercase().as_str() {
        "a" | "approve" => Ok(PromptAction::Approve),
        "r" | "reject" => Ok(PromptAction::Reject),
        "v" | "view" | "view raw" => Ok(PromptAction::ViewRaw),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unknown approval action",
        )),
    }
}

fn sanitize_for_tty(input: &str) -> String {
    let mut sanitized = String::with_capacity(input.len());
    for character in input.chars() {
        match character {
            '\\' => sanitized.push_str("\\\\"),
            '\n' => sanitized.push_str("\\n"),
            '\r' => sanitized.push_str("\\r"),
            '\t' => sanitized.push_str("\\t"),
            character if character.is_control() || is_unicode_format_character(character) => {
                sanitized.push_str(&format!("\\u{{{:x}}}", u32::from(character)));
            }
            character => sanitized.push(character),
        }
    }
    sanitized
}

fn is_unicode_format_character(character: char) -> bool {
    matches!(
        u32::from(character),
        0x00ad
            | 0x0600..=0x0605
            | 0x061c
            | 0x06dd
            | 0x070f
            | 0x0890..=0x0891
            | 0x08e2
            | 0x180e
            | 0x200b..=0x200f
            | 0x202a..=0x202e
            | 0x2060..=0x2064
            | 0x2066..=0x206f
            | 0xfeff
            | 0xfff9..=0xfffb
            | 0x110bd
            | 0x110cd
            | 0x13430..=0x1343f
            | 0x1bca0..=0x1bca3
            | 0x1d173..=0x1d17a
            | 0xe0001
            | 0xe0020..=0xe007f
    )
}

fn print_request_summary(request: &PulledRequest, account_info: &AccountInfo) {
    eprint!("{}", render_request_summary(request, account_info));
}

fn print_raw_payload_details(request: &PulledRequest) {
    eprint!("{}", render_raw_payload_details(request));
}

fn render_request_summary(request: &PulledRequest, account_info: &AccountInfo) -> String {
    let mut output = String::new();
    output.push('\n');

    write_card_border(&mut output);
    write_card_text_line(&mut output, "Starmask Local Signing Request");
    write_card_border(&mut output);

    write_request_section(&mut output, request);
    write_account_section(&mut output, request, account_info);
    match request.kind {
        RequestKind::SignTransaction => write_transaction_section(&mut output, request),
        RequestKind::SignMessage => write_message_section(&mut output, request),
    }
    write_context_section(&mut output, request);
    write_actions_section(&mut output);
    write_card_border(&mut output);

    output.push('\n');
    output
}

fn render_raw_payload_details(request: &PulledRequest) -> String {
    let mut output = String::new();
    output.push('\n');

    write_card_border(&mut output);
    write_card_text_line(&mut output, "Starmask Canonical Payload Details");
    write_card_border(&mut output);

    write_card_section_heading(&mut output, "Diagnostics");
    write_card_field_full(&mut output, "Request ID", &request.request_id.to_string());
    write_card_field_full(
        &mut output,
        "Payload Hash",
        &request.payload_hash.to_string(),
    );
    write_card_field_full(
        &mut output,
        "Client Request ID (untrusted)",
        &request.client_request_id.to_string(),
    );

    match request.kind {
        RequestKind::SignTransaction => {
            write_card_section_heading(&mut output, "Raw Transaction");
            if let Some(raw_txn_bcs_hex) = &request.raw_txn_bcs_hex {
                write_card_field_full(&mut output, "Raw Transaction BCS", raw_txn_bcs_hex);
                match decode_raw_transaction(raw_txn_bcs_hex) {
                    Ok(raw_txn) => write_decoded_transaction_fields(&mut output, &raw_txn, true),
                    Err(error) => write_card_field_full(&mut output, "Decode Error", &error),
                }
            } else {
                write_card_field_full(&mut output, "Raw Transaction BCS", "missing");
            }
        }
        RequestKind::SignMessage => {
            write_card_section_heading(&mut output, "Canonical Message");
            if let Some(message_format) = request.message_format {
                write_card_field_full(
                    &mut output,
                    "Message Format",
                    message_format_label(message_format),
                );
            }
            if let Some(message) = &request.message {
                write_card_field_full(&mut output, "Message", message);
            } else {
                write_card_field_full(&mut output, "Message", "missing");
            }
            match canonical_message_byte_len(request) {
                Ok(Some(byte_len)) => {
                    write_card_field_full(&mut output, "Byte Length", &byte_len.to_string())
                }
                Ok(None) => {}
                Err(error) => write_card_field_full(&mut output, "Message Decode Error", &error),
            }
        }
    }

    write_card_section_heading(&mut output, "Back");
    write_card_text_line(
        &mut output,
        "Press enter on the next prompt and choose approve or reject.",
    );
    write_card_border(&mut output);

    output.push('\n');
    output
}

fn write_request_section(output: &mut String, request: &PulledRequest) {
    write_card_section_heading(output, "Request");
    write_card_field_preview(output, "Request ID", &request.request_id.to_string());
    write_card_field_preview(
        output,
        "Client Request ID (untrusted)",
        &request.client_request_id.to_string(),
    );
    write_card_field_preview(output, "Kind", request_kind_label(request.kind));
    write_card_field_preview(output, "Payload Hash", &request.payload_hash.to_string());
    write_card_field_preview(
        output,
        "Resume Required",
        &request.resume_required.to_string(),
    );
    if let Some(presentation_id) = &request.presentation_id {
        write_card_field_preview(output, "Presentation ID", &presentation_id.to_string());
    }
}

fn write_account_section(output: &mut String, request: &PulledRequest, account_info: &AccountInfo) {
    write_card_section_heading(output, "Account");
    write_card_field_preview(output, "Account", &request.account_address);
    write_card_field_preview(
        output,
        "Account Locked",
        &account_info.is_locked.to_string(),
    );
}

fn write_transaction_section(output: &mut String, request: &PulledRequest) {
    write_card_section_heading(output, "Transaction");
    if let Some(raw_txn_bcs_hex) = &request.raw_txn_bcs_hex {
        match decode_raw_transaction(raw_txn_bcs_hex) {
            Ok(raw_txn) => write_decoded_transaction_fields(output, &raw_txn, false),
            Err(error) => {
                write_card_field_preview(output, "Decode Error", &error);
                write_card_field_preview(output, "Raw BCS Preview", raw_txn_bcs_hex);
            }
        }
    } else {
        write_card_field_preview(output, "Raw Transaction BCS", "missing");
    }
}

fn write_decoded_transaction_fields(
    output: &mut String,
    raw_txn: &RawUserTransaction,
    include_raw_argument_lines: bool,
) {
    write_card_field_preview(output, "Chain ID", &raw_txn.chain_id().id().to_string());
    write_card_field_preview(output, "Sender", &raw_txn.sender().to_string());
    write_card_field_preview(
        output,
        "Sequence Number",
        &raw_txn.sequence_number().to_string(),
    );
    write_card_field_preview(
        output,
        "Payload Type",
        payload_type_label(raw_txn.payload()),
    );
    write_card_field_preview(
        output,
        "Gas Unit Price",
        &raw_txn.gas_unit_price().to_string(),
    );
    write_card_field_preview(
        output,
        "Max Gas Amount",
        &raw_txn.max_gas_amount().to_string(),
    );
    write_card_field_preview(output, "Gas Token", &raw_txn.gas_token_code());
    write_card_field_preview(
        output,
        "Max Fee Budget",
        &format!(
            "{} {}",
            u128::from(raw_txn.max_gas_amount()) * u128::from(raw_txn.gas_unit_price()),
            raw_txn.gas_token_code()
        ),
    );
    write_card_field_preview(
        output,
        "Expiration",
        &format_expiration(raw_txn.expiration_timestamp_secs()),
    );

    match raw_txn.payload() {
        TransactionPayload::Script(script) => {
            write_card_field_preview(
                output,
                "Script Code Bytes",
                &script.code().len().to_string(),
            );
            if !script.ty_args().is_empty() {
                write_card_field_preview(output, "Type Args", &format_type_tags(script.ty_args()));
            }
            write_card_field_preview(output, "Argument Count", &script.args().len().to_string());
            if include_raw_argument_lines {
                for (index, arg) in script.args().iter().enumerate() {
                    write_card_field_full(
                        output,
                        &format!("Arg {}", index + 1),
                        &format_hex_bytes(arg),
                    );
                }
            }
        }
        TransactionPayload::ScriptFunction(script_function) => {
            write_card_field_preview(
                output,
                "Target Function",
                &format_script_function_target(script_function),
            );
            if !script_function.ty_args().is_empty() {
                write_card_field_preview(
                    output,
                    "Type Args",
                    &format_type_tags(script_function.ty_args()),
                );
            }
            if let Some(transfer_details) = decode_transfer_script_function(script_function) {
                write_card_field_preview(output, "Transaction Kind", "transfer");
                write_card_field_preview(output, "Recipient", &transfer_details.recipient);
                write_card_field_preview(output, "Amount", &transfer_details.amount);
                if let Some(asset) = transfer_details.asset {
                    write_card_field_preview(output, "Asset", &asset);
                }
            } else {
                write_card_field_preview(
                    output,
                    "Argument Count",
                    &script_function.args().len().to_string(),
                );
            }
            if include_raw_argument_lines {
                for (index, arg) in script_function.args().iter().enumerate() {
                    write_card_field_full(
                        output,
                        &format!("Arg {}", index + 1),
                        &format_hex_bytes(arg),
                    );
                }
            } else if !script_function.args().is_empty() {
                write_card_field_preview(
                    output,
                    "Argument Preview",
                    &format_hex_bytes(script_function.args()[0].as_slice()),
                );
            }
        }
        TransactionPayload::Package(package) => {
            write_card_field_preview(
                output,
                "Package Address",
                &package.package_address().to_string(),
            );
            write_card_field_preview(output, "Module Count", &package.modules().len().to_string());
            write_card_field_preview(
                output,
                "Has Init Script",
                &package.init_script().is_some().to_string(),
            );
            if include_raw_argument_lines {
                for (index, module) in package.modules().iter().enumerate() {
                    write_card_field_full(
                        output,
                        &format!("Module {} Bytes", index + 1),
                        &module.code().len().to_string(),
                    );
                }
            }
        }
    }
}

fn write_message_section(output: &mut String, request: &PulledRequest) {
    write_card_section_heading(output, "Message");
    if let Some(message_format) = request.message_format {
        write_card_field_preview(
            output,
            "Message Format",
            message_format_label(message_format),
        );
    }
    match canonical_message_byte_len(request) {
        Ok(Some(byte_len)) => {
            write_card_field_preview(output, "Byte Length", &byte_len.to_string())
        }
        Ok(None) => {}
        Err(error) => write_card_field_preview(output, "Message Decode Error", &error),
    }
    if let Some(message) = &request.message {
        write_card_field_preview(output, "Canonical Message", message);
    } else {
        write_card_field_preview(output, "Canonical Message", "missing");
    }
}

fn write_context_section(output: &mut String, request: &PulledRequest) {
    if request.display_hint.is_none() && request.client_context.is_none() {
        return;
    }

    write_card_section_heading(output, "Context");
    if let Some(display_hint) = &request.display_hint {
        write_card_field_preview(output, "Display Hint (untrusted)", display_hint);
    }
    if let Some(client_context) = &request.client_context {
        write_card_field_preview(output, "Client Context (untrusted)", client_context);
    }
}

fn write_actions_section(output: &mut String) {
    write_card_section_heading(output, "Actions");
    write_card_text_line(output, "  [a] approve");
    write_card_text_line(output, "  [r] reject");
    write_card_text_line(output, "  [v] view raw canonical payload");
}

fn write_card_section_heading(output: &mut String, heading: &str) {
    write_card_border(output);
    write_card_text_line(output, heading);
}

fn write_card_border(output: &mut String) {
    writeln!(output, "+{}+", "-".repeat(CARD_INNER_WIDTH + 2)).unwrap();
}

fn write_card_text_line(output: &mut String, text: &str) {
    let preview = truncate_chars(text, CARD_INNER_WIDTH);
    let padding = CARD_INNER_WIDTH.saturating_sub(preview.chars().count());
    writeln!(output, "| {}{} |", preview, " ".repeat(padding)).unwrap();
}

fn write_card_field_preview(output: &mut String, label: &str, value: &str) {
    write_card_field(output, label, value, false);
}

fn write_card_field_full(output: &mut String, label: &str, value: &str) {
    write_card_field(output, label, value, true);
}

fn write_card_field(output: &mut String, label: &str, value: &str, full: bool) {
    let sanitized = sanitize_for_tty(value);
    let prefix = format!("  {label}: ");
    let continuation_prefix = " ".repeat(prefix.chars().count());
    let first_width = CARD_INNER_WIDTH.saturating_sub(prefix.chars().count());
    let continuation_width = CARD_INNER_WIDTH.saturating_sub(continuation_prefix.chars().count());

    if !full {
        let preview = truncate_chars(&sanitized, first_width);
        write_card_text_line(output, &format!("{prefix}{preview}"));
        return;
    }

    let mut segments = split_by_width(&sanitized, first_width, continuation_width);
    let first_segment = segments.remove(0);
    write_card_text_line(output, &format!("{prefix}{first_segment}"));
    for segment in segments {
        write_card_text_line(output, &format!("{continuation_prefix}{segment}"));
    }
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    let input_len = input.chars().count();
    if input_len <= max_chars {
        return input.to_owned();
    }
    if max_chars <= PREVIEW_SUFFIX.chars().count() {
        return PREVIEW_SUFFIX.chars().take(max_chars).collect();
    }

    let prefix_len = max_chars - PREVIEW_SUFFIX.chars().count();
    let mut truncated: String = input.chars().take(prefix_len).collect();
    truncated.push_str(PREVIEW_SUFFIX);
    truncated
}

fn split_by_width(input: &str, first_width: usize, continuation_width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_limit = first_width.max(1);

    for character in input.chars() {
        if current.chars().count() == current_limit {
            lines.push(current);
            current = String::new();
            current_limit = continuation_width.max(1);
        }
        current.push(character);
    }

    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn decode_raw_transaction(
    raw_txn_bcs_hex: &str,
) -> std::result::Result<RawUserTransaction, String> {
    let raw_txn_bytes = decode_hex_bytes(raw_txn_bcs_hex)?;
    bcs_ext::from_bytes(&raw_txn_bytes)
        .map_err(|error| format!("Invalid raw transaction payload: {error}"))
}

fn decode_hex_bytes(input: &str) -> std::result::Result<Vec<u8>, String> {
    let trimmed = input.strip_prefix("0x").unwrap_or(input);
    hex::decode(trimmed).map_err(|error| format!("invalid hex payload: {error}"))
}

fn format_hex_bytes(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}

fn format_script_function_target(script_function: &ScriptFunction) -> String {
    format!(
        "{}::{}::{}",
        shorten_account_address(script_function.module().address()),
        script_function.module().name(),
        script_function.function()
    )
}

fn shorten_account_address(address: &AccountAddress) -> String {
    let full = address.to_string();
    let Some(trimmed) = full.strip_prefix("0x") else {
        return full;
    };

    let without_leading_zeroes = trimmed.trim_start_matches('0');
    if without_leading_zeroes.is_empty() {
        "0x0".to_owned()
    } else {
        format!("0x{without_leading_zeroes}")
    }
}

fn format_type_tags(type_tags: &[starcoin_types::language_storage::TypeTag]) -> String {
    type_tags
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn payload_type_label(payload: &TransactionPayload) -> &'static str {
    match payload {
        TransactionPayload::Script(_) => "script",
        TransactionPayload::Package(_) => "package",
        TransactionPayload::ScriptFunction(_) => "script_function",
    }
}

fn request_kind_label(kind: RequestKind) -> &'static str {
    match kind {
        RequestKind::SignTransaction => "sign_transaction",
        RequestKind::SignMessage => "sign_message",
    }
}

fn message_format_label(format: MessageFormat) -> &'static str {
    match format {
        MessageFormat::Utf8 => "utf8",
        MessageFormat::Hex => "hex",
    }
}

fn format_expiration(expiration_timestamp_secs: u64) -> String {
    if expiration_timestamp_secs == u64::MAX {
        "never".to_owned()
    } else {
        format!("{expiration_timestamp_secs} (unix seconds)")
    }
}

fn canonical_message_byte_len(
    request: &PulledRequest,
) -> std::result::Result<Option<usize>, String> {
    let Some(message) = request.message.as_deref() else {
        return Ok(None);
    };
    let Some(format) = request.message_format else {
        return Ok(None);
    };
    match format {
        MessageFormat::Utf8 => Ok(Some(message.len())),
        MessageFormat::Hex => decode_hex_bytes(message)
            .map(|bytes| Some(bytes.len()))
            .map_err(|error| format!("invalid hex message: {error}")),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TransferDetails {
    recipient: String,
    amount: String,
    asset: Option<String>,
}

fn decode_transfer_script_function(script_function: &ScriptFunction) -> Option<TransferDetails> {
    let module = script_function.module();
    if module.address() != &core_code_address() || module.name().as_str() != "TransferScripts" {
        return None;
    }
    if script_function.function().as_str() != "peer_to_peer_v2" {
        return None;
    }
    if script_function.args().len() != 2 {
        return None;
    }

    let recipient: AccountAddress = bcs_ext::from_bytes(&script_function.args()[0]).ok()?;
    let amount: u128 = bcs_ext::from_bytes(&script_function.args()[1]).ok()?;
    let asset = script_function.ty_args().first().map(ToString::to_string);

    Some(TransferDetails {
        recipient: recipient.to_string(),
        amount: amount.to_string(),
        asset,
    })
}

#[cfg(test)]
mod tests {
    use std::io;

    use pretty_assertions::assert_eq;
    use starcoin_account_api::AccountInfo;
    use starcoin_types::{
        account_address::AccountAddress,
        account_config::core_code_address,
        genesis_config::ChainId,
        identifier::Identifier,
        language_storage::ModuleId,
        transaction::{RawUserTransaction, Script, ScriptFunction, TransactionPayload},
    };
    use starmask_types::{
        ClientRequestId, DeliveryLeaseId, MessageFormat, PayloadHash, PulledRequest, RequestId,
        RequestKind,
    };

    use super::{
        parse_prompt_action, render_raw_payload_details, render_request_summary, sanitize_for_tty,
        truncate_chars,
    };

    #[test]
    fn sanitize_for_tty_escapes_control_and_format_sequences_but_preserves_unicode() {
        assert_eq!(
            sanitize_for_tty("hi\nthere\x1b[31m\t\u{202e}你好\u{2066}"),
            "hi\\nthere\\u{1b}[31m\\t\\u{202e}你好\\u{2066}"
        );
    }

    #[test]
    fn sanitize_for_tty_escapes_literal_backslashes() {
        assert_eq!(sanitize_for_tty("\\n\\u{1b}"), "\\\\n\\\\u{1b}");
    }

    #[test]
    fn truncate_chars_adds_suffix_when_needed() {
        assert_eq!(truncate_chars("abcdefgh", 6), "abc...");
        assert_eq!(truncate_chars("abc", 6), "abc");
    }

    #[test]
    fn parse_prompt_action_rejects_closed_stdin() {
        let error = parse_prompt_action(0, "").unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
        assert_eq!(error.to_string(), "approval prompt stdin is closed");
    }

    #[test]
    fn parse_prompt_action_accepts_approve_reject_and_view() {
        assert_eq!(
            parse_prompt_action(2, "a\n").unwrap(),
            super::PromptAction::Approve
        );
        assert_eq!(
            parse_prompt_action(8, "reject\n").unwrap(),
            super::PromptAction::Reject
        );
        assert_eq!(
            parse_prompt_action(5, "view\n").unwrap(),
            super::PromptAction::ViewRaw
        );
    }

    #[test]
    fn parse_prompt_action_rejects_unknown_values() {
        let error = parse_prompt_action(4, "nope\n").unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(error.to_string(), "unknown approval action");
    }

    fn sample_account_info(locked: bool) -> AccountInfo {
        let mut account = AccountInfo::random();
        account.is_locked = locked;
        account
    }

    fn sample_message_request() -> PulledRequest {
        PulledRequest {
            request_id: RequestId::new("req-sign-message").unwrap(),
            client_request_id: ClientRequestId::new("client-sign-message").unwrap(),
            kind: RequestKind::SignMessage,
            account_address: "0x1".to_owned(),
            payload_hash: PayloadHash::new("payload-sign-message").unwrap(),
            display_hint: Some("Friendly summary".to_owned()),
            client_context: Some("phase2-test".to_owned()),
            resume_required: false,
            delivery_lease_id: Some(DeliveryLeaseId::new("lease-sign-message").unwrap()),
            lease_expires_at: None,
            presentation_id: None,
            presentation_expires_at: None,
            raw_txn_bcs_hex: None,
            message: Some("0xdeadbeef".to_owned()),
            message_format: Some(MessageFormat::Hex),
        }
    }

    #[test]
    fn render_request_summary_marks_host_fields_untrusted_but_keeps_canonical_message_primary() {
        let rendered =
            render_request_summary(&sample_message_request(), &sample_account_info(true));

        assert!(rendered.contains("Display Hint (untrusted): Friendly summary"));
        assert!(rendered.contains("Client Context (untrusted): phase2-test"));
        assert!(rendered.contains("Canonical Message: 0xdeadbeef"));
        assert!(rendered.contains("Message Format: hex"));
        assert!(rendered.contains("Account Locked: true"));
        assert!(rendered.contains("[v] view raw canonical payload"));
    }

    #[test]
    fn render_message_sections_surface_invalid_hex_messages() {
        let mut request = sample_message_request();
        request.message = Some("0xnope".to_owned());

        let summary = render_request_summary(&request, &sample_account_info(false));
        let details = render_raw_payload_details(&request);

        assert!(summary.contains("Message Decode Error: invalid hex message:"));
        assert!(details.contains("Message Decode Error: invalid hex message:"));
    }

    #[test]
    fn render_request_summary_decodes_transfer_transaction_fields() {
        let recipient = AccountAddress::random();
        let raw_txn = RawUserTransaction::new_with_default_gas_token(
            AccountAddress::from_hex_literal("0x1").unwrap(),
            7,
            TransactionPayload::ScriptFunction(ScriptFunction::new(
                ModuleId::new(
                    core_code_address(),
                    Identifier::new("TransferScripts").unwrap(),
                ),
                Identifier::new("peer_to_peer_v2").unwrap(),
                vec![],
                vec![
                    bcs_ext::to_bytes(&recipient).unwrap(),
                    bcs_ext::to_bytes(&1234u128).unwrap(),
                ],
            )),
            1_000,
            2,
            100_000,
            ChainId::test(),
        );
        let request = PulledRequest {
            request_id: RequestId::new("req-sign-transaction").unwrap(),
            client_request_id: ClientRequestId::new("client-sign-transaction").unwrap(),
            kind: RequestKind::SignTransaction,
            account_address: "0x1".to_owned(),
            payload_hash: PayloadHash::new("payload-sign-transaction").unwrap(),
            display_hint: Some("Transfer".to_owned()),
            client_context: Some("phase2-test".to_owned()),
            resume_required: false,
            delivery_lease_id: Some(DeliveryLeaseId::new("lease-sign-transaction").unwrap()),
            lease_expires_at: None,
            presentation_id: None,
            presentation_expires_at: None,
            raw_txn_bcs_hex: Some(format!(
                "0x{}",
                hex::encode(bcs_ext::to_bytes(&raw_txn).unwrap())
            )),
            message: None,
            message_format: None,
        };

        let rendered = render_request_summary(&request, &sample_account_info(false));

        assert!(rendered.contains("Transaction Kind: transfer"));
        assert!(rendered.contains("Recipient:"));
        assert!(rendered.contains(&recipient.to_string()));
        assert!(rendered.contains("Amount: 1234"));
        assert!(rendered.contains("Payload Type: script_function"));
        assert!(rendered.contains("Target Function:"));
        assert!(rendered.contains("TransferScripts::peer_to_peer_v2"));
        assert!(rendered.contains("Max Fee Budget: 2000 0x1::STC::STC"));
    }

    #[test]
    fn render_request_summary_does_not_label_nonstandard_transfer_shape_as_transfer() {
        let recipient = AccountAddress::random();
        let raw_txn = RawUserTransaction::new_with_default_gas_token(
            AccountAddress::from_hex_literal("0x1").unwrap(),
            7,
            TransactionPayload::ScriptFunction(ScriptFunction::new(
                ModuleId::new(
                    core_code_address(),
                    Identifier::new("TransferScripts").unwrap(),
                ),
                Identifier::new("peer_to_peer_v2").unwrap(),
                vec![],
                vec![
                    bcs_ext::to_bytes(&recipient).unwrap(),
                    bcs_ext::to_bytes(&1234u128).unwrap(),
                    bcs_ext::to_bytes(&999u64).unwrap(),
                ],
            )),
            1_000,
            2,
            100_000,
            ChainId::test(),
        );
        let request = PulledRequest {
            request_id: RequestId::new("req-sign-transaction-extra-arg").unwrap(),
            client_request_id: ClientRequestId::new("client-sign-transaction-extra-arg").unwrap(),
            kind: RequestKind::SignTransaction,
            account_address: "0x1".to_owned(),
            payload_hash: PayloadHash::new("payload-sign-transaction-extra-arg").unwrap(),
            display_hint: Some("Transfer".to_owned()),
            client_context: Some("phase2-test".to_owned()),
            resume_required: false,
            delivery_lease_id: Some(
                DeliveryLeaseId::new("lease-sign-transaction-extra-arg").unwrap(),
            ),
            lease_expires_at: None,
            presentation_id: None,
            presentation_expires_at: None,
            raw_txn_bcs_hex: Some(format!(
                "0x{}",
                hex::encode(bcs_ext::to_bytes(&raw_txn).unwrap())
            )),
            message: None,
            message_format: None,
        };

        let rendered = render_request_summary(&request, &sample_account_info(false));

        assert!(!rendered.contains("Transaction Kind: transfer"));
        assert!(rendered.contains("Argument Count: 3"));
    }

    #[test]
    fn render_request_summary_handles_script_transactions_without_decoding_transfer() {
        let raw_txn = RawUserTransaction::new_with_default_gas_token(
            AccountAddress::from_hex_literal("0x1").unwrap(),
            1,
            TransactionPayload::Script(Script::new(vec![0xaa, 0xbb], vec![], vec![vec![1, 2]])),
            100,
            1,
            5,
            ChainId::test(),
        );
        let request = PulledRequest {
            request_id: RequestId::new("req-script").unwrap(),
            client_request_id: ClientRequestId::new("client-script").unwrap(),
            kind: RequestKind::SignTransaction,
            account_address: "0x1".to_owned(),
            payload_hash: PayloadHash::new("payload-script").unwrap(),
            display_hint: None,
            client_context: None,
            resume_required: false,
            delivery_lease_id: None,
            lease_expires_at: None,
            presentation_id: None,
            presentation_expires_at: None,
            raw_txn_bcs_hex: Some(format!(
                "0x{}",
                hex::encode(bcs_ext::to_bytes(&raw_txn).unwrap())
            )),
            message: None,
            message_format: None,
        };

        let rendered = render_request_summary(&request, &sample_account_info(false));

        assert!(rendered.contains("Payload Type: script"));
        assert!(rendered.contains("Script Code Bytes: 2"));
        assert!(rendered.contains("Argument Count: 1"));
    }

    #[test]
    fn render_raw_payload_details_shows_full_payload_content() {
        let rendered = render_raw_payload_details(&sample_message_request());

        assert!(rendered.contains("Starmask Canonical Payload Details"));
        assert!(rendered.contains("Canonical Message"));
        assert!(rendered.contains("Message: 0xdeadbeef"));
        assert!(rendered.contains("Byte Length: 4"));
    }
}
