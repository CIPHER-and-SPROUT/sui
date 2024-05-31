module 0x42::M {

    #[allow(lint(constant_naming))]
    const Another_BadName: u64 = 42; // Should trigger a warning

    #[allow(lint(empty_loop))]
    public fun while_infinite_loop_always_true() {
        while (true) {
            // Intentionally left empty for testing
        }
    }
}
