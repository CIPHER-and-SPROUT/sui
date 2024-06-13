module 0x42::M {
    use std::vector;
    
    #[allow(lint(constant_naming))]
    const Another_BadName: u64 = 42; // Should trigger a warning

    #[allow(lint(out_of_bounds_indexing))]
    fun out_of_bound_index() {
        let arr2 = vector[1, 2, 3, 4, 5];
        vector::push_back(&mut arr2, 6);
        vector::push_back(&mut arr2, 6);
        vector::push_back(&mut arr2, 6);
        vector::pop_back(&mut arr2);
        vector::pop_back(&mut arr2);

        vector::borrow(&arr2, 7);
    }
}
