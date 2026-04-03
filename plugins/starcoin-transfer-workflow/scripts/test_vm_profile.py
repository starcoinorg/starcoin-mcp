#!/usr/bin/env python3
from __future__ import annotations

import unittest

from node_cli_client import rewrite_vm_profile_config_text
from transfer_controller import (
    CANONICAL_STC_TOKEN_CODE,
    VM1_STC_TOKEN_CODE,
    default_token_code_for_vm_profile,
    normalize_vm_profile,
    resolve_token_code,
)


class VmProfileTests(unittest.TestCase):
    def test_normalize_vm_profile_accepts_kebab_case_alias(self) -> None:
        self.assertEqual(normalize_vm_profile("vm1-only"), "vm1_only")

    def test_normalize_vm_profile_rejects_unknown_value(self) -> None:
        with self.assertRaisesRegex(ValueError, "vm profile must be one of"):
            normalize_vm_profile("vm3_only")

    def test_default_token_code_for_vm1_only_uses_vm1_stc(self) -> None:
        self.assertEqual(default_token_code_for_vm_profile("vm1_only"), VM1_STC_TOKEN_CODE)

    def test_default_token_code_for_auto_uses_vm2_stc(self) -> None:
        self.assertEqual(
            default_token_code_for_vm_profile("auto"),
            CANONICAL_STC_TOKEN_CODE,
        )

    def test_resolve_token_code_uses_vm_profile_when_token_is_omitted(self) -> None:
        self.assertEqual(resolve_token_code(None, "vm1_only"), VM1_STC_TOKEN_CODE)
        self.assertEqual(resolve_token_code(None, "vm2_only"), CANONICAL_STC_TOKEN_CODE)

    def test_rewrite_vm_profile_config_replaces_existing_value(self) -> None:
        original = '\n'.join(
            [
                'rpc_endpoint_url = "http://127.0.0.1:9850"',
                'mode = "transaction"',
                'vm_profile = "auto"',
                "",
            ]
        )
        rewritten = rewrite_vm_profile_config_text(original, "vm1_only")
        self.assertIn('vm_profile = "vm1_only"', rewritten)
        self.assertNotIn('vm_profile = "auto"', rewritten)

    def test_rewrite_vm_profile_config_appends_missing_value(self) -> None:
        original = '\n'.join(
            [
                'rpc_endpoint_url = "http://127.0.0.1:9850"',
                'mode = "transaction"',
            ]
        )
        rewritten = rewrite_vm_profile_config_text(original, "vm2_only")
        self.assertTrue(rewritten.endswith('vm_profile = "vm2_only"\n'))


if __name__ == "__main__":
    unittest.main()
