module 0x42::M {

    #[allow(lint(constant_naming))]
    const Another_BadName: u64 = 42; // Should trigger a warning

    #[allow(lint(unnecessary_while_loop))]
    public fun finite_loop() {
        let counter = 0;
        while (true) {
            if(counter == 10) {
                break
            };
            counter = counter + 1;
        }
    }
}
