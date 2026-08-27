//! Round-trip test for persistent user env vars against a unique test name.

#![cfg(windows)]

use codex_kimi_switch::env_var::{get_persistent, remove_persistent, set_persistent};

#[test]
fn persistent_env_var_round_trips() -> Result<(), Box<dyn std::error::Error>> {
    const NAME: &str = "CODEX_KIMI_SWITCH_TEST_VAR";

    // Given: the current persistent state of a unique test variable.
    let original = get_persistent(NAME);

    // When: a value is written persistently.
    set_persistent(NAME, "roundtrip-value")?;

    // Then: it reads back, and the original state is restorable.
    assert_eq!(get_persistent(NAME).as_deref(), Some("roundtrip-value"));
    match &original {
        Some(value) => set_persistent(NAME, value)?,
        None => remove_persistent(NAME)?,
    }
    assert_eq!(get_persistent(NAME), original);
    Ok(())
}
