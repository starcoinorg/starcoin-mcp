#![forbid(unsafe_code)]

use std::io::{self, Write};

use starcoin_account_api::AccountInfo;
use starmask_types::{MessageFormat, PulledRequest, RequestKind, WalletCapability};

use crate::request_support::{RequestRejection, ensure_local_unlock_capability};

#[derive(Clone, Debug)]
pub(crate) struct PromptApproval {
    approved: bool,
    password: Option<String>,
}

impl PromptApproval {
    pub(crate) fn approved(&self) -> bool {
        self.approved
    }

    pub(crate) fn password(&self) -> Option<&str> {
        self.password.as_deref()
    }
}

pub(crate) fn prompt_for_request(
    request: &PulledRequest,
    account_info: &AccountInfo,
    capabilities: &[WalletCapability],
) -> std::result::Result<PromptApproval, RequestRejection> {
    ensure_local_unlock_capability(account_info.is_locked, capabilities)?;
    print_request_summary(request, account_info);

    let approved = prompt_yes_no("Approve request? [y/N]: ").map_err(|error| RequestRejection {
        reason_code: starmask_types::RejectReasonCode::BackendUnavailable,
        message: Some(format!("Failed to read local approval input: {error}")),
    })?;
    if !approved {
        return Ok(PromptApproval {
            approved: false,
            password: None,
        });
    }

    let password = if account_info.is_locked {
        let password =
            rpassword::prompt_password("Account password: ").map_err(|error| RequestRejection {
                reason_code: starmask_types::RejectReasonCode::BackendUnavailable,
                message: Some(format!("Failed to read account password: {error}")),
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

fn sanitize_for_tty(input: &str) -> String {
    let mut sanitized = String::with_capacity(input.len());
    for character in input.chars() {
        match character {
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

fn print_tty_field(label: &str, value: &str) {
    eprintln!("  {label}: {}", sanitize_for_tty(value));
}

fn print_untrusted_tty_field(label: &str, value: &str) {
    eprintln!("  {label} (untrusted): {}", sanitize_for_tty(value));
}

fn print_request_summary(request: &PulledRequest, account_info: &AccountInfo) {
    eprintln!();
    eprintln!("Starmask Local Signing Request");
    eprintln!("  Request ID: {}", request.request_id);
    let client_request_id = request.client_request_id.to_string();
    print_untrusted_tty_field("Client Request ID", &client_request_id);
    print_tty_field("Account", &request.account_address);
    eprintln!("  Account Locked: {}", account_info.is_locked);
    eprintln!("  Kind: {}", request_kind_label(request.kind));
    let payload_hash = request.payload_hash.to_string();
    print_tty_field("Payload Hash", &payload_hash);
    if let Some(display_hint) = &request.display_hint {
        print_untrusted_tty_field("Display Hint", display_hint);
    }
    if let Some(client_context) = &request.client_context {
        print_untrusted_tty_field("Client Context", client_context);
    }

    match request.kind {
        RequestKind::SignTransaction => {
            if let Some(raw_txn_bcs_hex) = &request.raw_txn_bcs_hex {
                print_untrusted_tty_field("Raw Transaction BCS", raw_txn_bcs_hex);
            }
        }
        RequestKind::SignMessage => {
            if let Some(message_format) = request.message_format {
                eprintln!("  Message Format: {}", message_format_label(message_format));
            }
            if let Some(message) = &request.message {
                print_untrusted_tty_field("Canonical Message", message);
            }
        }
    }
    eprintln!();
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

fn prompt_yes_no(prompt: &str) -> io::Result<bool> {
    let mut stderr = io::stderr().lock();
    stderr.write_all(prompt.as_bytes())?;
    stderr.flush()?;

    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    let normalized = line.trim().to_ascii_lowercase();
    Ok(matches!(normalized.as_str(), "y" | "yes"))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::sanitize_for_tty;

    #[test]
    fn sanitize_for_tty_escapes_control_and_format_sequences_but_preserves_unicode() {
        assert_eq!(
            sanitize_for_tty("hi\nthere\x1b[31m\t\u{202e}你好\u{2066}"),
            "hi\\nthere\\u{1b}[31m\\t\\u{202e}你好\\u{2066}"
        );
    }
}
