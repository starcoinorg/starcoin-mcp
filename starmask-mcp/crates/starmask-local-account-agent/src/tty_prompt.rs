#![forbid(unsafe_code)]

use std::{
    fmt::Write as _,
    io::{self, Write},
};

use starcoin_account_api::AccountInfo;
use starmask_types::{MessageFormat, PulledRequest, RequestKind, WalletCapability};

use crate::request_support::{RequestRejection, ensure_local_unlock_capability};

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

        let approved =
            prompt_yes_no("Approve request? [y/N]: ").map_err(|error| RequestRejection {
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

fn prompt_yes_no(prompt: &str) -> io::Result<bool> {
    let mut stderr = io::stderr().lock();
    stderr.write_all(prompt.as_bytes())?;
    stderr.flush()?;

    let mut line = String::new();
    let bytes_read = io::stdin().read_line(&mut line)?;
    parse_prompt_yes_no(bytes_read, &line)
}

fn parse_prompt_yes_no(bytes_read: usize, line: &str) -> io::Result<bool> {
    if bytes_read == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "approval prompt stdin is closed",
        ));
    }

    let normalized = line.trim().to_ascii_lowercase();
    Ok(matches!(normalized.as_str(), "y" | "yes"))
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

fn write_tty_field(output: &mut String, label: &str, value: &str) {
    writeln!(output, "  {label}: {}", sanitize_for_tty(value)).unwrap();
}

fn write_untrusted_tty_field(output: &mut String, label: &str, value: &str) {
    writeln!(output, "  {label} (untrusted): {}", sanitize_for_tty(value)).unwrap();
}

fn print_request_summary(request: &PulledRequest, account_info: &AccountInfo) {
    eprint!("{}", render_request_summary(request, account_info));
}

fn render_request_summary(request: &PulledRequest, account_info: &AccountInfo) -> String {
    let mut output = String::new();
    output.push('\n');
    writeln!(output, "Starmask Local Signing Request").unwrap();
    writeln!(output, "  Request ID: {}", request.request_id).unwrap();
    let client_request_id = request.client_request_id.to_string();
    write_untrusted_tty_field(&mut output, "Client Request ID", &client_request_id);
    write_tty_field(&mut output, "Account", &request.account_address);
    writeln!(output, "  Account Locked: {}", account_info.is_locked).unwrap();
    writeln!(output, "  Kind: {}", request_kind_label(request.kind)).unwrap();
    let payload_hash = request.payload_hash.to_string();
    write_tty_field(&mut output, "Payload Hash", &payload_hash);
    if let Some(display_hint) = &request.display_hint {
        write_untrusted_tty_field(&mut output, "Display Hint", display_hint);
    }
    if let Some(client_context) = &request.client_context {
        write_untrusted_tty_field(&mut output, "Client Context", client_context);
    }

    match request.kind {
        RequestKind::SignTransaction => {
            if let Some(raw_txn_bcs_hex) = &request.raw_txn_bcs_hex {
                write_untrusted_tty_field(&mut output, "Raw Transaction BCS", raw_txn_bcs_hex);
            }
        }
        RequestKind::SignMessage => {
            if let Some(message_format) = request.message_format {
                writeln!(
                    output,
                    "  Message Format: {}",
                    message_format_label(message_format)
                )
                .unwrap();
            }
            if let Some(message) = &request.message {
                write_untrusted_tty_field(&mut output, "Canonical Message", message);
            }
        }
    }
    output.push('\n');
    output
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

#[cfg(test)]
mod tests {
    use std::io;

    use pretty_assertions::assert_eq;
    use starcoin_account_api::AccountInfo;
    use starmask_types::{
        ClientRequestId, DeliveryLeaseId, MessageFormat, PayloadHash, PulledRequest, RequestId,
        RequestKind,
    };

    use super::{parse_prompt_yes_no, render_request_summary, sanitize_for_tty};

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
    fn parse_prompt_yes_no_rejects_closed_stdin() {
        let error = parse_prompt_yes_no(0, "").unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
        assert_eq!(error.to_string(), "approval prompt stdin is closed");
    }

    #[test]
    fn parse_prompt_yes_no_accepts_only_affirmative_answers() {
        assert!(parse_prompt_yes_no(2, "y\n").unwrap());
        assert!(parse_prompt_yes_no(4, "YeS\n").unwrap());
        assert!(!parse_prompt_yes_no(2, "n\n").unwrap());
        assert!(!parse_prompt_yes_no(1, "\n").unwrap());
    }

    fn sample_account_info(locked: bool) -> AccountInfo {
        let mut account = AccountInfo::random();
        account.is_locked = locked;
        account
    }

    #[test]
    fn render_request_summary_marks_host_fields_untrusted_but_keeps_canonical_message() {
        let request = PulledRequest {
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
        };

        let rendered = render_request_summary(&request, &sample_account_info(true));

        assert!(rendered.contains("Display Hint (untrusted): Friendly summary"));
        assert!(rendered.contains("Client Context (untrusted): phase2-test"));
        assert!(rendered.contains("Canonical Message (untrusted): 0xdeadbeef"));
        assert!(rendered.contains("Message Format: hex"));
        assert!(rendered.contains("Account Locked: true"));
    }

    #[test]
    fn render_request_summary_includes_raw_transaction_bytes_for_sign_transaction() {
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
            raw_txn_bcs_hex: Some("0xabc123".to_owned()),
            message: None,
            message_format: None,
        };

        let rendered = render_request_summary(&request, &sample_account_info(false));

        assert!(rendered.contains("Kind: sign_transaction"));
        assert!(rendered.contains("Raw Transaction BCS (untrusted): 0xabc123"));
        assert!(rendered.contains("Display Hint (untrusted): Transfer"));
        assert!(rendered.contains("Account Locked: false"));
    }
}
