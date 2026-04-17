use crate::{
    commands::{
        CreateAccountCommand, CreateExportAccountCommand, CreateImportAccountCommand,
        CreateSignMessageCommand, CreateSignTransactionCommand,
    },
    error::CoreResult,
};

pub trait PolicyEngine {
    fn check_account_listing(&self) -> CoreResult<()> {
        Ok(())
    }

    fn check_public_key_lookup(&self, _address: &str) -> CoreResult<()> {
        Ok(())
    }

    fn check_create_sign_transaction(
        &self,
        _command: &CreateSignTransactionCommand,
    ) -> CoreResult<()> {
        Ok(())
    }

    fn check_create_sign_message(&self, _command: &CreateSignMessageCommand) -> CoreResult<()> {
        Ok(())
    }

    fn check_create_account(&self, _command: &CreateAccountCommand) -> CoreResult<()> {
        Ok(())
    }

    fn check_create_export_account(&self, _command: &CreateExportAccountCommand) -> CoreResult<()> {
        Ok(())
    }

    fn check_create_import_account(&self, _command: &CreateImportAccountCommand) -> CoreResult<()> {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AllowAllPolicy;

impl PolicyEngine for AllowAllPolicy {}
